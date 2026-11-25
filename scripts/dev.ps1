# =============================================================================
# 开发启动脚本（Windows PowerShell）
#
# 作用：
#   1. 补齐 MinGW 工具链路径（GNU 版 Rust 需要 dlltool，否则 windows-sys 编译失败）
#   2. 启动后端服务
#
# 用法：
#   powershell -ExecutionPolicy Bypass -File scripts/dev.ps1
# =============================================================================

$ErrorActionPreference = 'Stop'

# ---- 0. 控制台 UTF-8（否则中文日志在旧终端会显示乱码） ----
try {
    chcp 65001 | Out-Null
    [Console]::OutputEncoding = [System.Text.Encoding]::UTF8
    $OutputEncoding = [System.Text.Encoding]::UTF8
} catch {
    # 某些受限终端不允许改编码，忽略即可，不影响程序运行
}

# ---- 1. 补齐工具链 ----
# 如果你的 MinGW 不在这个路径，改这里，或改用 MSVC 工具链：
#   rustup default stable-x86_64-pc-windows-msvc
$mingw = 'C:\msys64\mingw64\bin'
if (Test-Path $mingw) {
    if ($env:PATH -notlike "*$mingw*") {
        $env:PATH = "$mingw;$env:PATH"
        Write-Host "[env] 已加入 MinGW: $mingw" -ForegroundColor DarkGray
    }
} else {
    Write-Host "[warn] 未找到 $mingw，若编译报 dlltool 缺失请手动配置" -ForegroundColor Yellow
}

# ---- 2. 数据库提示 ----
if (-not $env:DATABASE_URL) {
    Write-Host "[info] 未设置 DATABASE_URL，将使用 config/default.toml 中的默认值" -ForegroundColor DarkGray
    Write-Host "[info] 首次运行请先执行: docker compose up -d postgres" -ForegroundColor DarkGray
}

# ---- 3. 启动 ----
Write-Host "[run ] cargo run -p server" -ForegroundColor Cyan
cargo run -p server
