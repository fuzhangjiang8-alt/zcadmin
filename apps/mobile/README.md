# 卓才智能 移动端（Flutter）

骨架阶段：**登录 + 客户列表（只读）**，用于验证后端 API 可以被移动端直接复用。

## 运行

```bash
cd apps/mobile
flutter pub get

# Android 模拟器访问宿主机用 10.0.2.2（默认值）
flutter run

# 真机：改成电脑的局域网 IP
flutter run --dart-define=API_BASE=http://192.168.1.10:8080
```

## 注意

1. Android 9+ 默认禁止明文 HTTP。本地调试需要在
   `android/app/src/main/AndroidManifest.xml` 的 `<application>` 上加
   `android:usesCleartextTraffic="true"`；生产必须走 HTTPS。
2. 会话 Cookie 存在 `SharedPreferences`（移动端没有浏览器的 HttpOnly 容器）。
   正式版建议改用短期 token + 刷新机制，避免长期凭据落盘。
3. 目前没有 `android/`、`ios/` 平台目录，需要先执行：

```bash
cd apps/mobile
flutter create . --platforms=android,ios
```

（骨架刻意不提交平台目录，避免几百个生成文件污染仓库。）

## 后续

| 功能 | 说明 |
|---|---|
| 客户新增/编辑 | 复用 Web 的表单校验规则 |
| 扫码 | `mobile_scanner`，用于设备/IoT 场景 |
| 离线缓存 | `drift` 或 `isar` |
| 推送 | FCM / 厂商通道 |


## 工具链现状（本机）

| 依赖 | 状态 |
|---|---|
| Flutter SDK | ❌ 未安装 |
| Dart SDK | ❌ 未安装 |
| Java | ⚠️ 已安装 OpenJDK 8（Flutter 需要 JDK 17+） |
| Android SDK | ❌ 未安装 |

因此本机**无法编译 APK**；CI 的 `mobile` job 会安装 Flutter stable 并执行：

```
flutter create --platforms=android --project-name zcadmin_mobile .
flutter pub get
flutter analyze
flutter build apk --debug
```

真机运行：

```bash
flutter run --dart-define=API_BASE=http://<局域网IP>:8080
```

## 页面

- 经营看板：合同额/回款/未回款/合同数/客户数/商机数 + 近 6 个月趋势

- 登录
- 客户列表
- 客户详情：基本信息 + 联系人 + 商机 + 合同（含已回款）
- 底部导航：客户 / 商机两个列表
- 商机详情：阶段/金额/预计成交 + 转化合同 + 阶段历史
- 回款列表：关键字 + 日期范围筛选，点击进入合同详情
- 审计告警：看板入口 → 全部/我的订阅切换 → 一键处理 + 订阅设置
- 合同列表 → 合同详情（回款进度 + 最近回款）

## 代码结构

``
lib/
  main.dart              # 入口
  api_client.dart        # 会话 Cookie + 统一错误
  pages/
    login_page.dart
    home_page.dart       # 底部导航
    customers_page.dart
    customer_detail_page.dart
    opportunities_page.dart
    contracts_page.dart
    contract_detail_page.dart
    opportunity_detail_page.dart
``

## 交互

- 客户/商机/合同列表支持下拉刷新（RefreshIndicator）与右上角刷新按钮。
