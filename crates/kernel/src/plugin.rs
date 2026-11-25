//! 插件契约与注册表（支持编译期 Cargo features + 运行期 `DISABLED_PLUGINS` 双层可插拔）。
//!
//! 职责：定义 [`Plugin`] 契约、编译期注册表收集、依赖拓扑排序及运行期启停过滤。
//! 边界：内核不感知具体业务插件类型；跨插件协作仅通过事件或契约完成。

use crate::state::AppState;
use anyhow::{bail, Context};
use axum::Router;
use serde::Serialize;
use sqlx::migrate::Migrator;
use std::collections::{HashMap, HashSet};

/// 对外返回的已启用插件元信息视图。
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfoVo {
    /// 插件唯一标识（对应路由前缀 `/api/{id}` 与数据库 schema `{id}`）。
    pub id: String,
    /// 插件展示名称。
    pub name: String,
    /// 插件版本号。
    pub version: String,
    /// 装配排序号。
    pub order: i32,
}

/// 插件元信息：同时决定路由前缀（`/api/{id}`）、数据库 schema（`{id}`）与权限点前缀。
#[derive(Debug, Clone, Copy)]
pub struct PluginMeta {
    /// 插件唯一标识（小写字母/数字/下划线，发布后不可改）。
    pub id: &'static str,
    /// 展示名称（中文）。
    pub name: &'static str,
    /// 语义化版本。
    pub version: &'static str,
    /// 依赖的其他插件 id。
    pub depends_on: &'static [&'static str],
    /// 同层装配顺序（小在前）。
    pub order: i32,
}

/// 权限点声明。
///
/// 插件在启动时声明自己提供的权限点，内核负责 upsert 到 `sys_permission`。
/// 这样权限点只有一个来源（代码），不会出现「代码里有、库里没有」的漂移。
#[derive(Debug, Clone, Copy)]
pub struct PermissionDef {
    /// 完整权限点编码，格式 `{plugin}:{resource}:{action}`。
    pub code: &'static str,
    /// 展示名称。
    pub name: &'static str,
}

/// 菜单声明。
///
/// 插件声明自己需要出现在侧边栏的菜单，内核负责按租户幂等落库。
/// 这样新增页面不需要改前端路由表，也不需要内核认识具体插件。
#[derive(Debug, Clone, Copy)]
pub struct MenuDef {
    /// 菜单标题。
    pub title: &'static str,
    /// 前端路由路径（全局唯一）。
    pub path: &'static str,
    /// 图标名（前端图标映射表的 key）。
    pub icon: &'static str,
    /// 需要的权限点；None 表示登录即可见。
    pub perm: Option<&'static str>,
    /// 父菜单路径；None 表示根菜单。
    pub parent: Option<&'static str>,
    /// 排序号，小的在前。
    pub sort: i32,
}

/// 插件契约：实现者声明元信息、迁移、路由、权限点、菜单与 OpenAPI 片段，其余横切能力由内核统一处理。
pub trait Plugin: Send + Sync + 'static {
    /// 返回插件元信息。
    fn meta(&self) -> PluginMeta;

    /// 该插件的数据库迁移。
    fn migrations(&self) -> Option<&'static Migrator> {
        None
    }

    /// 该插件的 HTTP 路由，会被挂载到 `/api/{id}` 下。
    fn routes(&self) -> Router<AppState> {
        Router::new()
    }

    /// 该插件提供的权限点（启动时幂等 upsert 到 `sys_permission`）。
    fn permissions(&self) -> Vec<PermissionDef> {
        Vec::new()
    }

    /// 该插件声明的菜单（启动时按租户幂等落库）。
    fn menus(&self) -> Vec<MenuDef> {
        Vec::new()
    }

    /// 该插件的 OpenAPI 片段（由 `server --dump-openapi` 合并进总契约）。
    fn openapi(&self) -> Option<utoipa::openapi::OpenApi> {
        None
    }
}

/// 插件注册项。
///
/// 各插件通过 `inventory::submit!` 在链接期注册一个构造函数，
/// 内核无需知道任何具体插件类型。
pub struct PluginRegistration {
    /// 插件构造函数（必须是普通函数指针，保证可在静态区初始化）。
    pub create: fn() -> Box<dyn Plugin>,
}

inventory::collect!(PluginRegistration);

/// 收集并排序所有已注册插件（不禁用任何插件）。
pub fn load_plugins() -> anyhow::Result<Vec<Box<dyn Plugin>>> {
    load_enabled_plugins(&[])
}

/// 收集、按配置过滤（可插拔）并拓扑排序已启用的插件。
///
/// `disabled` 中列出的插件 id 会被跳过（不建 schema、不执行迁移、不同步权限/菜单、不挂路由）。
/// 若某个已启用插件依赖了被禁用的插件，会在拓扑排序阶段报错阻止半残启动。
pub fn load_enabled_plugins(disabled: &[String]) -> anyhow::Result<Vec<Box<dyn Plugin>>> {
    let disabled_set: HashSet<String> = disabled
        .iter()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();

    let plugins: Vec<Box<dyn Plugin>> = inventory::iter::<PluginRegistration>
        .into_iter()
        .map(|reg| (reg.create)())
        .filter(|p| {
            let id = p.meta().id;
            if disabled_set.contains(id) {
                tracing::info!(plugin = id, "插件已按配置禁用，跳过加载");
                false
            } else {
                true
            }
        })
        .collect();

    sort_plugins(plugins)
}

