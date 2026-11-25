use axum::extract::State;
use kernel::prelude::*;

use crate::api::scope_filter;
use serde::Serialize;
use sqlx::FromRow;

/// 月度统计。
#[derive(Debug, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MonthStat {
    /// 月份（YYYY-MM）。
    pub month: String,
    /// 合同金额（分）。
    pub contract_cents: i64,
    /// 回款金额（分）。
    pub payment_cents: i64,
}

/// 经营看板汇总。
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DashboardSummary {
    /// 合同金额合计（不含作废）。
    pub contract_amount_cents: i64,
    /// 已回款合计。
    pub paid_cents: i64,
    /// 未回款。
    pub remaining_cents: i64,
    /// 合同数量。
    pub contract_count: i64,
    /// 客户数量。
    pub customer_count: i64,
    /// 商机数量。
    pub opportunity_count: i64,
    /// 近 6 个月趋势。
    pub months: Vec<MonthStat>,
}

/// 经营看板：合同额、回款额、客户/商机数量与近 6 个月趋势。
#[utoipa::path(
    get,
    path = "/api/crm/dashboard/summary",
    tag = "crm",
    responses((status = 200, description = "经营看板汇总", body = ApiResponse<DashboardSummary>))
)]
pub async fn dashboard_summary(
    State(_state): State<AppState>,
    user: CurrentUser,
    mut tx: TenantTx,
) -> ApiResult<DashboardSummary> {
    crate::perm::require(&user, crate::perm::CUSTOMER_LIST)?;
    // 合同额（排除作废）
    let contract_sql = format!(
        r#"
        SELECT COALESCE(sum(c.amount_cents), 0)::bigint, count(*)::bigint
        FROM crm.contract c
        WHERE c.deleted_at IS NULL AND c.tenant_id = $1 AND c.status <> 'void' {}
        "#,
        scope_filter("$2", "$3", "$4")
    );
    let (contract_amount, contract_count): (i64, i64) = sqlx::query_as(&contract_sql)
        .bind(user.tenant_id)
        .bind(&user.data_scope)
        .bind(user.user_id)
        .bind(user.dept_id)
        .fetch_one(&mut *tx)
        .await?;

    // 回款额
    let payment_sql = format!(
        r#"
        SELECT COALESCE(sum(c.paid_cents), 0)::bigint
        FROM crm.contract c
        WHERE c.deleted_at IS NULL AND c.tenant_id = $1 AND c.status <> 'void' {}
        "#,
        scope_filter("$2", "$3", "$4")
    );
    let paid: i64 = sqlx::query_scalar(&payment_sql)
        .bind(user.tenant_id)
        .bind(&user.data_scope)
        .bind(user.user_id)
        .bind(user.dept_id)
        .fetch_one(&mut *tx)
        .await?;

    // 客户 / 商机数量
    let customer_sql = format!(
        "SELECT count(*)::bigint FROM crm.customer c WHERE c.deleted_at IS NULL AND c.tenant_id = $1 {}",
        scope_filter("$2", "$3", "$4")
    );
    let customer_count: i64 = sqlx::query_scalar(&customer_sql)
        .bind(user.tenant_id)
        .bind(&user.data_scope)
        .bind(user.user_id)
        .bind(user.dept_id)
        .fetch_one(&mut *tx)
        .await?;

    let opp_sql = format!(
        "SELECT count(*)::bigint FROM crm.opportunity c WHERE c.deleted_at IS NULL AND c.tenant_id = $1 {}",
        scope_filter("$2", "$3", "$4")
    );
    let opportunity_count: i64 = sqlx::query_scalar(&opp_sql)
        .bind(user.tenant_id)
        .bind(&user.data_scope)
        .bind(user.user_id)
        .bind(user.dept_id)
        .fetch_one(&mut *tx)
        .await?;

    // 近 6 个月合同额
    let contract_month_sql = format!(
        r#"
        SELECT to_char(date_trunc('month', c.created_at), 'YYYY-MM') AS m,
               COALESCE(sum(c.amount_cents), 0)::bigint
        FROM crm.contract c
        WHERE c.deleted_at IS NULL AND c.tenant_id = $1
          AND c.status <> 'void'
          AND c.created_at >= date_trunc('month', now()) - interval '5 months'
          {}
        GROUP BY 1
        "#,
        scope_filter("$2", "$3", "$4")
    );
    let contract_months: Vec<(String, i64)> = sqlx::query_as(&contract_month_sql)
        .bind(user.tenant_id)
        .bind(&user.data_scope)
        .bind(user.user_id)
        .bind(user.dept_id)
        .fetch_all(&mut *tx)
        .await?;

    // data_scope=all 走按日汇总表（约 180 行）；受限用户回退到明细聚合
    let payment_months: Vec<(String, i64)> = if user.data_scope == "all" {
        sqlx::query_as(
            r#"
            SELECT to_char(date_trunc('month', d.day), 'YYYY-MM') AS m,
                   COALESCE(sum(d.amount_cents), 0)::bigint
            FROM crm.payment_daily d
            WHERE d.tenant_id = $1
              AND d.day >= (date_trunc('month', now()) - interval '5 months')::date
            GROUP BY 1
            "#,
        )
        .bind(user.tenant_id)
        .fetch_all(&mut *tx)
        .await?
    } else {
        let payment_month_sql = format!(
            r#"
            SELECT to_char(date_trunc('month', c.created_at), 'YYYY-MM') AS m,
                   COALESCE(sum(c.amount_cents), 0)::bigint
            FROM crm.payment c
            WHERE c.deleted_at IS NULL AND c.tenant_id = $1
              AND c.created_at >= date_trunc('month', now()) - interval '5 months'
              {}
            GROUP BY 1
            "#,
            scope_filter("$2", "$3", "$4")
        );
        sqlx::query_as(&payment_month_sql)
            .bind(user.tenant_id)
            .bind(&user.data_scope)
            .bind(user.user_id)
            .bind(user.dept_id)
            .fetch_all(&mut *tx)
            .await?
    };

    tx.commit().await?;

    // 生成最近 6 个月的标签（从早到晚）
    use chrono::{Datelike, Months, Utc};
    let now = Utc::now();
    let mut months = Vec::with_capacity(6);
    for i in (0..6).rev() {
        let d = now
            .with_day(1)
            .and_then(|x| x.checked_sub_months(Months::new(i)))
            .unwrap_or(now);
        let label = d.format("%Y-%m").to_string();
        let contract_cents = contract_months
            .iter()
            .find(|(m, _)| m == &label)
            .map(|(_, v)| *v)
            .unwrap_or(0);
        let payment_cents = payment_months
            .iter()
            .find(|(m, _)| m == &label)
            .map(|(_, v)| *v)
            .unwrap_or(0);
        months.push(MonthStat {
            month: label,
            contract_cents,
            payment_cents,
        });
    }

    Ok(ApiResponse::new(DashboardSummary {
        contract_amount_cents: contract_amount,
        paid_cents: paid,
        remaining_cents: contract_amount - paid,
        contract_count,
        customer_count,
        opportunity_count,
        months,
    }))
}
