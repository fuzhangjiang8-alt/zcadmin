# =============================================================================
# smoke.ps1 - end-to-end smoke test for zcadmin
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File scripts/smoke.ps1
#   powershell -ExecutionPolicy Bypass -File scripts/smoke.ps1 -StartServer
#   powershell -ExecutionPolicy Bypass -File scripts/smoke.ps1 -ForceRls -DbUrl "postgres://..."
#
# What it does:
#   1. (optional) starts the API server and waits for /healthz
#   2. logs in and keeps the session cookie
#   3. calls every read endpoint + a CRM create/update/delete round-trip
#   4. (optional) applies ops/force_rls.sql and repeats the whole suite
#      - if the second run fails, ops/disable_rls.sql is applied automatically
#
# Exit code: 0 = all checks passed, 1 = failures (details on stdout)
# =============================================================================

param(
    [string]$BaseUrl = "http://localhost:8080",
    [string]$Username = "admin",
    [string]$Password = $env:AUTH_DEV_PASSWORD,
    [string]$DbUrl = "postgres://zcadmin:zcadmin@localhost:5432/zcadmin",
    [switch]$StartServer,
    [switch]$ForceRls
)

$ErrorActionPreference = "Stop"
$script:Failures = 0
$script:Server = $null
$script:Session = New-Object Microsoft.PowerShell.Commands.WebRequestSession

if ([string]::IsNullOrWhiteSpace($Password)) { $Password = "admin123456" }

function Write-Step([string]$Message) {
    Write-Host ""
    Write-Host "== $Message" -ForegroundColor Cyan
}

function Invoke-Api {
    param(
        [string]$Method,
        [string]$Path,
        $Body = $null
    )
    $uri = "$BaseUrl$Path"
    $params = @{
        Uri                = $uri
        Method             = $Method
        WebSession         = $script:Session
        UseBasicParsing    = $true
        TimeoutSec         = 30
    }
    if ($null -ne $Body) {
        $params.ContentType = "application/json"
        $params.Body = ($Body | ConvertTo-Json -Depth 6)
    }
    $response = Invoke-WebRequest @params
    $content = $response.Content
    if ([string]::IsNullOrWhiteSpace($content)) { return $null }
    return $content | ConvertFrom-Json
}

function Check {
    param([string]$Name, [scriptblock]$Action)
    try {
        & $Action | Out-Null
        Write-Host "  OK   $Name" -ForegroundColor Green
    } catch {
        $script:Failures++
        Write-Host "  FAIL $Name" -ForegroundColor Red
        Write-Host "       $($_.Exception.Message)" -ForegroundColor DarkRed
    }
}

function Wait-Healthy {
    param([int]$Seconds = 120)
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $deadline) {
        try {
            Invoke-WebRequest -Uri "$BaseUrl/healthz" -UseBasicParsing -TimeoutSec 3 | Out-Null
            return $true
        } catch {
            Start-Sleep -Seconds 2
        }
    }
    return $false
}

function Invoke-Suite {
    Write-Step "auth"
    Check "login" {
        $body = @{ username = $Username; password = $Password; tenant = $null }
        Invoke-Api -Method Post -Path "/api/auth/login" -Body $body | Out-Null
    }
    Check "me" {
        Invoke-Api -Method Get -Path "/api/auth/me" | Out-Null
    }

    Write-Step "system reads"
    $reads = @(
        "/api/system/users?page=1&size=5",
        "/api/system/roles",
        "/api/system/permissions",
        "/api/system/menus",
        "/api/system/depts",
        "/api/system/dicts",
        "/api/system/configs",
        "/api/system/notices",
        "/api/system/audit-logs?page=1&size=5",
        "/api/system/files"
    )
    foreach ($path in $reads) {
        Check "GET $path" { Invoke-Api -Method Get -Path $path | Out-Null }
    }

    Write-Step "crm round-trip"
    $customerId = $null
    Check "create customer" {
        $suffix = [DateTime]::UtcNow.ToString("yyyyMMddHHmmss")
        $body = @{
            code   = "SMOKE-$suffix"
            name   = "Smoke Test Customer"
            level  = "normal"
            status = "potential"
        }
        $created = Invoke-Api -Method Post -Path "/api/crm/customers" -Body $body
        $script:CustomerId = $created.data.id
    }
    Check "list customers" {
        Invoke-Api -Method Get -Path "/api/crm/customers?page=1&size=5" | Out-Null
    }
    if ($script:CustomerId) {
        Check "update customer" {
            $body = @{ name = "Smoke Test Customer (updated)" }
            Invoke-Api -Method Patch -Path "/api/crm/customers/$($script:CustomerId)" -Body $body | Out-Null
        }
        Check "delete customer" {
            Invoke-Api -Method Delete -Path "/api/crm/customers/$($script:CustomerId)" | Out-Null
        }
    }

    Write-Step "notices"
    Check "unread count" { Invoke-Api -Method Get -Path "/api/system/notices/unread-count" | Out-Null }
}

# ---------------------------------------------------------------------------
# main
# ---------------------------------------------------------------------------
Write-Host "zcadmin smoke test -> $BaseUrl" -ForegroundColor White

if ($StartServer) {
    Write-Step "starting server"
    $env:DATABASE_URL = $DbUrl
    $script:Server = Start-Process -FilePath "cargo" -ArgumentList @("run", "-q", "-p", "server") -PassThru -NoNewWindow
    if (-not (Wait-Healthy)) {
        Write-Host "server did not become healthy" -ForegroundColor Red
        if ($script:Server -and -not $script:Server.HasExited) { Stop-Process -Id $script:Server.Id -Force }
        exit 1
    }
    Write-Host "  server is up" -ForegroundColor Green
}

try {
    Invoke-Suite

    if ($ForceRls) {
        Write-Step "applying FORCE ROW LEVEL SECURITY"
        $psql = Get-Command psql -ErrorAction SilentlyContinue
        if (-not $psql) {
            Write-Host "psql not found - skipping FORCE test" -ForegroundColor Yellow
            exit 1
        }
        & psql $DbUrl -v ON_ERROR_STOP=1 -f "ops/force_rls.sql" | Out-Null
        if ($LASTEXITCODE -ne 0) {
            Write-Host "force_rls.sql failed" -ForegroundColor Red
            exit 1
        }

        $before = $script:Failures
        Invoke-Suite

        if ($script:Failures -gt $before) {
            Write-Host ""
            Write-Host "FORCE caused failures - rolling back" -ForegroundColor Red
            & psql $DbUrl -v ON_ERROR_STOP=1 -f "ops/disable_rls.sql" | Out-Null
        } else {
            Write-Host ""
            Write-Host "FORCE passed all checks (keep it enabled)" -ForegroundColor Green
        }
    }
} finally {
    if ($script:Server -and -not $script:Server.HasExited) {
        Stop-Process -Id $script:Server.Id -Force
    }
}

Write-Host ""
if ($script:Failures -eq 0) {
    Write-Host "ALL CHECKS PASSED" -ForegroundColor Green
    exit 0
} else {
    Write-Host "$($script:Failures) CHECK(S) FAILED" -ForegroundColor Red
    exit 1
}
