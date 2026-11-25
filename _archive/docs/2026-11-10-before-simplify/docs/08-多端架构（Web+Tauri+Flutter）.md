# 08 · 多端架构（Web + Tauri 桌面 + Flutter 移动）

> 决策：**Web = Vue 3.5 + Ant Design Vue 4｜桌面 = Tauri 2（复用 Web 前端）｜移动 = Flutter**
> 原则：**共享"契约与常量"，不共享"UI 与状态"**

---

## 0. 三端定位

| 端 | 技术 | 承载 | 代码复用 |
|---|---|---|---|
| **Web** | Vue 3.5 + Ant Design Vue 4 + Vite 6 | 管理后台、CRM、IoT 控制台、AI 工作台、IM Web | 主战场 100% |
| **桌面** | Tauri 2（Rust 壳）+ 同一套 Vue 前端 | IM 桌面客户端、重度操作台、需要本地能力的场景 | Web 代码 85–90% |
| **移动** | Flutter 3.x (Dart) | IM 移动端、审批/待办/扫码、外勤 | 仅契约、设计 token、i18n |

**为什么不统一到一个技术**：

- 管理后台的复杂度在**表格与表单**，Web 生态（AntD/ProTable）无可替代；Flutter Web 的表格、输入法、滚动体验都不达标。
- 移动端的复杂度在**后台保活、推送、原生能力**，Web 技术栈（含 Capacitor）在国产 ROM 上体验差。
- 桌面端的复杂度在**体积、内存、系统集成**，Tauri 用系统 WebView + Rust 壳，是当前最优解。

> "一次编写，三端运行"是幻觉。**共享契约，各自实现 UI**，才是能维护的方案。

---

## 1. 共享边界（先定死，否则一定乱）

| 资产 | Web | 桌面 | 移动 | 共享方式 |
|---|---|---|---|---|
| 业务逻辑（composables / store） | ✅ | ✅ 同一份 | ❌ 重写 | — |
| UI 组件 | ✅ | ✅ 同一份 | ❌ 重写 | — |
| **API 契约** | ✅ | ✅ | ✅ | `contracts/openapi.json` → TS / Dart 生成 |
| **错误码** | ✅ | ✅ | ✅ | `contracts/errors.json` |
| **设计 Token** | ✅ | ✅ | ✅ | `contracts/tokens.json` → CSS 变量 / ThemeData |
| **i18n 词条** | ✅ | ✅ | ✅ | `contracts/i18n/*.json` |
| **WS 协议** | ✅ | ✅ | ✅ | `contracts/ws/*.proto` |
| 权限点字符串 | ✅ | ✅ | ✅ | 后端下发 |

**铁律**：三端**不得各自手写 API 客户端**。手写必漂移，漂移必出"Web 能用、App 报错"的诡异 bug。

---

## 2. 契约仓库结构

```
contracts/
├── openapi.json               # 后端导出（CI 自动更新）
├── errors.json                # 错误码 → 中文/英文文案 → 建议动作
├── tokens.json                # 颜色/字号/圆角/间距/层级
├── i18n/
│   ├── zh-CN.json
│   └── en-US.json
└── ws/
    ├── message.proto          # IM 消息帧
    └── presence.proto
```

```bash
# 生成三端产物（CI 统一执行）
pnpm gen:api      # → packages/types/src/api.d.ts
dart run build_runner build   # → apps/im-mobile/lib/api/*.g.dart
pnpm gen:tokens   # → packages/ui/src/tokens.css + apps/im-mobile/lib/theme/tokens.dart
```

**门禁**：生成物 diff 不为空 → 说明有人手改了契约产物，CI 失败。

---

## 3. Tauri 2 桌面端

### 3.1 为什么是 Tauri 而不是 Electron

