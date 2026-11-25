//! CRM HTTP 接口。
//!
//! 所有查询都带**数据权限**过滤（`all / dept_and_child / dept / self`），
//! 与用户管理共用同一套语义，避免每个业务模块各写一套。

use kernel::prelude::*;

pub use crate::api_customer_write::{
    __path_create, __path_delete, __path_update, create, delete, update,
};
pub use crate::api_customers::{__path_get, __path_list, get, list};
pub use crate::api_dashboard::{
    __path_dashboard_summary, dashboard_summary, DashboardSummary, MonthStat,
};
pub use crate::api_metrics::{
    __path_contract_payments, __path_opportunity_stats, contract_payments, opportunity_stats,
    ContractPaymentsVo, PaymentQuery, StageStat,
};
/// 生成数据权限条件片段（统一实现在 `kernel::scope`）。
pub use crate::api_relations::{
    __path_customer_contacts, __path_customer_contracts, __path_customer_opportunities,
    __path_opportunity_contracts, __path_top_customers, customer_contacts, customer_contracts,
    customer_opportunities, opportunity_contracts, top_customers, TopCustomer, TopQuery,
};
pub use crate::api_stage::{
    __path_opportunity_stage_conversion, __path_opportunity_stage_duration,
    __path_opportunity_stage_history, __path_stuck_opportunities, opportunity_stage_conversion,
    opportunity_stage_duration, opportunity_stage_history, stuck_opportunities, StageConversion,
    StageDuration, StageHistoryVo, StuckOpportunity, StuckQuery,
};

pub(crate) fn scope_filter(scope: &str, user: &str, dept: &str) -> String {
    kernel::scope::condition("c", scope, user, dept)
}

/// 校验客户存在且在当前用户的数据范围内。
pub(crate) async fn ensure_customer_visible(
    conn: &mut sqlx::PgConnection,
    user: &CurrentUser,
    customer_id: Uuid,
) -> Result<(), AppError> {
    let sql = format!(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM crm.customer c
            WHERE c.id = $1 AND c.deleted_at IS NULL {}
        )
        "#,
        scope_filter("$2", "$3", "$4")
    );
    let exists: bool = sqlx::query_scalar(&sql)
        .bind(customer_id)
        .bind(&user.data_scope)
        .bind(user.user_id)
        .bind(user.dept_id)
        .fetch_one(&mut *conn)
        .await?;
    if exists {
        Ok(())
    } else {
        Err(AppError::NotFound)
    }
}

/// 校验合同存在且在当前用户的数据范围内。
///
/// 回款必须挂在「看得见」的合同上，否则会绕过行级权限写入别部门的数据。
pub(crate) async fn ensure_contract_visible(
    conn: &mut sqlx::PgConnection,
    user: &CurrentUser,
    contract_id: Uuid,
) -> Result<(), AppError> {
    let sql = format!(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM crm.contract c
            WHERE c.id = $1 AND c.deleted_at IS NULL {}
        )
        "#,
        scope_filter("$2", "$3", "$4")
    );
    let exists: bool = sqlx::query_scalar(&sql)
        .bind(contract_id)
        .bind(&user.data_scope)
        .bind(user.user_id)
        .bind(user.dept_id)
        .fetch_one(&mut *conn)
        .await?;
    if exists {
        Ok(())
    } else {
        Err(AppError::NotFound)
    }
}
