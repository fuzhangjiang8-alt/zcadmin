//! 审批流领域模型、视图对象（VO）、请求体（DTO）与状态机校验规则。
//!
//! 边界：纯数据结构与领域校验，不包含 SQL 或 HTTP 逻辑（便于独立单测）。

use chrono::{DateTime, Utc};
use kernel::error::AppError;
use kernel::response::PageQuery;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// 允许的业务分类白名单。
const ALLOWED_BIZ_TYPES: [&str; 4] = ["contract", "discount", "refund", "general"];

/// 审批单视图对象。
#[derive(Debug, Clone, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRequestVo {
    /// 审批单 ID（UUIDv7）。
    pub id: Uuid,
    /// 审批单编码（租户内唯一）。
    pub code: String,
    /// 审批标题。
    pub title: String,
    /// 业务类型：contract / discount / refund / general。
    pub biz_type: String,
    /// 关联业务单据标识（可空）。
    pub biz_id: Option<String>,
    /// 当前状态：pending / approved / rejected / cancelled。
    pub status: String,
    /// 涉及金额（分）。
    pub amount_cents: i64,
    /// 申请说明。
    pub remark: Option<String>,
    /// 审批意见。
    pub decision_note: Option<String>,
    /// 审批人 ID。
    pub decided_by: Option<Uuid>,
    /// 审批时间。
    pub decided_at: Option<DateTime<Utc>>,
    /// 发起人 ID。
    pub created_by: Uuid,
    /// 创建时间。
    pub created_at: DateTime<Utc>,
    /// 乐观锁版本号。
    pub version: i32,
}

/// 审批单按状态汇总统计视图（供审批中心看板卡片展示）。
#[derive(Debug, Clone, Default, Serialize, FromRow, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowStatsVo {
    /// 总单数。
    pub total: i64,
    /// 待审批数。
    pub pending: i64,
    /// 已通过数。
    pub approved: i64,
    /// 已驳回数。
    pub rejected: i64,
    /// 已撤回数。
    pub cancelled: i64,
}

/// 审批单列表查询参数。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowQuery {
    /// 分页参数。
    #[serde(flatten)]
    pub page: PageQuery,
    /// 按状态过滤（pending / approved / rejected / cancelled）。
    pub status: Option<String>,
    /// 按业务类型过滤（contract / discount / refund / general）。
    pub biz_type: Option<String>,
    /// 关键字（匹配编码或标题）。
    pub keyword: Option<String>,
}

/// 发起审批单请求体。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateWorkflowRequest {
    /// 审批单编码（1~64 字符）。
    pub code: String,
    /// 审批标题（1~128 字符）。
    pub title: String,
    /// 业务类型：contract / discount / refund / general。
    pub biz_type: String,
    /// 关联业务单据 ID（可选，最长 64 字符）。
    pub biz_id: Option<String>,
    /// 涉及金额（分，>= 0）。
    #[serde(default)]
    pub amount_cents: i64,
    /// 申请备注（可选，最长 1000 字符）。
    pub remark: Option<String>,
}

/// 审批决议请求体。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DecideWorkflowRequest {
    /// 审批动作：`approve`（通过）/ `reject`（驳回）/ `cancel`（撤回）。
    pub action: String,
    /// 审批意见（可选，最长 500 字符）。
    pub note: Option<String>,
}

impl CreateWorkflowRequest {
    /// 校验发起审批参数。
    pub fn validate(&self) -> Result<(), AppError> {
        let code = self.code.trim();
        if code.is_empty() || code.chars().count() > 64 {
            return Err(AppError::Validation("审批单编号长度需为 1~64 字符".into()));
        }
        let title = self.title.trim();
        if title.is_empty() || title.chars().count() > 128 {
            return Err(AppError::Validation("审批标题长度需为 1~128 字符".into()));
        }
        if !ALLOWED_BIZ_TYPES.contains(&self.biz_type.as_str()) {
            return Err(AppError::Validation(
                "业务类型仅支持 contract / discount / refund / general".into(),
            ));
        }
        if self.amount_cents < 0 {
            return Err(AppError::Validation("审批金额不能为负数".into()));
        }
        if let Some(biz_id) = &self.biz_id {
            if biz_id.chars().count() > 64 {
                return Err(AppError::Validation("关联业务单号最长 64 字符".into()));
            }
        }
        if let Some(remark) = &self.remark {
            if remark.chars().count() > 1000 {
                return Err(AppError::Validation("申请说明最长 1000 字符".into()));
            }
        }
        Ok(())
    }
}

/// 将审批动作映射为目标状态（`approve -> approved`，`reject -> rejected`，`cancel -> cancelled`）。
pub fn resolve_target_status(action: &str) -> Result<&'static str, AppError> {
    match action.trim() {
        "approve" => Ok("approved"),
        "reject" => Ok("rejected"),
        "cancel" => Ok("cancelled"),
        _ => Err(AppError::Validation(
            "审批动作仅支持 approve / reject / cancel".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_decision_actions_to_terminal_states() {
        assert_eq!(
            resolve_target_status("approve").expect("approve"),
            "approved"
        );
        assert_eq!(resolve_target_status("reject").expect("reject"), "rejected");
        assert_eq!(
            resolve_target_status("cancel").expect("cancel"),
            "cancelled"
        );
        assert!(resolve_target_status("unknown").is_err());
    }

    #[test]
    fn validates_create_request_fields() {
        let valid = CreateWorkflowRequest {
            code: "WF-2026-001".into(),
            title: "华东区年度框架合同特批".into(),
            biz_type: "contract".into(),
            biz_id: Some("CT-001".into()),
            amount_cents: 500_000,
            remark: Some("特批九折".into()),
        };
        assert!(valid.validate().is_ok());

        let bad_type = CreateWorkflowRequest {
            biz_type: "invalid".into(),
            ..valid
        };
        assert!(bad_type.validate().is_err());
    }
}