| 指标 | Tauri 2 | Electron |
|---|---|---|
| 安装包 | 5–15 MB | 100–150 MB |
| 空闲内存 | 100–150 MB | 250–400 MB |
| 渲染内核 | 系统 WebView | 自带 Chromium |
| 安全模型 | capabilities 细粒度授权，默认无 Node | 默认 Node 全权限 |
| 与 Rust 后端协同 | 同语言，可共享 crate | 无 |
| 代价 | **WebView 版本不一致**（见 3.7） | 包体大但一致 |

### 3.2 进程与目录

```
apps/im-desktop/
├── src-tauri/
│   ├── Cargo.toml
│   ├── tauri.conf.json          # 窗口、CSP、bundle、updater
│   ├── capabilities/
│   │   └── main.json            # 权限声明（最小集）
│   └── src/
│       ├── main.rs
│       ├── commands/            # #[tauri::command] 暴露给前端
│       ├── window.rs            # 多窗口管理
│       ├── tray.rs              # 托盘与角标
│       ├── notify.rs            # 系统通知
│       └── updater.rs           # 更新检查
└── src/                         # ← 复用 plugins-web/im-web + packages/*
    └── platform/tauri.ts        # 平台适配层（唯一有分支的地方）
```

### 3.3 复用策略：条件构建

```ts
// packages/sdk/src/platform/index.ts
export interface PlatformCapabilities {
  notifications: boolean;
  tray: boolean;
  fileSystem: boolean;
  autoUpdate: boolean;
  windowControl: boolean;
}

export const platform: PlatformCapabilities =
  import.meta.env.VITE_TARGET === 'desktop'
    ? await import('./tauri').then(m => m.caps)
    : webCaps;
```

```bash
# Web 构建
VITE_TARGET=web     pnpm build
# 桌面构建（同一个前端仓库）
VITE_TARGET=desktop pnpm build && cargo tauri build
```

**平台差异只允许出现在 `platform/` 目录**（预计 3–5 个文件）。业务代码里出现 `if (isTauri)` 就是设计失败。

### 3.4 必备系统能力

| 能力 | Tauri 方案 | 业务用途 |
|---|---|---|
| 多窗口 | 内置 | 主窗口 + 独立会话窗口 + 通知小窗 |
| 托盘 + 角标 | 内置 tray | 未读数、快捷入口、最小化到托盘 |
| 全局快捷键 | `global-shortcut` | 一键唤起、截图、呼出搜索 |
| 系统通知 | `notification` | 新消息，点击跳转会话 |
| 深链接 | `deep-link` | `zcadmin://chat/123`、邮件/浏览器唤起 |
| 开机自启 | `autostart` | 可选开关 |
| 单实例 | `single-instance` | 二次启动聚焦已有窗口 |
| 文件下载/拖拽 | `dialog` / `fs` / `opener` | 附件、图片另存 |
| 本地缓存 | `sql`（SQLite） | 离线消息、会话列表、草稿 |
| 自动更新 | `updater` | 差量更新 + 签名校验 + 灰度 |
| 日志 | `log` | 本地滚动日志，便于远程排障 |
| 剪贴板 | `clipboard-manager` | 复制消息、粘贴图片 |

### 3.5 安全配置

```json
// capabilities/main.json —— 只开必需的
{
  "identifier": "main-capability",
  "windows": ["main", "chat-*"],
  "permissions": [
    "core:default",
    "notification:default",
    "dialog:allow-open",
    "fs:allow-download-write",
    "opener:allow-open-url",
    "updater:default"
  ]
}
```

| 项 | 要求 |
|---|---|
| CSP | `default-src 'self'; connect-src 'self' https://api.example.com wss://ws.example.com; img-src 'self' data: https:; script-src 'self'` |
| 外链 | 一律 `opener` 交给系统浏览器，禁止 WebView 内导航 |
| 命令白名单 | 禁止暴露 `shell:allow-execute`、`fs:allow-*` 全局读写 |
| 更新验签 | 内置 minisign 公钥，签名不匹配拒绝安装 |
| 代码签名 | Windows EV 证书（防 SmartScreen）、macOS 公证 + staple |
| 前端 | 禁用 `eval`、禁用远程脚本 |

