//! 系统初始化种子：初始管理员与默认菜单。
//!
//! # 职责
//!
//! 首次启动时创建默认租户 + 管理员账号，并为所有租户补齐默认菜单。
//!
//! # 边界
//!
//! 只做幂等初始化，不覆盖已有账号/菜单；业务数据不在这里创建。
//!
//! # 关键约束
//!
//! 从 `system.rs` 拆出；`app.rs` 与 `tenant_admin` 仍通过
//! `system::ensure_seed` / `system::ensure_default_menus` 调用（system 重新导出）。

use crate::config::AuthConfig;
use crate::password;
use uuid::Uuid;

/// 确保系统里至少有一个可登录的管理员。
///
/// 只在 `sys_user` 为空时执行，**不会覆盖已有账号**（避免每次重启重置密码）。
/// 初始密码来自配置 `auth.dev_password`，生产环境必须用环境变量覆盖，
/// 并在首次登录后强制修改（M1-b 实现 `must_change_password`）。
/// 初始管理员是否强制首登改密。
///
/// # 为什么不能只看「有没有设置 AUTH_DEV_PASSWORD」
///
/// 旧口径是「设置了环境变量就不强制改密」。运维只要把口令设成
/// **符合强度策略但可猜**的值（例如 `Admin123456`），这条控制就完全失效：
/// 账号可以带着一个人人可猜的口令无限期使用。
///
/// 现在的判定顺序：
/// 1. `AUTH_FORCE_PASSWORD_CHANGE`（true/false）显式指定时以它为准，
///    给自动化部署留逃生口（脚本播种后立刻调接口的场景）；
/// 2. **release 构建一律强制**：生产初始口令必然来自运维手填，必须首登改掉；
/// 3. debug 构建沿用旧口径（未覆盖 AUTH_DEV_PASSWORD 才强制），
///    避免本地开发与 CI 每次都先改密码。
fn initial_admin_must_change() -> bool {
    if let Ok(raw) = std::env::var("AUTH_FORCE_PASSWORD_CHANGE") {
        match raw.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => return true,
            "0" | "false" | "no" | "off" => return false,
            other => tracing::warn!(
                value = other,
                "AUTH_FORCE_PASSWORD_CHANGE 取值无法识别，按默认口径处理"
            ),
        }
    }
    !cfg!(debug_assertions) || std::env::var("AUTH_DEV_PASSWORD").is_err()
}

/// 确保系统里至少有一个可登录的管理员；初始化哈希通过受控阻塞池生成。
pub async fn ensure_seed(pool: &sqlx::PgPool, cfg: &AuthConfig) -> anyhow::Result<()> {
    // 用 sys_tenant 判断是否已初始化：
    // sys_user 在 RLS FORCE 后需要租户上下文，启动时还没有
    let tenant_count: i64 = sqlx::query_scalar("SELECT count(*) FROM sys.sys_tenant")
        .fetch_one(pool)
        .await?;
    if tenant_count > 0 {
        return Ok(());
    }

    // 生产（release）构建：必须显式设置强初始密码，避免弱口令进入生产。
    if !cfg!(debug_assertions) {
        let pwd = std::env::var("AUTH_DEV_PASSWORD")
            .map_err(|_| anyhow::anyhow!("生产环境必须设置 AUTH_DEV_PASSWORD"))?;
        if let Err(e) = password::validate_strength(&pwd) {
            anyhow::bail!("AUTH_DEV_PASSWORD 强度不足: {e}");
        }
    }

    let tenant_id = Uuid::now_v7();
    let user_id = Uuid::now_v7();
    let hash = password::hash_password_async(cfg.dev_password.clone()).await?;

    let mut tx = pool.begin().await?;

    sqlx::query(
        "INSERT INTO sys.sys_tenant (id, code, name, status) VALUES ($1, 'default', '默认租户', 'active')",
    )
    .bind(tenant_id)
    .execute(&mut *tx)
    .await?;

    // RLS：后续写入 sys_user / sys_menu 需要租户上下文
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant_id.to_string())
        .execute(&mut *tx)
        .await?;

    sqlx::query(
        r#"
        INSERT INTO sys.sys_user
            (id, tenant_id, username, display_name, password_hash, is_super, status,
             must_change_password)
        VALUES ($1, $2, $3, '系统管理员', $4, true, 'active', $5)
        "#,
    )
    .bind(user_id)
    .bind(tenant_id)
    .bind(&cfg.dev_username)
    .bind(&hash)
    .bind(initial_admin_must_change())
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    tracing::warn!(
        username = %cfg.dev_username,
        "已创建初始管理员账号，请立即修改密码（生产环境务必用 AUTH_DEV_PASSWORD 覆盖默认值）"
    );
    Ok(())
}

