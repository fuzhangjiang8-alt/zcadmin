// =============================================================================
// 角色域共享类型
//
// 抽出到独立文件，避免页面与子组件各写一份、字段漂移。
// =============================================================================

/** 角色视图。 */
export interface RoleVo {
  id: string;
  code: string;
  name: string;
  description: string | null;
  dataScope: string;
  isSystem: boolean;
  createdAt: string;
}

/** 权限点视图。 */
export interface PermissionVo {
  id: string;
  plugin: string;
  code: string;
  name: string;
  deprecated: boolean;
}

/** 部门树节点。 */
export interface DeptNode {
  id: string;
  name: string;
  children: DeptNode[];
}

/** 数据范围预览结果。 */
export interface ScopePreview {
  dataScope: string;
  description: string;
  visibleDeptIds: string[];
  userCount: number;
}

/** 数据范围文案。 */
export function scopeText(scope: string): string {
  return (
    {
      all: '全部数据',
      dept_and_child: '本部门及下级',
      dept: '本部门',
      custom: '自定义部门',
      self: '仅本人',
    }[scope] ?? scope
  );
}