### 3.6 性能预算

| 指标 | 目标 | 超标动作 |
|---|---|---|
| 安装包 | ≤ 15 MB | 裁剪图标/字体，开 LTO + strip |
| 冷启动 | ≤ 1.5 s | 延迟加载非首屏模块，减少首屏 JS |
| 空闲内存 | ≤ 150 MB | 消息列表虚拟滚动，限制本地缓存条数 |
| 1 万条消息滚动 | 60 fps | 虚拟列表 + 图片懒加载 |
| 更新包 | ≤ 8 MB | 差量更新 |

### 3.7 最大风险：WebView 不是同一个浏览器

| 平台 | 内核 | 风险 |
|---|---|---|
| Windows | WebView2（Chromium，随系统更新） | 低 |
| macOS | WKWebView（Safari） | 中：CSS `:has()`、部分 API 差异 |
| Linux | WebKitGTK | 高：版本旧、特性缺失 |

**对策**：
1. CSS 只用基线特性，Autoprefixer + `browserslist` 覆盖 Safari 15+。
2. CI 增加 macOS/Linux 冒烟 E2E（`tauri-driver`）。
3. 关键页面在三个平台人工过一遍（登录、会话、发消息、文件）。
4. 若 Linux 是必交付平台，评估改用 Chromium 内嵌（体积代价）。

### 3.8 打包与发布

| 平台 | 产物 | 签名 | 更新 |
|---|---|---|---|
| Windows | NSIS `.exe` / MSI | EV 代码签名 | updater + 自建更新源 |
| macOS | `.dmg` + `.app` | 公证（notarization） | updater（需签名） |
| Linux | AppImage / `.deb` | GPG | updater 或包管理器 |

更新通道：`stable` / `beta`，按租户下发 `channel` 配置；灰度 5% → 20% → 100%，失败自动回滚到上一版本（保留双份安装目录）。

---

## 4. Flutter 移动端

### 4.1 定位

**只做轻办公**：IM（会话/通讯录/群）、审批待办、扫码、通知。不做复杂报表、不做后台配置。

### 4.2 技术栈

| 关注点 | 选型 | 理由 |
|---|---|---|
| 状态管理 | **Riverpod 3** | 编译期安全、可测试、无 BuildContext 依赖 |
| 路由 | go_router | 官方推荐，深链接友好 |
| 网络 | dio + retrofit | 拦截器、取消、重试 |
| 序列化 | freezed + json_serializable | 不可变模型 + copyWith |
| 本地库 | drift（SQLite） | 类型安全 SQL，适合消息缓存 |
| 安全存储 | flutter_secure_storage | Token / 设备证书 |
| WebSocket | web_socket_channel + 自研重连 | 心跳、指数退避、seq 补偿 |
| 推送 | FCM / APNs + 国内厂商通道 | 见 4.4 |
| 图片 | cached_network_image | 磁盘缓存 |
| 国际化 | intl + 共享 i18n JSON | 与 Web 同词条 |
| 崩溃 | Sentry / 自建上报 | 带 traceId 关联后端 |

**不选**：GetX（隐式魔法多、难测试）、BLoC（样板多，团队小不划算）。

### 4.3 与 Web 共享什么

```
contracts/openapi.json  →  retrofit 接口 + freezed 模型（build_runner 生成）
contracts/errors.json   →  错误码 → 文案 → 处理动作
contracts/tokens.json   →  ThemeData（颜色/字号/圆角）
contracts/i18n/*.json   →  文案
contracts/ws/*.proto    →  消息帧
```

**不共享**：Vue 组件、Pinia store、路由表。移动端的信息架构与交互本来就不同，强行共享 UI 只会两边都难用。

### 4.4 推送（国内必须认清现实）

| 环境 | 方案 |
|---|---|
| 海外 | FCM（Android）/ APNs（iOS）足够 |
| **国内 Android** | **必须接厂商通道**：华为 HMS、小米、OPPO、vivo、荣耀；或聚合服务（极光/个推/UniPush） |
| iOS | APNs（国内也走 APNs） |