/// 为所有租户补齐默认菜单（幂等）。
///
/// 为什么不在迁移里做：迁移执行时可能还没有租户（全新库），
/// 而租户是在 [`ensure_seed`] 里创建的。所以菜单初始化必须放在种子之后，
/// 并且每次启动都跑一遍（幂等），这样"老租户新增菜单"也能自动补上。
pub async fn ensure_default_menus(pool: &sqlx::PgPool) -> anyhow::Result<()> {
    const STATEMENTS: &[&str] = &[
        r#"
        INSERT INTO sys.sys_menu (id, tenant_id, parent_id, title, path, icon, perm, sort)
        SELECT gen_random_uuid(), $1, NULL, '工作台', '/', 'home', NULL, 0
           WHERE NOT EXISTS (SELECT 1 FROM sys.sys_menu m WHERE m.tenant_id = $1 AND m.path = '/')
        "#,
        r#"
        INSERT INTO sys.sys_menu (id, tenant_id, parent_id, title, path, icon, perm, sort)
        SELECT gen_random_uuid(), $1, NULL, '系统管理', '/system', 'setting', NULL, 10
           WHERE NOT EXISTS (SELECT 1 FROM sys.sys_menu m WHERE m.tenant_id = $1 AND m.path = '/system')
        "#,
        r#"
        INSERT INTO sys.sys_menu (id, tenant_id, parent_id, title, path, icon, perm, sort)
        SELECT gen_random_uuid(), $1, p.id, '用户管理', '/system/users', 'team', 'system:user:list', 11
           FROM sys.sys_menu p
           WHERE p.tenant_id = $1 AND p.path = '/system'
             AND NOT EXISTS (SELECT 1 FROM sys.sys_menu m WHERE m.tenant_id = $1 AND m.path = '/system/users')
        "#,
        r#"
        INSERT INTO sys.sys_menu (id, tenant_id, parent_id, title, path, icon, perm, sort)
        SELECT gen_random_uuid(), $1, p.id, '角色管理', '/system/roles', 'safety', 'system:role:list', 12
           FROM sys.sys_menu p
           WHERE p.tenant_id = $1 AND p.path = '/system'
             AND NOT EXISTS (SELECT 1 FROM sys.sys_menu m WHERE m.tenant_id = $1 AND m.path = '/system/roles')
        "#,
        r#"
        INSERT INTO sys.sys_menu (id, tenant_id, parent_id, title, path, icon, perm, sort)
        SELECT gen_random_uuid(), $1, p.id, '字典管理', '/system/dicts', 'book', 'system:dict:list', 13
           FROM sys.sys_menu p
           WHERE p.tenant_id = $1 AND p.path = '/system'
             AND NOT EXISTS (SELECT 1 FROM sys.sys_menu m WHERE m.tenant_id = $1 AND m.path = '/system/dicts')
        "#,
        r#"
        INSERT INTO sys.sys_menu (id, tenant_id, parent_id, title, path, icon, perm, sort)
        SELECT gen_random_uuid(), $1, p.id, '文件管理', '/system/files', 'file', 'system:file:list', 14
           FROM sys.sys_menu p
           WHERE p.tenant_id = $1 AND p.path = '/system'
             AND NOT EXISTS (SELECT 1 FROM sys.sys_menu m WHERE m.tenant_id = $1 AND m.path = '/system/files')
        "#,
        r#"
        INSERT INTO sys.sys_menu (id, tenant_id, parent_id, title, path, icon, perm, sort)
        SELECT gen_random_uuid(), $1, p.id, '通知管理', '/system/notices', 'bell', 'system:notice:list', 15
           FROM sys.sys_menu p
           WHERE p.tenant_id = $1 AND p.path = '/system'
             AND NOT EXISTS (SELECT 1 FROM sys.sys_menu m WHERE m.tenant_id = $1 AND m.path = '/system/notices')
        "#,
        r#"
        INSERT INTO sys.sys_menu (id, tenant_id, parent_id, title, path, icon, perm, sort)
        SELECT gen_random_uuid(), $1, p.id, '部门管理', '/system/depts', 'cluster', 'system:user:list', 16
           FROM sys.sys_menu p
           WHERE p.tenant_id = $1 AND p.path = '/system'
             AND NOT EXISTS (SELECT 1 FROM sys.sys_menu m WHERE m.tenant_id = $1 AND m.path = '/system/depts')
        "#,
        r#"
        INSERT INTO sys.sys_menu (id, tenant_id, parent_id, title, path, icon, perm, sort)
        SELECT gen_random_uuid(), $1, p.id, '审计日志', '/system/audit', 'audit', 'system:audit:list', 17
           FROM sys.sys_menu p
           WHERE p.tenant_id = $1 AND p.path = '/system'
             AND NOT EXISTS (SELECT 1 FROM sys.sys_menu m WHERE m.tenant_id = $1 AND m.path = '/system/audit')
        "#,
        r#"
        INSERT INTO sys.sys_menu (id, tenant_id, parent_id, title, path, icon, perm, sort)
        SELECT gen_random_uuid(), $1, p.id, '租户管理', '/system/tenants', 'apartment', 'system:tenant:list', 19
           FROM sys.sys_menu p
           WHERE p.tenant_id = $1 AND p.path = '/system'
             AND NOT EXISTS (SELECT 1 FROM sys.sys_menu m WHERE m.tenant_id = $1 AND m.path = '/system/tenants')
        "#,
        r#"
        INSERT INTO sys.sys_menu (id, tenant_id, parent_id, title, path, icon, perm, sort)
        SELECT gen_random_uuid(), $1, p.id, '参数配置', '/system/configs', 'setting', 'system:config:list', 18
           FROM sys.sys_menu p
           WHERE p.tenant_id = $1 AND p.path = '/system'
             AND NOT EXISTS (SELECT 1 FROM sys.sys_menu m WHERE m.tenant_id = $1 AND m.path = '/system/configs')
        "#,
        r#"
        INSERT INTO sys.sys_menu (id, tenant_id, parent_id, title, path, icon, perm, sort)
        SELECT gen_random_uuid(), $1, p.id, '菜单管理', '/system/menus', 'menu', 'system:menu:list', 19
        FROM sys.sys_menu p
        WHERE p.tenant_id = $1 AND p.path = '/system'
          AND NOT EXISTS (SELECT 1 FROM sys.sys_menu m WHERE m.tenant_id = $1 AND m.path = '/system/menus')
        "#,
        r#"
        INSERT INTO sys.sys_menu (id, tenant_id, parent_id, title, path, icon, perm, sort)
        SELECT gen_random_uuid(), $1, p.id, '导出任务', '/system/exports', 'download', 'system:export:list', 20
        FROM sys.sys_menu p
        WHERE p.tenant_id = $1 AND p.path = '/system'
          AND NOT EXISTS (SELECT 1 FROM sys.sys_menu m WHERE m.tenant_id = $1 AND m.path = '/system/exports')
        "#,
        r#"
        INSERT INTO sys.sys_menu (id, tenant_id, parent_id, title, path, icon, perm, sort)
        SELECT gen_random_uuid(), $1, p.id, '岗位管理', '/system/posts', 'idcard', 'system:post:list', 21
        FROM sys.sys_menu p
        WHERE p.tenant_id = $1 AND p.path = '/system'
          AND NOT EXISTS (SELECT 1 FROM sys.sys_menu m WHERE m.tenant_id = $1 AND m.path = '/system/posts')
        "#,
        r#"
        INSERT INTO sys.sys_menu (id, tenant_id, parent_id, title, path, icon, perm, sort)
        SELECT gen_random_uuid(), $1, p.id, '运维大屏', '/system/ops', 'dashboard', 'system:audit:list', 22
        FROM sys.sys_menu p
        WHERE p.tenant_id = $1 AND p.path = '/system'
          AND NOT EXISTS (SELECT 1 FROM sys.sys_menu m WHERE m.tenant_id = $1 AND m.path = '/system/ops')
        "#,
    ];

    // 每个租户一个事务：注入 app.tenant_id 后写菜单，RLS 才能通过
    let tenants: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM sys.sys_tenant WHERE deleted_at IS NULL")
            .fetch_all(pool)
            .await?;

    for tenant_id in tenants {
        let mut tx = pool.begin().await?;
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant_id.to_string())
            .execute(&mut *tx)
            .await?;
        for sql in STATEMENTS {
            sqlx::query(sql).bind(tenant_id).execute(&mut *tx).await?;
        }
        tx.commit().await?;
    }
    Ok(())
}
