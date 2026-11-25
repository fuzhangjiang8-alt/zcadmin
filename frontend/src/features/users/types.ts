// =============================================================================
// 用户域共享类型
// =============================================================================

/** 用户视图。 */
export interface UserVo {
  id: string;
  username: string;
  displayName: string;
  email: string | null;
  phone: string | null;
  deptId?: string | null;
  deptName: string | null;
  status: 'active' | 'locked' | 'disabled';
  isSuper: boolean;
  createdAt: string;
}

/** 部门树节点。 */
export interface DeptNode {
  id: string;
  name: string;
  children: DeptNode[];
}

/** 角色简要信息。 */
export interface RoleVo {
  id: string;
  code: string;
  name: string;
  isSystem: boolean;
}

/** 岗位简要信息。 */
export interface PostVo {
  id: string;
  name: string;
  code: string;
}

/** 导入错误行。 */
export interface ImportErrorRow {
  row: number;
  message: string;
  username?: string | null;
  displayName?: string | null;
  email?: string | null;
  phone?: string | null;
  dept?: string | null;
}

/** 导入结果。 */
export interface ImportResult {
  total: number;
  success: number;
  failed: number;
  errors: ImportErrorRow[];
}

/** 把部门树拍平成下拉选项（全角空格缩进表示层级）。 */
export function flattenDepts(
  nodes: DeptNode[],
  depth = 0,
  out: { label: string; value: string }[] = [],
): { label: string; value: string }[] {
  for (const n of nodes) {
    out.push({ label: `${'　'.repeat(depth)}${n.name}`, value: n.id });
    flattenDepts(n.children ?? [], depth + 1, out);
  }
  return out;
}
