# =============================================================================
# bench.ps1 - 运行性能基线（真库、完整中间件链）
#
# 前置：PostgreSQL 已启动，DATABASE_URL 指向可写的库。
# 输出：RPS 与 p50/p95/p99（debug 构建，release 会更快）。
# =============================================================================

param(
    [string]$DatabaseUrl = "postgres://zcadmin:zcadmin@127.0.0.1:5432/zcadmin",
    [string]$Password = "admin123456"
)

$ErrorActionPreference = "Stop"
$env:DATABASE_URL = $DatabaseUrl
$env:AUTH_DEV_PASSWORD = $Password

Push-Location (Split-Path -Parent $PSScriptRoot)
try {
    cargo test -p server --test bench -- --ignored --nocapture
} finally {
    Pop-Location
}
