//! 数据权限 SQL 片段（唯一实现）。
//!
//! 所有业务查询都应通过本模块生成条件，避免「CRM 一份、系统一份、生成器一份」
//! 导致规则漂移（新增规则时漏改某处 = 越权或不可见）。
//!
//! 用法：
//! ```ignore
//! let cond = kernel::scope::condition("c", "$3", "$4", "$5");
//! let sql = format!("SELECT ... FROM t c WHERE ... {cond}");
//! ```

/// 生成数据权限条件片段（默认表含 `{alias}.owner_id` 与 `{alias}.dept_id` 列）。
///
/// - `alias`：业务表别名（如 `c`，表需有 `owner_id` / `dept_id`）；
/// - `scope` / `user` / `dept`：SQL 占位符（如 `$3` / `$4` / `$5`）。
pub fn condition(alias: &str, scope: &str, user: &str, dept: &str) -> String {
    condition_with_owner(
        &format!("{alias}.owner_id"),
        &format!("{alias}.dept_id"),
        scope,
        user,
        dept,
    )
}

/// 生成自定义列名的数据权限条件片段（例如 `sys_user` 表使用 `u.id` 与 `u.dept_id`）。
pub fn condition_with_owner(
    owner_col: &str,
    dept_col: &str,
    scope: &str,
    user: &str,
    dept: &str,
) -> String {
    let template = r#"
    AND (
          {scope} LIKE '%all%'
       OR ({scope} LIKE '%self%' AND {owner_col} = {user})
       OR ({scope} LIKE '%dept%' AND {dept_col} = {dept})
       OR ({scope} LIKE '%custom%' AND {dept_col} IN (
              SELECT rd.dept_id
              FROM sys.sys_role_dept rd
              JOIN sys.sys_user_role ur ON ur.role_id = rd.role_id
              WHERE ur.user_id = {user}
                AND rd.tenant_id = current_setting('app.tenant_id')::uuid
          ))
       OR ({scope} LIKE '%child%' AND {dept_col} IN (
              WITH RECURSIVE sub AS (
                  SELECT id FROM sys.sys_dept WHERE id = {dept}
                  UNION ALL
                  SELECT d.id FROM sys.sys_dept d JOIN sub ON d.parent_id = sub.id
              )
              SELECT id FROM sub
          ))
    )
"#;
    template
        .replace("{scope}", scope)
        .replace("{user}", user)
        .replace("{dept}", dept)
        .replace("{owner_col}", owner_col)
        .replace("{dept_col}", dept_col)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_standard_and_custom_owner_scope_sql() {
        let std_sql = condition("c", "$2", "$3", "$4");
        assert!(std_sql.contains("c.owner_id = $3"));
        assert!(std_sql.contains("c.dept_id = $4"));

        let user_sql = condition_with_owner("u.id", "u.dept_id", "$2", "$3", "$4");
        assert!(user_sql.contains("u.id = $3"));
        assert!(user_sql.contains("u.dept_id = $4"));
    }
}
