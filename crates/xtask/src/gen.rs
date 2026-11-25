//! `cargo xtask gen` —— 从 schema.toml 生成一套 CRUD。
//!
//! 生成三样东西：
//! 1. `plugins/{plugin}/migrations/{seq}_create_{table}.sql`
//! 2. `plugins/{plugin}/src/{table}.rs`（模型 + 5 个接口 + router）
//! 3. 控制台打印挂载步骤（不自动改 lib.rs，避免破坏手写代码）
//!
//! 设计取舍：**只生成骨架，不生成业务规则**。
//! 生成物可以自由修改，改完就是你的代码；schema 只在第一次生成时有用。

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use serde::Deserialize;

/// schema 文件根结构。
#[derive(Debug, Deserialize)]
pub struct SchemaFile {
    /// 实体元信息。
    pub entity: Entity,
    /// 字段列表。
    #[serde(default)]
    pub field: Vec<Field>,
}

/// 实体元信息。
#[derive(Debug, Deserialize)]
pub struct Entity {
    /// 所属插件（决定目录与 schema 名）。
    pub plugin: String,
    /// 表名（snake_case）。
    pub table: String,
    /// 中文名称。
    pub name: String,
    /// 路由片段，例如 `/contacts`。
    pub route: String,
    /// 负责人字段（数据权限 self 依据，可选）。
    #[serde(default)]
    pub owner_field: Option<String>,
    /// 部门字段（数据权限 dept 依据，可选）。
    #[serde(default)]
    pub dept_field: Option<String>,
}

/// 字段定义。
#[derive(Debug, Deserialize)]
pub struct Field {
    /// 列名（snake_case）。
    pub name: String,
    /// 类型：uuid / string / text / int / bool / datetime。
    #[serde(rename = "type")]
    pub ty: String,
    /// string 长度。
    #[serde(default)]
    pub length: Option<usize>,
    /// 是否必填。
    #[serde(default)]
    pub required: bool,
    /// 是否在列表展示（仅注释用途）。
    #[serde(default)]
    pub list: bool,
    /// 搜索方式：ct（模糊）。
    #[serde(default)]
    pub search: Option<String>,
    /// 系统托管字段（owner_id / dept_id），不出现在请求体。
    #[serde(default)]
    pub auto: bool,
}

/// 生成入口。
pub fn generate(schema_path: &str, root: &Path) -> Result<(), String> {
    let raw = fs::read_to_string(schema_path)
        .map_err(|e| format!("读取 schema 失败 {schema_path}: {e}"))?;
    let schema: SchemaFile = toml::from_str(&raw).map_err(|e| format!("解析 schema 失败: {e}"))?;
    validate(&schema)?;

    let plugin_dir = root.join("plugins").join(&schema.entity.plugin);
    if !plugin_dir.exists() {
        return Err(format!(
            "插件目录不存在: {}（先执行 cargo xtask new-plugin {}）",
            plugin_dir.display(),
            schema.entity.plugin
        ));
    }

    let migration = render_migration(&schema);
    let code = render_rust(&schema)?;

    let seq = next_migration_seq(&plugin_dir)?;
    let sql_path = plugin_dir
        .join("migrations")
        .join(format!("{seq}_create_{}.sql", schema.entity.table));
    let rs_path = plugin_dir
        .join("src")
        .join(format!("{}.rs", schema.entity.table));

    fs::write(&sql_path, migration).map_err(|e| format!("写入迁移失败: {e}"))?;
    fs::write(&rs_path, code).map_err(|e| format!("写入代码失败: {e}"))?;

    println!("✅ 迁移: {}", sql_path.display());
    println!("✅ 代码: {}", rs_path.display());
    println!(
        "\n还需要两步（在 plugins/{}/src/lib.rs）：",
        schema.entity.plugin
    );
    println!("  1) 顶部加: mod {};", schema.entity.table);
    println!(
        "  2) router() 里加: .merge({}::router())",
        schema.entity.table
    );
    println!(
        "  3) 把 {}::list/detail/create/update/delete 加进插件的 OpenAPI paths",
        schema.entity.table
    );
    println!(
        "  4) 在插件 permissions() 里声明：{}:{}:list / add / edit / delete",
        schema.entity.plugin, schema.entity.table
    );
    Ok(())
}

