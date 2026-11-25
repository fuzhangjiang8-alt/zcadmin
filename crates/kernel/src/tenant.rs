//! 请求级租户事务（`TenantTx`）。
//!
//! # 解决什么问题
//!
//! 多租户隔离目前靠每条 SQL 手写 `tenant_id = $1`。一旦某个查询漏写，
//! 就会跨租户泄漏数据。数据库层的 RLS（行级安全）可以兜底，但前提是
//! 每个请求都能告诉数据库「我是哪个租户」。
//!
//! `TenantTx` 的做法：
//!
//! ```text
//! BEGIN;
//! SELECT set_config('app.tenant_id', '<uuid>', true);   -- true = 随事务失效
//! ... 业务 SQL（RLS 策略自动生效）...
//! COMMIT;
//! ```
//!
//! 使用 `true`（local）而不是 `false` 是关键：
//! 连接回池后设置自动失效，不会串租户。
//!
//! # 用法
//!
//! ```ignore
//! async fn handler(state: State<AppState>, mut tx: TenantTx) -> ApiResult<...> {
//!     let rows = sqlx::query_as(...).fetch_all(&mut *tx).await?;
//!     tx.commit().await?;
//!     Ok(ApiResponse::new(rows))
//! }
//! ```
//!
//! # 使用范围
//!
//! 除登录/健康检查等没有租户上下文的入口外，业务 handler 一律使用 `TenantTx`。
//! 平台超管（`is_super`）会额外得到 `app.platform = 'on'`，
//! 用于跨租户读取账单/用量等平台级数据；普通租户永远拿不到该标记。
//! 表的 `FORCE ROW LEVEL SECURITY` 已由迁移启用（见 `docs/03-数据架构.md` 第 9 节）。

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::state::AppState;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use sqlx::{PgConnection, Postgres, Transaction};
use std::ops::{Deref, DerefMut};
use uuid::Uuid;

/// 带租户上下文的请求级事务。
pub struct TenantTx {
    /// `Option` 是为了在 `commit` 时 move 出事务（`Transaction` 不实现 Clone）。
    tx: Option<Transaction<'static, Postgres>>,
}

impl TenantTx {
    /// 开启事务并注入租户上下文（登录等还不知道用户的场景）。
    pub async fn begin_tenant(pool: &sqlx::PgPool, tenant_id: Uuid) -> Result<Self, AppError> {
        let mut tx = pool.begin().await?;
        // set_config(..., true) = 事务级设置，COMMIT/ROLLBACK 后自动清除。
        sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
            .bind(tenant_id.to_string())
            .execute(&mut *tx)
            .await?;
        Ok(Self { tx: Some(tx) })
    }

    /// 开启事务并注入租户 + 用户上下文。
    pub async fn begin(
        pool: &sqlx::PgPool,
        tenant_id: Uuid,
        user_id: Uuid,
    ) -> Result<Self, AppError> {
        let mut this = Self::begin_tenant(pool, tenant_id).await?;
        sqlx::query("SELECT set_config('app.user_id', $1, true)")
            .bind(user_id.to_string())
            .execute(&mut *this)
            .await?;
        Ok(this)
    }

    /// 提交事务。未调用则 Drop 时自动回滚。
    pub async fn commit(mut self) -> Result<(), AppError> {
        if let Some(tx) = self.tx.take() {
            tx.commit().await?;
        }
        Ok(())
    }

    /// 显式回滚事务（避免触发未显式提交的诊断日志）。
    pub async fn rollback(mut self) -> Result<(), AppError> {
        if let Some(tx) = self.tx.take() {
            tx.rollback().await?;
        }
        Ok(())
    }
}

impl Drop for TenantTx {
    fn drop(&mut self) {
        if self.tx.is_some() && !std::thread::panicking() {
            tracing::debug!("TenantTx 在未显式调用 commit/rollback 的情况下被丢弃，将自动回滚");
        }
    }
}

impl Deref for TenantTx {
    type Target = PgConnection;

    fn deref(&self) -> &Self::Target {
        // 构造函数保证 Some；commit 会消费 self，不会走到这里
        self.tx.as_ref().expect("TenantTx 已提交").deref()
    }
}

impl DerefMut for TenantTx {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.tx.as_mut().expect("TenantTx 已提交").deref_mut()
    }
}

impl FromRequestParts<AppState> for TenantTx {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // 先从会话里拿到用户（未登录直接 401）
        let user =
            <CurrentUser as FromRequestParts<AppState>>::from_request_parts(parts, state).await?;
        let mut this = Self::begin(&state.db, user.tenant_id, user.user_id).await?;
        // 平台超管需要跨租户访问平台级数据（租户账单/用量/汇总）。
        // 标记为事务级 GUC，策略里用它放行；普通租户永远拿不到这个标记。
        if user.is_super_admin() {
            sqlx::query("SELECT set_config('app.platform', 'on', true)")
                .execute(&mut *this)
                .await?;
        }
        Ok(this)
    }
}