/// 返回当前实例已启用的插件元信息列表（供前端工作台与运维自检使用）。
pub fn enabled_plugin_infos(disabled: &[String]) -> Vec<PluginInfoVo> {
    load_enabled_plugins(disabled)
        .unwrap_or_default()
        .into_iter()
        .map(|p| {
            let m = p.meta();
            PluginInfoVo {
                id: m.id.to_string(),
                name: m.name.to_string(),
                version: m.version.to_string(),
                order: m.order,
            }
        })
        .collect()
}

/// 对给定插件列表执行稳定排序与依赖拓扑排序。
fn sort_plugins(mut plugins: Vec<Box<dyn Plugin>>) -> anyhow::Result<Vec<Box<dyn Plugin>>> {
    // 先按 order 排，保证拓扑排序在"无依赖关系"时结果稳定。
    plugins.sort_by_key(|p| (p.meta().order, p.meta().id));

    // id -> 下标，用于依赖查找。
    let mut index: HashMap<&'static str, usize> = HashMap::with_capacity(plugins.len());
    for (i, p) in plugins.iter().enumerate() {
        let id = p.meta().id;
        if index.insert(id, i).is_some() {
            bail!("插件 id 重复: {id}");
        }
    }

    // 0=未访问, 1=访问中（用于检测环）, 2=已完成
    let mut state = vec![0u8; plugins.len()];
    let mut order: Vec<usize> = Vec::with_capacity(plugins.len());

    for i in 0..plugins.len() {
        visit(i, &plugins, &index, &mut state, &mut order)?;
    }

    // 按下标重排：用 Option 占位以便 move 出 Box。
    let mut slots: Vec<Option<Box<dyn Plugin>>> = plugins.into_iter().map(Some).collect();
    let sorted = order
        .into_iter()
        .filter_map(|i| slots[i].take())
        .collect::<Vec<_>>();

    Ok(sorted)
}

/// 深度优先遍历一个插件节点。
fn visit(
    i: usize,
    plugins: &[Box<dyn Plugin>],
    index: &HashMap<&'static str, usize>,
    state: &mut [u8],
    order: &mut Vec<usize>,
) -> anyhow::Result<()> {
    match state[i] {
        1 => bail!("检测到循环依赖，涉及插件: {}", plugins[i].meta().id),
        2 => return Ok(()),
        _ => {}
    }

    state[i] = 1;
    let meta = plugins[i].meta();
    for dep in meta.depends_on {
        let j = *index
            .get(dep)
            .with_context(|| format!("插件 `{}` 依赖的 `{}` 不存在或未被链接", meta.id, dep))?;
        visit(j, plugins, index, state, order)?;
    }
    state[i] = 2;
    order.push(i);
    Ok(())
}

/// 校验插件集合：id 合法、无重复、依赖完备。
///
/// 独立成函数是为了能在启动早期给出**可读的错误信息**，
/// 而不是等到挂载路由时才 panic。
pub fn validate(plugins: &[Box<dyn Plugin>]) -> anyhow::Result<()> {
    let mut seen = HashSet::new();
    for p in plugins {
        let meta = p.meta();
        let ok = !meta.id.is_empty()
            && meta.id.len() <= 63
            && meta
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        anyhow::ensure!(ok, "非法插件 id: {}", meta.id);
        anyhow::ensure!(seen.insert(meta.id), "插件 id 重复: {}", meta.id);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubPlugin {
        meta: PluginMeta,
    }

    impl Plugin for StubPlugin {
        fn meta(&self) -> PluginMeta {
            self.meta
        }
    }

    #[test]
    fn sorts_dependencies_and_rejects_missing_disabled_dep() {
        let a = Box::new(StubPlugin {
            meta: PluginMeta {
                id: "workflow",
                name: "审批流",
                version: "0.1.0",
                depends_on: &["crm"],
                order: 50,
            },
        }) as Box<dyn Plugin>;
        let b = Box::new(StubPlugin {
            meta: PluginMeta {
                id: "crm",
                name: "客户管理",
                version: "0.1.0",
                depends_on: &[],
                order: 200,
            },
        }) as Box<dyn Plugin>;

        let sorted = sort_plugins(vec![a, b]).expect("topo sort");
        assert_eq!(sorted[0].meta().id, "crm");
        assert_eq!(sorted[1].meta().id, "workflow");

        // 若被依赖的 crm 插件被禁用拔除，仅剩 workflow 时必须报错
        let orphan = Box::new(StubPlugin {
            meta: PluginMeta {
                id: "workflow",
                name: "审批流",
                version: "0.1.0",
                depends_on: &["crm"],
                order: 50,
            },
        }) as Box<dyn Plugin>;
        assert!(sort_plugins(vec![orphan]).is_err());
    }
}