fn validate(s: &SchemaFile) -> Result<(), String> {
    if s.field.is_empty() {
        return Err("schema 至少要有一个字段".into());
    }
    for f in &s.field {
        match f.ty.as_str() {
            "uuid" | "string" | "text" | "int" | "bool" | "datetime" => {}
            other => return Err(format!("不支持的字段类型: {other}（字段 {}）", f.name)),
        }
        if f.auto && f.ty != "uuid" {
            return Err(format!("auto 字段必须是 uuid: {}", f.name));
        }
    }
    let has_search = s
        .field
        .iter()
        .any(|f| f.search.is_some() && matches!(f.ty.as_str(), "string" | "text"));
    if !has_search {
        return Err("至少需要一个 search = \"ct\" 的 string/text 字段".into());
    }
    for (label, name) in [
        ("owner_field", &s.entity.owner_field),
        ("dept_field", &s.entity.dept_field),
    ] {
        if let Some(n) = name {
            if !s.field.iter().any(|f| &f.name == n) {
                return Err(format!("{label} `{n}` 不在字段列表中"));
            }
        }
    }
    Ok(())
}

fn pascal(name: &str) -> String {
    name.split('_')
        .map(|p| {
            let mut c = p.chars();
            match c.next() {
                Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

fn sql_type(f: &Field) -> String {
    match f.ty.as_str() {
        "uuid" => "uuid".into(),
        "string" => format!("varchar({})", f.length.unwrap_or(64)),
        "text" => "text".into(),
        "int" => "int".into(),
        "bool" => "boolean".into(),
        "datetime" => "timestamptz".into(),
        _ => "text".into(),
    }
}

fn rust_type(f: &Field, force_optional: bool) -> String {
    let base = match f.ty.as_str() {
        "uuid" => "Uuid",
        "string" | "text" => "String",
        "int" => "i32",
        "bool" => "bool",
        "datetime" => "DateTime<Utc>",
        _ => "String",
    };
    if force_optional || !f.required {
        format!("Option<{base}>")
    } else {
        base.to_string()
    }
}

fn render_migration(s: &SchemaFile) -> String {
    let e = &s.entity;
    let mut cols = String::new();
    for f in &s.field {
        let nullability = if f.required { "NOT NULL " } else { "" };
        writeln!(cols, "    {} {} {},", f.name, sql_type(f), nullability).unwrap();
    }

    format!(
        r#"-- =============================================================================
-- {name}（由 cargo xtask gen 生成）
--
-- 注意：本文件生成后即可自由修改；已发布后只能追加新迁移。
-- =============================================================================

CREATE TABLE IF NOT EXISTS {table} (
    id          uuid        PRIMARY KEY,
    tenant_id   uuid        NOT NULL,
{cols}    version     bigint      NOT NULL DEFAULT 0,
    created_at  timestamptz NOT NULL DEFAULT now(),
    created_by  uuid,
    updated_at  timestamptz,
    updated_by  uuid,
    deleted_at  timestamptz
);

COMMENT ON TABLE {table} IS '{name}';

CREATE INDEX IF NOT EXISTS idx_{table}_list
    ON {table} (tenant_id, created_at DESC, id)
    WHERE deleted_at IS NULL;

-- 行级安全：默认强制隔离，避免“生成的新表忘了加策略”
ALTER TABLE {table} ENABLE ROW LEVEL SECURITY;
ALTER TABLE {table} FORCE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS tenant_isolation ON {table};
CREATE POLICY tenant_isolation ON {table}
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);

COMMENT ON POLICY tenant_isolation ON {table} IS '租户隔离：tenant_id 必须等于当前租户';
"#,
        name = e.name,
        table = e.table,
        cols = cols,
    )
}

fn render_rust(s: &SchemaFile) -> Result<String, String> {
    let e = &s.entity;
    let struct_name = pascal(&e.table);
    let business: Vec<&Field> = s.field.iter().filter(|f| !f.auto).collect();
    let autos: Vec<&Field> = s.field.iter().filter(|f| f.auto).collect();

    let mut vo_fields = String::new();
    // 主键与审计列不属于 schema 字段，但前端/接口都需要
    writeln!(vo_fields, "    /// 主键\n    pub id: Uuid,").unwrap();
    for f in &s.field {
        // list 字段在文档注释里标注，方便生成前端表格时识别
        let note = if f.list { "（列表展示）" } else { "" };
        writeln!(
            vo_fields,
            "    /// {}{}\n    pub {}: {},",
            f.name,
            note,
            f.name,
            rust_type(f, false)
        )
        .unwrap();
    }
    writeln!(
        vo_fields,
        "    /// 创建时间\n    pub created_at: DateTime<Utc>,"
    )
    .unwrap();
    writeln!(
        vo_fields,
        "    /// 更新时间\n    pub updated_at: Option<DateTime<Utc>>,"
    )
    .unwrap();

    let mut create_fields = String::new();
    for f in &business {
        writeln!(
            create_fields,
            "    /// {}\n    pub {}: {},",
            f.name,
            f.name,
            rust_type(f, false)
        )
        .unwrap();
    }

    let mut update_fields = String::new();
    for f in &business {
        writeln!(
            update_fields,
            "    /// {}\n    pub {}: {},",
            f.name,
            f.name,
            rust_type(f, true)
        )
        .unwrap();
    }

    let search_fields: Vec<&Field> = s
        .field
        .iter()
        .filter(|f| f.search.is_some() && matches!(f.ty.as_str(), "string" | "text"))
        .collect();
    let keyword_clause = search_fields
        .iter()
        .map(|f| format!("t.{} ILIKE $1", f.name))
        .collect::<Vec<_>>()
        .join(" OR ");

    let mut select_cols = vec!["t.id".to_string()];
    for f in &s.field {
        select_cols.push(format!("t.{}", f.name));
    }
    select_cols.push("t.created_at".into());
    select_cols.push("t.updated_at".into());
    let select_alias_cols = select_cols.join(", ");
    // RETURNING 里不能用表别名
    let return_cols = select_alias_cols.replace("t.", "");

    // ---- INSERT ----
    let mut insert_cols = vec!["id".to_string(), "tenant_id".to_string()];
    let mut insert_vals = vec!["$1".to_string(), "$2".to_string()];
    let mut insert_binds = String::new();
    let mut idx = 3usize;
    for f in &business {
        insert_cols.push(f.name.clone());
        insert_vals.push(format!("${idx}"));
        writeln!(insert_binds, "        .bind(req.{})", f.name).unwrap();
        idx += 1;
    }
    for f in &autos {
        insert_cols.push(f.name.clone());
        insert_vals.push(format!("${idx}"));
        if Some(&f.name) == e.owner_field.as_ref() {
            insert_binds.push_str("        .bind(user.user_id)\n");
        } else if Some(&f.name) == e.dept_field.as_ref() {
            insert_binds.push_str("        .bind(user.dept_id)\n");
        } else {
            return Err(format!("auto 字段 {} 未声明为 owner/dept", f.name));
        }
        idx += 1;
    }
    insert_cols.push("created_by".into());
    insert_vals.push(format!("${idx}"));
    insert_binds.push_str("        .bind(user.user_id)\n");

    // ---- UPDATE SET ----
    let mut update_sets = String::new();
    let mut update_binds = String::new();
    let mut uidx = 4usize; // $1 id, $2 tenant, $3 updated_by
    for f in &business {
        writeln!(
            update_sets,
            "            {} = COALESCE(${}, t.{}),",
            f.name, uidx, f.name
        )
        .unwrap();
        writeln!(update_binds, "        .bind(req.{})", f.name).unwrap();
        uidx += 1;
    }

    // ---- 数据权限 ----
    let has_scope = e.owner_field.is_some() && e.dept_field.is_some();
    let scope_sql = |a: usize, b: usize, c: usize| -> String {
        if !has_scope {
            return String::new();
        }
        format!(
            r#"
          AND (
                ${a} LIKE '%all%'
             OR (${a} LIKE '%self%' AND t.{owner} = ${b})
             OR (${a} LIKE '%dept%' AND t.{dept} = ${c})
             OR (${a} LIKE '%custom%' AND t.{dept} IN (
                    SELECT rd.dept_id
                    FROM sys.sys_role_dept rd
                    JOIN sys.sys_user_role ur ON ur.role_id = rd.role_id
                    WHERE ur.user_id = ${b}
                      AND rd.tenant_id = current_setting('app.tenant_id')::uuid
                ))
             OR (${a} LIKE '%child%' AND t.{dept} IN (
                    WITH RECURSIVE sub AS (
                        SELECT id FROM sys.sys_dept WHERE id = ${c}
                        UNION ALL
                        SELECT d.id FROM sys.sys_dept d JOIN sub ON d.parent_id = sub.id
                    )
                    SELECT id FROM sub
                ))
          )
"#,
            a = a,
            b = b,
            c = c,
            owner = e.owner_field.as_deref().unwrap_or("owner_id"),
            dept = e.dept_field.as_deref().unwrap_or("dept_id"),
        )
    };
    let scope_binds = if has_scope {
        "        .bind(&user.data_scope)\n        .bind(user.user_id)\n        .bind(user.dept_id)\n"
    } else {
        ""
    };

    let scope_list = scope_sql(5, 6, 7);
    let scope_count = scope_sql(3, 4, 5);
    let scope_get = scope_sql(3, 4, 5);
    let scope_update = scope_sql(uidx, uidx + 1, uidx + 2);
    let scope_delete = scope_sql(4, 5, 6);

    // 列表 count 需要额外的 scope 绑定（$3..$5）
    let count_scope_binds = if has_scope {
        "    .bind(&user.data_scope)\n    .bind(user.user_id)\n    .bind(user.dept_id)\n"
    } else {
        ""
    };

    Ok(format!(
        r##"//! {name}（由 `cargo xtask gen` 生成）
//!
//! 生成后请：
//! 1. 在插件 `lib.rs` 顶部加 `mod {table};`
//! 2. 在 `router()` 里加 `.merge({table}::router())`
//! 3. 把本文件的 5 个 handler 加进插件的 OpenAPI 片段（paths(...)），否则契约里没有它们
//! 4. 在插件 `permissions()` 里声明 `{schema}:{table}:list/add/edit/delete`

#![allow(clippy::too_many_arguments)]
#![allow(unused_imports)]

use chrono::{{DateTime, Utc}};
use kernel::prelude::*;
use kernel::response::{{Page, PageMeta, PageQuery}};
use serde::{{Deserialize, Serialize}};
use sqlx::FromRow;
use utoipa::ToSchema;

/// 本表的权限点（生成后请同步到插件的 `permissions()`，否则角色无法授权）。
pub mod perm {{
    /// 列表/详情。
    pub const LIST: &str = "{schema}:{table}:list";
    /// 新增。
    pub const ADD: &str = "{schema}:{table}:add";
    /// 编辑。
    pub const EDIT: &str = "{schema}:{table}:edit";
    /// 删除。
    pub const DELETE: &str = "{schema}:{table}:delete";
}}

/// 权限校验；失败返回 403。
fn require(user: &CurrentUser, code: &str) -> Result<(), AppError> {{
    if user.has_permission(code) {{
        Ok(())
    }} else {{
        Err(AppError::Forbidden)
    }}
}}

/// {name}视图对象。
#[derive(Debug, Serialize, FromRow, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct {struct_name}Vo {{
{vo_fields}}}

/// 创建请求。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Create{struct_name}Request {{
{create_fields}}}

/// 更新请求（全部可选）。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Update{struct_name}Request {{
{update_fields}}}

