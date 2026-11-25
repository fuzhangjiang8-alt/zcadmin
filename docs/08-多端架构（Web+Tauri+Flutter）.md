# 08 · 多端架构：Web、Tauri与Flutter

既定选型保持不变：Web为Vue，桌面为Tauri 2复用Web，移动为Flutter独立UI。本文只维护共享边界、运行入口和交付要求。

## 1. 定位与复用

| 端 | 职责 | 复用边界 |
|---|---|---|
| Web | 管理后台与业务工作台 | frontend/为主应用 |
| 桌面 | 本地能力、重度操作、桌面入口 | 同一Vue构建+Tauri壳，不重写整套UI |
| 移动 | 移动场景、待办、扫码/外勤等方向 | 独立Flutter界面，不共享Web store/composable |

产品规划不等于这些场景已实现。多端共享协议、错误码、权限与设计约定，设备特有交互分别实现。

## 2. 契约与身份

OpenAPI为HTTP源，errors.json为错误码源；TS通过正式命令生成，Dart生成能力/产物以移动端现状为准，不假定已有完整SDK。
共享资产只有存在并实际生成时才称为已接入；目标中的WS协议、i18n/token全端自动同步需单独建设。
令牌不进入前端localStorage，桌面/移动敏感凭据使用各平台安全能力；Cookie、域、HTTPS和跨域按真实运行环境验证。
默认Web会话、PAT与scope遵循后端同一授权模型，客户端不能绕过租户/权限检查。

## 3. 当前工程入口

| 位置 | 内容 |
|---|---|
| frontend/ | Web及桌面复用的Vue页面 |
| apps/desktop/ | Tauri CLI包与src-tauri壳 |
| apps/mobile/ | Flutter工程，见其README和pubspec |
| contracts/ | 共享HTTP、错误码等真实契约 |

Tauri的frontendDist/devUrl/beforeBuildCommand以tauri.conf.json为准。当前壳安全配置需按发布目标复核，不因能启动就宣称CSP/本地能力权限已通过。

## 4. 本地命令

Web与契约命令只维护在07；桌面在apps/desktop中：

```bash
npm install
npm run dev
npm run build
```

Windows需WebView2及匹配的Rust/系统SDK；MSVC和GNU支持情况依目标/依赖而定。详细已有构建记录见[桌面README](../apps/desktop/README.md)，历史产物不证明当前版本可打包。

移动在apps/mobile中：

```bash
flutter pub get
flutter analyze
flutter run --dart-define=API_BASE=http://<开发机局域网IP>:8080
```

Android需匹配Flutter的JDK/SDK。真机localhost指设备本身；使用实际开发机地址，生产切HTTPS。命令入口见[移动README](../apps/mobile/README.md)。

## 5. 原生能力与安全

Tauri命令、文件系统、通知/剪贴板/托盘等按最小权限逐项开放，不暴露任意shell或全盘文件访问；窗口/外链来源和升级签名要约束。
移动推送、相机、后台运行等先定义权限拒绝、生命周期、重连与数据清理，再实现；不能依赖Web页面行为推断原生正确。
登出/切换租户应清除缓存和在途请求，离线数据需隔离/加密/过期；离线写同步必须有冲突与幂等规则。

## 6. 发布验收

Web构建、Tauri exe、安装包、移动分析/APK及设备运行是不同结果。安装/升级/卸载、WebView兼容、证书/签名、权限拒绝、网络切换与会话清理分别验证。
各平台产物必须匹配后端契约；检查配置地址与秘密不入包，记录版本、平台、构建命令和实际产物。
CI配置存在不代表job通过，便携包不等于MSI/NSIS生成；验收状态集中在[记录](ci-local-rehearsal.md)，不在此维护易过期的包体和本机SDK清单。

## 7. 演进边界

继续复用Web薄壳，不引入另一套桌面UI；Flutter共享契约而非UI/状态。Electron、原生重写、WASM前端等候选比较归档，不是本轮重新选型。
离线、推送、IM等能力按真实场景立项，未明确支持的平台不得出具交付承诺。