**残酷事实**：国产 ROM 会杀后台，**自建长连接在后台活不过几分钟**。正确架构：

```
前台：WebSocket 长连接（实时）
后台：厂商推送唤醒 → 拉取增量（seq）→ 本地落库 → 通知栏
```

配套：

- 角标数由服务端下发（各厂商 API 不同，需适配层）
- 免打扰时段、消息聚合（同一会话 5 分钟内合并）
- 多端已读同步：任一端已读 → 服务端广播 → 其他端清除

### 4.5 离线与同步

| 项 | 策略 |
|---|---|
| 本地缓存 | 最近 30 天消息 + 全部会话列表（drift） |
| 增量同步 | 启动带 `last_seq`，服务端返回区间；断档则全量拉最近 N 条 |
| 发送失败 | 本地队列 + 重试（指数退避）+ 手动重发 |
| 冲突 | 消息不可变，服务端为准；草稿本地优先 |
| 清理 | 超 30 天或超 500MB 触发清理（保留星标/收藏） |

### 4.6 发布

| 平台 | 渠道 | 注意 |
|---|---|---|
| Android | 应用商店（华为/小米/OPPO/vivo/应用宝）+ 企业 MDM 分发 | 各商店隐私合规要求不同，需单独过审 |
| iOS | App Store / TestFlight | 审核对 IM 有内容审核要求（UGC 必须提供举报/屏蔽） |
| 强制升级 | 后端 `min_version` + 启动校验 | 低于最低版本 → 阻断并引导下载 |

---

## 5. 三端一致性保障

| 项 | 机制 | 检查 |
|---|---|---|
| 接口契约 | OpenAPI 单一来源，三端生成 | CI 生成物 diff |
| 错误码 | `errors.json` 单一来源 | 三端文案快照 |
| 视觉 | `tokens.json` → CSS / ThemeData | 设计走查 |
| 行为 | 关键路径用例三端对齐（登录、发消息、已读、离线） | 手工 + E2E |
| 版本 | 客户端上报 `version/platform`，后端按版本灰度或阻断 | 发布检查 |
| 埋点 | 统一事件名与参数 | 数据看板 |

**兼容矩阵**（后端维护）：

| 后端版本 | Web | Desktop | Mobile |
|---|---|---|---|
| 1.4.x | ≥ 1.4.0 | ≥ 1.3.0 | ≥ 2.1.0 |
| 1.5.x | ≥ 1.4.2 | ≥ 1.4.0 | ≥ 2.2.0 |

---

## 6. 仓库结构（合并后）

```
zcadmin/
├── apps/
│   ├── admin-web/            # Web
│   ├── im-web/               # IM 前端（被 Web 与 Desktop 共用）
│   ├── im-desktop/           # Tauri 壳
│   │   ├── src-tauri/
│   │   └── src/ → 复用 im-web
│   └── im-mobile/            # Flutter（独立 pubspec）
├── packages/{sdk,ui,types,utils}/
├── plugins-web/{crm-web,system-web,...}/
├── contracts/                # 跨端契约
└── crates/ plugins/          # Rust 后端
```

**构建矩阵**：

| 产物 | 命令 | 频率 |
|---|---|---|
| Web | `pnpm build:web` | 每次合并 |
| Desktop | `pnpm build:desktop && cargo tauri build` | 每周/发版 |
| Mobile | `flutter build apk --release` | 双周/发版 |

---

## 7. 团队与技能

| 角色 | 技能 | 人数（4 条产品线时） |
|---|---|---|
| Web 前端 | Vue 3 + TS + AntD | 4–6 |
| 桌面 | Vue + 少量 Rust（Tauri 命令） | 1–2（可由 Web 兼） |
| 移动 | Dart + Flutter + 原生推送 | 2–3 |
| 契约维护 | OpenAPI/Protobuf/设计 token | 1（可由架构兼） |

> **Tauri 的人力成本很低**：壳层代码通常 < 3000 行 Rust，主要工作量在前端复用与三平台适配。