/// 列表查询参数。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct {struct_name}Query {{
    /// 分页参数。
    #[serde(flatten)]
    pub page: PageQuery,
    /// 关键字（模糊匹配可搜索字段）。
    pub keyword: Option<String>,
}}

/// 路由。
pub fn router() -> Router<AppState> {{
    Router::new()
        .route("{route}", get(list).post(create))
        .route("{route}/{{id}}", get(detail).patch(update).delete(delete))
}}

/// 列表。
#[utoipa::path(
    get,
    path = "{api_path}",
    tag = "{schema}",
    params(
        ("page" = Option<u64>, Query, description = "页码"),
        ("size" = Option<u64>, Query, description = "每页条数")
    ),
    responses((status = 200, description = "分页列表", body = ApiResponse<Page<{struct_name}Vo>>))
)]
pub async fn list(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Query(q): Query<{struct_name}Query>,
) -> ApiResult<Page<{struct_name}Vo>> {{
    require(&user, perm::LIST)?;
    let keyword = q
        .keyword
        .as_deref()
        .map(|s| format!("%{{}}%", s.trim()))
        .filter(|s| s != "%%");

    let sql = r#"
        SELECT {select_cols}
        FROM {schema}.{table} t
        WHERE t.deleted_at IS NULL
          AND t.tenant_id = $2
          AND ($1::text IS NULL OR {keyword_clause})
          {scope_list}
        ORDER BY t.created_at DESC, t.id
        LIMIT $3 OFFSET $4
    "#;

    let rows = sqlx::query_as::<_, {struct_name}Vo>(sql)
        .bind(keyword.as_deref())
        .bind(user.tenant_id)
        .bind(q.page.limit())
        .bind(q.page.offset())
{scope_binds}        .fetch_all(&mut *tx)
        .await?;

    let total: i64 = sqlx::query_scalar(
        r#"
        SELECT count(*) FROM {schema}.{table} t
        WHERE t.deleted_at IS NULL
          AND t.tenant_id = $2
          AND ($1::text IS NULL OR {keyword_clause})
          {scope_count}
        "#,
    )
    .bind(keyword.as_deref())
    .bind(user.tenant_id)
{count_scope_binds}    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(ApiResponse::new(Page {{
        data: rows,
        page: PageMeta::new(q.page.page(), q.page.size(), total as u64),
    }}))
}}

