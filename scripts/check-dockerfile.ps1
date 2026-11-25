# =============================================================================
# check-dockerfile.ps1 —— 在没有 Docker 的环境做静态校验
#
# 校验：
#   1) Dockerfile 里 COPY 的源路径都存在；
#   2) docker-compose.yml 能被 YAML 解析；
#   3) .dockerignore 没有误排除构建所需目录。
#
# 真正的镜像构建在 CI 的 docker job（ubuntu-latest 有 Docker）。
# =============================================================================

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    $required = @(
        "Cargo.toml",
        "Cargo.lock",
        "crates/kernel/Cargo.toml",
        "crates/xtask/Cargo.toml",
        "plugins/demo/Cargo.toml",
        "plugins/crm/Cargo.toml",
        "plugins/workflow/Cargo.toml",
        "apps/server/Cargo.toml",
        "frontend/package.json",
        "frontend/package-lock.json",
        "config/default.toml"
    )
    $missing = $required | Where-Object { -not (Test-Path $_) }
    if ($missing) {
        throw "Dockerfile COPY 源缺失: $($missing -join ', ')"
    }

    python -c "import yaml,sys; yaml.safe_load(open('docker-compose.yml',encoding='utf-8')); print('compose yaml ok')"

    $ignore = Get-Content .dockerignore -Raw
    foreach ($needed in @("config", "frontend", "crates", "plugins", "apps")) {
        if ($ignore -match "(?m)^$needed/?$") {
            throw ".dockerignore 误排除了 $needed"
        }
    }

    Write-Host "Dockerfile static check passed (image build is verified by CI docker job)"
} finally {
    Pop-Location
}
