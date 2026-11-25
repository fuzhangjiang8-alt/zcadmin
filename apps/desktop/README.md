# 卓才智能 桌面端（Tauri 2）

复用 `frontend/` 的 Vue 代码，不重写 UI。

## 前置条件

| 依赖 | 说明 |
|---|---|
| Rust | GNU 或 MSVC 工具链均可；本机实测 **GNU 可用** |
| WebView2 Runtime | Win10/11 一般已内置；Server 版需单独安装 |
| Node 22+ | 构建前端与运行 Tauri CLI |
| Visual Studio Build Tools | 仅 MSVC 工具链需要（含 Windows 10 SDK） |

> 本机踩坑：BuildTools 2022 已安装，但**缺少 Windows 10 SDK**，
> MSVC 链接时报 `LNK1181: 无法打开输入文件 kernel32.lib`；
> 改用 GNU 工具链一次通过。

## 构建（实测命令）

```powershell
# release 可执行文件
cargo +stable-x86_64-pc-windows-gnu build --release --manifest-path apps/desktop/src-tauri/Cargo.toml
# 产物：apps/desktop/src-tauri/target/release/zcadmin-desktop.exe（约 21MB）
```

开发模式：

```bash
cd apps/desktop
npm install            # 安装 @tauri-apps/cli
npm run dev            # 启动前端 dev server + 桌面窗口
```

## 图标

`src-tauri/icons/icon.ico` 与 `icon.png` 已提供（256×256，蓝底 ZC）。
`tauri.conf.json` 的 `bundle.icon` 已指向它们。

## 打包

已产出**便携包**（单 exe，前端已内嵌）：

```
dist/desktop/zcadmin-desktop-0.1.0-win-x64.zip   （约 5.7MB）
```

安装包（MSI/NSIS）：

```bash
cd apps/desktop
npm install
npm run build          # 产出 src-tauri/target/release/bundle/
```

本机执行时 WiX 下载超时（GitHub 网络受限），因此 MSI/NSIS 未生成；
CI 的 `desktop` job（windows-latest，自带 MSVC + Windows SDK + 网络）
会构建并上传 `bundle/` 产物。