/// 详情。
#[utoipa::path(
    get,
    path = "{api_path}/{{id}}",
    tag = "{schema}",
    params(("id" = Uuid, Path, description = "主键")),
    responses((status = 200, description = "详情", body = ApiResponse<{struct_name}Vo>))
)]
pub async fn detail(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> ApiResult<{struct_name}Vo> {{
    require(&user, perm::LIST)?;
    let sql = r#"
        SELECT {select_cols}
        FROM {schema}.{table} t
        WHERE t.id = $1 AND t.tenant_id = $2 AND t.deleted_at IS NULL
        {scope_get}
    "#;

    let vo = sqlx::query_as::<_, {struct_name}Vo>(sql)
        .bind(id)
        .bind(user.tenant_id)
{scope_binds}        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AppError::NotFound)?;

    tx.commit().await?;
    Ok(ApiResponse::new(vo))
}}

/// 创建。
#[utoipa::path(
    post,
    path = "{api_path}",
    tag = "{schema}",
    request_body = Create{struct_name}Request,
    responses((status = 200, description = "创建成功", body = ApiResponse<{struct_name}Vo>))
)]
pub async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Json(req): Json<Create{struct_name}Request>,
) -> ApiResult<{struct_name}Vo> {{
    require(&user, perm::ADD)?;
    let sql = r#"
        INSERT INTO {schema}.{table} ({insert_cols})
        VALUES ({insert_vals})
        RETURNING {return_cols}
    "#;

    let vo = sqlx::query_as::<_, {struct_name}Vo>(sql)
        .bind(Uuid::now_v7())
        .bind(user.tenant_id)
{insert_binds}        .fetch_one(&mut *tx)
        .await?;

    tx.commit().await?;
    kernel::audit::record_user(
        &state.db,
        &user,
        "{schema}.{table}.create",
        "{table}",
        Some(&vo.id.to_string()),
        None,
    )
    .await;
    Ok(ApiResponse::new(vo))
}}