---

## 8. 反模式

| 反模式 | 后果 | 正确做法 |
|---|---|---|
| 用 Flutter Web 做管理后台 | 表格卡、输入法错乱、SEO 无 | Web 用 Vue |
| 用 Electron 替代 Tauri | 包体与内存翻倍 | Tauri 2 |
| 三端各写 API 客户端 | 契约漂移，联调地狱 | OpenAPI 生成 |
| 业务代码里散落 `isTauri` | 无法维护 | 平台适配层统一 |
| 桌面端 WebView 内直接开外链 | 安全问题、导航失控 | `opener` 交给系统 |
| 移动端自建长连接做推送 | 后台被杀，收不到消息 | 厂商通道 + 增量拉取 |
| 强行三端共用 UI 代码 | 三端都难用 | 共享契约，各自实现 |
| Desktop 用最新 CSS 特性 | macOS/Linux 白屏 | 基线特性 + 多平台 E2E |
| 更新不验签 | 供应链攻击 | minisign + 代码签名 |

---

## 9. 上线检查清单

**Tauri**
- [ ] 三平台冒烟通过（登录/会话/发消息/文件）
- [ ] CSP 与 capabilities 已最小化
- [ ] 代码签名 + 公证完成
- [ ] 自动更新验签 + 灰度 + 回滚验证
- [ ] 冷启动/内存/包体达标

**Flutter**
- [ ] 厂商推送通道全部联调（华为/小米/OPPO/vivo）
- [ ] 后台被杀后仍能收到推送
- [ ] 离线缓存与增量同步正确（弱网/断网/切网）
- [ ] 强制升级逻辑生效
- [ ] 隐私合规（权限声明、隐私政策、注销账号）


---

### 落地进度

| 端 | 状态 | 位置 |
|---|---|---|
| Web | ✅ 已实现并构建 | `frontend/` |
| 桌面（Tauri 2） | 🟡 骨架（配置 + 入口，未在 MSVC 环境实编译） | `apps/desktop/` |
| 移动（Flutter） | 🟡 骨架（登录 + 客户列表，未安装 Flutter SDK 验证） | `apps/mobile/` |

说明：桌面端必须用 **MSVC 工具链**（主仓库为绕开 Windows 链接问题用 GNU），
两者互不影响；Flutter 需要先 `flutter create . --platforms=android,ios` 生成平台目录。


## 桌面端实测（Tauri 2）

- 环境：Windows + Rust GNU 工具链；MSVC 因缺少 Windows 10 SDK 无法链接
  （`LNK1181: 无法打开输入文件 kernel32.lib`）。
- 命令：

  ```powershell
  cargo +stable-x86_64-pc-windows-gnu build --release --manifest-path apps/desktop/src-tauri/Cargo.toml
  ```

- 产物：`zcadmin-desktop.exe`，约 21MB，复用 `frontend/dist` 静态资源。
- 图标：`src-tauri/icons/icon.ico` / `icon.png` 已生成并写入 `bundle.icon`。
- 待办：安装包（MSI/NSIS）需要 Tauri CLI 下载 WiX/NSIS，尚未执行。


## 移动端实测（Flutter）

本机工具链：Flutter/Dart/Android SDK 均未安装，Java 为 OpenJDK 8（Flutter 需要 17+），
**无法本地编译 APK**。

CI 增加 `mobile` job（ubuntu-latest + subosito/flutter-action）：

- `flutter create --platforms=android .` 生成平台目录
- `flutter pub get` / `flutter analyze` / `flutter build apk --debug`

真机联调：`flutter run --dart-define=API_BASE=http://<局域网IP>:8080`。

### 桌面端产物

- 便携包：`dist/desktop/zcadmin-desktop-0.1.0-win-x64.zip`（单 exe，约 5.7MB）
- 安装包：本机 WiX 下载超时未生成；CI `desktop` job（windows-latest）负责构建 MSI/NSIS 并上传 artifact。
