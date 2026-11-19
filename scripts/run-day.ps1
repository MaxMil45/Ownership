<#
.SYNOPSIS
  Downloads one day of SEC filings from every source the engine reads, then
  prints every event the rules find in what has been downloaded so far.

.EXAMPLE
  .\scripts\run-day.ps1 -Date 2026-11-18
  .\scripts\run-day.ps1 -Date 2026-11-05 -All       # every filing of the day, not the first 25 per form

.NOTES
  Needs OE_USER_AGENT (your name and email, which the SEC requires). If it is
  not set, the script loads it from .env in the repository root, which must
  contain one line:  $env:OE_USER_AGENT = "Your Name your.email@example.com"
#>
param(
    [Parameter(Mandatory = $true)][string]$Date,
    [switch]$All
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo

if (-not $env:OE_USER_AGENT) {
    $envFile = Join-Path $repo '.env'
    if (-not (Test-Path $envFile)) { throw "Set `$env:OE_USER_AGENT or create .env (see the notes at the top of this script)." }
    $line = (Get-Content $envFile -Raw).Trim()
    if ($line -notmatch '^\$env:OE_USER_AGENT\s*=\s*"[^"`$;]+"$') { throw ".env must contain exactly one line: `$env:OE_USER_AGENT = `"Your Name your.email@example.com`"" }
    Invoke-Expression $line
}

cargo build --release -q -p oe-cli
if ($LASTEXITCODE -ne 0) { throw "build failed" }
$oe = Join-Path $repo 'target\release\oe.exe'
$count = if ($All) { '--all' } else { '--limit', '25' }

Write-Host "== Downloading $Date" -ForegroundColor Cyan
& $oe daily-form4 $Date @count --include-amendments
& $oe daily-form144 $Date @count
& $oe daily-13dg $Date @count --include-amendments
& $oe daily-proxy $Date --all
& $oe daily-periodic $Date @count

Write-Host "`n== Events in everything downloaded so far" -ForegroundColor Cyan
Write-Host "-- insider.cluster_buy@1 (Form 4; companies with an event only)"
if (Test-Path data\form4) { Get-ChildItem data\form4 -Directory | ForEach-Object { $o = @(& $oe cluster-buys $_.FullName); if ($o -and $o[0] -notmatch ', 0 event') { $_.Name; $o } } }
Write-Host "-- insider.planned_sale@1 (Form 144)"; if (Test-Path data\form144) { & $oe planned-sales data/form144 }
Write-Host "-- activist.new_13d@1 / passive_to_active@1 (Schedule 13D/G)"; if (Test-Path data\sched13) { & $oe activist-events data/sched13 }
Write-Host "-- activist.proxy_contest@1 (contested proxy filings)"; if (Test-Path data\proxy) { & $oe proxy-contests data/proxy }
Write-Host "-- insider.plan_terminated@1 (10-Q/10-K Item 408)"; if (Test-Path data\periodic) { & $oe plan-terminations data/periodic }