/// 更新。
#[utoipa::path(
    patch,
    path = "{api_path}/{{id}}",
    tag = "{schema}",
    params(("id" = Uuid, Path, description = "主键")),
    request_body = Update{struct_name}Request,
    responses((status = 200, description = "更新后的记录", body = ApiResponse<{struct_name}Vo>))
)]
pub async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
    Json(req): Json<Update{struct_name}Request>,
) -> ApiResult<{struct_name}Vo> {{
    require(&user, perm::EDIT)?;
    let sql = r#"
        UPDATE {schema}.{table} t SET
{update_sets}            updated_at = now(),
            updated_by = $3,
            version = t.version + 1
        WHERE t.id = $1 AND t.tenant_id = $2 AND t.deleted_at IS NULL
        {scope_update}
        RETURNING {return_cols}
    "#;

    let vo = sqlx::query_as::<_, {struct_name}Vo>(sql)
        .bind(id)
        .bind(user.tenant_id)
        .bind(user.user_id)
{update_binds}{scope_binds}        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AppError::NotFound)?;

    tx.commit().await?;
    kernel::audit::record_user(
        &state.db,
        &user,
        "{schema}.{table}.update",
        "{table}",
        Some(&vo.id.to_string()),
        None,
    )
    .await;
    Ok(ApiResponse::new(vo))
}}

/// 删除（软删）。
#[utoipa::path(
    delete,
    path = "{api_path}/{{id}}",
    tag = "{schema}",
    params(("id" = Uuid, Path, description = "主键")),
    responses((status = 200, description = "删除成功"))
)]
pub async fn delete(
    State(state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
    Path(id): Path<Uuid>,
) -> ApiResult<()> {{
    require(&user, perm::DELETE)?;
    let sql = r#"
        UPDATE {schema}.{table} t
        SET deleted_at = now(), updated_by = $3, version = t.version + 1
        WHERE t.id = $1 AND t.tenant_id = $2 AND t.deleted_at IS NULL
        {scope_delete}
    "#;

    let affected = sqlx::query(sql)
        .bind(id)
        .bind(user.tenant_id)
        .bind(user.user_id)
{scope_binds_4}        .execute(&mut *tx)
        .await?
        .rows_affected();

    if affected == 0 {{
        return Err(AppError::NotFound);
    }}
    tx.commit().await?;
    kernel::audit::record_user(
        &state.db,
        &user,
        "{schema}.{table}.delete",
        "{table}",
        Some(&id.to_string()),
        None,
    )
    .await;
    Ok(ApiResponse::new(()))
}}
"##,
        name = e.name,
        table = e.table,
        route = e.route,
        schema = e.plugin,
        api_path = format!("/api/{}{}", e.plugin, e.route),
        struct_name = struct_name,
        vo_fields = vo_fields,
        create_fields = create_fields,
        update_fields = update_fields,
        select_cols = select_alias_cols,
        keyword_clause = keyword_clause,
        scope_list = scope_list,
        scope_count = scope_count,
        scope_get = scope_get,
        scope_update = scope_update,
        scope_delete = scope_delete,
        scope_binds = scope_binds,
        scope_binds_4 = scope_binds,
        count_scope_binds = count_scope_binds,
        insert_cols = insert_cols.join(", "),
        insert_vals = insert_vals.join(", "),
        insert_binds = insert_binds,
        update_sets = update_sets,
        update_binds = update_binds,
        return_cols = return_cols,
    ))
}

fn next_migration_seq(plugin_dir: &Path) -> Result<String, String> {
    let dir = plugin_dir.join("migrations");
    let mut max = 0u32;
    if let Ok(entries) = fs::read_dir(&dir) {
        for e in entries.flatten() {
            if let Some(prefix) = e.file_name().to_string_lossy().split('_').next() {
                if let Ok(n) = prefix.parse::<u32>() {
                    max = max.max(n);
                }
            }
        }
    }
    Ok(format!("{:04}", max + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SchemaFile {
        SchemaFile {
            entity: Entity {
                plugin: "crm".into(),
                table: "contact".into(),
                name: "联系人".into(),
                route: "/contacts".into(),
                owner_field: Some("owner_id".into()),
                dept_field: Some("dept_id".into()),
            },
            field: vec![
                Field {
                    name: "name".into(),
                    ty: "string".into(),
                    length: Some(64),
                    required: true,
                    list: true,
                    search: Some("ct".into()),
                    auto: false,
                },
                Field {
                    name: "owner_id".into(),
                    ty: "uuid".into(),
                    length: None,
                    required: true,
                    list: false,
                    search: None,
                    auto: true,
                },
                Field {
                    name: "dept_id".into(),
                    ty: "uuid".into(),
                    length: None,
                    required: false,
                    list: false,
                    search: None,
                    auto: true,
                },
            ],
        }
    }

    /// 迁移里必须有表名与所有业务列。
    #[test]
    fn migration_contains_columns() {
        let sql = render_migration(&sample());
        assert!(sql.contains("CREATE TABLE IF NOT EXISTS contact"));
        assert!(sql.contains("name varchar(64) NOT NULL"));
        assert!(sql.contains("owner_id uuid NOT NULL"));
        // 生成即安全：新表必须自带 RLS，不能依赖人工补策略
        assert!(sql.contains("FORCE ROW LEVEL SECURITY"));
        assert!(sql.contains("CREATE POLICY tenant_isolation ON contact"));
    }

    /// 生成的 Rust 必须包含实体、CRUD 路由与数据权限片段。
    #[test]
    fn rust_contains_crud_and_scope() {
        let code = render_rust(&sample()).expect("render");
        assert!(code.contains("pub struct ContactVo"));
        assert!(code.contains("pub fn router()"));
        assert!(code.contains("#[utoipa::path("));
        assert!(code.contains("mut tx: TenantTx"));
        assert!(code.contains("tx.commit()"));
        assert!(code.contains("path = \"/api/crm/contacts\""));
        assert!(code.contains("%child%"));
        assert!(code.contains("%custom%"));
        assert!(code.contains("/contacts/{id}"));
        // 生成即安全：权限点 + 审计缺一不可
        assert!(code.contains("pub mod perm"));
        assert!(code.contains("require(&user, perm::ADD)"));
        assert!(code.contains("crm.contact.create"));
    }

    /// 非法类型必须被拒绝，避免生成不可编译的代码。
    #[test]
    fn validate_rejects_unknown_type() {
        let mut s = sample();
        s.field[0].ty = "money".into();
        assert!(validate(&s).is_err());
    }
}
