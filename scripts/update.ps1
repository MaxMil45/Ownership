<#
.SYNOPSIS
  Brings everything up to date: downloads every SEC filing since the last
  time this script finished, loads them into the database, and rebuilds the
  events and the name index. Run it whenever you like; there is nothing to
  schedule. It always continues from where it last stopped.

.EXAMPLE
  .\scripts\update.ps1                  # from the last finished day to yesterday (US Eastern)
  .\scripts\update.ps1 -ThroughToday    # also today (the SEC's index for a day appears in the evening)
  .\scripts\update.ps1 -From 2026-11-10 # start from a given day instead

.NOTES
  - Needs OE_USER_AGENT (see run-day.ps1) and Docker Desktop for the database.
  - Takes about 10 minutes for one day, longer after a long break (a full
    day of filings downloads in 5 to 10 minutes at the SEC's 8 requests a second).
  - Stops if it is already running or if `oe follow` is running (both use the
    SEC's request limit). Safe to stop and run again: saved filings are skipped,
    and the "last finished day" moves forward only when every download worked.
  - Does not download House reports (they need your own OE_OFFICIALS_ACK) or the
    SEC's quarterly Form 3/4/5 data sets (`oe bulk-insider` when a new quarter appears).
#>
param(
    [string]$From,
    [switch]$ThroughToday
)
$ErrorActionPreference = 'Continue'   # native programs write progress to stderr; every step checks its exit code
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo
$env:DATABASE_URL = "postgres://oe:oe_dev_only@127.0.0.1:55432/ownership"

function Say([string]$t) { Write-Host "$(Get-Date -Format T) $t" }

if (-not $env:OE_USER_AGENT) {
    $envFile = Join-Path $repo '.env'
    if (-not (Test-Path $envFile)) { throw "Set `$env:OE_USER_AGENT or create .env (see run-day.ps1)." }
    $line = (Get-Content $envFile -Raw).Trim()
    if ($line -notmatch '^\$env:OE_USER_AGENT\s*=\s*"[^"`$;]+"$') { throw ".env must contain exactly one line: `$env:OE_USER_AGENT = `"Your Name your.email@example.com`"" }
    Invoke-Expression $line
}

# One update at a time, and not beside `oe follow` or another download: each
# `oe` keeps its own count of SEC requests.
$stateDir = Join-Path $repo "data\update"
New-Item -ItemType Directory -Force $stateDir | Out-Null
$lock = Join-Path $stateDir "running.txt"
if (Test-Path $lock) {
    $other = [int](Get-Content $lock -ErrorAction SilentlyContinue | Select-Object -First 1)
    if ($other -and (Get-Process -Id $other -ErrorAction SilentlyContinue)) { Say "another update is running (process $other)"; exit 1 }
}
$busy = Get-CimInstance Win32_Process -Filter "Name = 'oe.exe'" -ErrorAction SilentlyContinue |
    Where-Object { $_.CommandLine -match '\s(follow|daily-\S+|bulk-insider|house-ptr|recent-form4)(\s|$)' }
if ($busy) { Say "a download is running (oe $($busy[0].CommandLine -replace '^.*oe.exe\"?\s*','')). Wait for it, or close it: they share the SEC's request limit."; exit 1 }
Set-Content $lock $PID

try {
    $eastern = [TimeZoneInfo]::ConvertTimeBySystemTimeZoneId([DateTime]::UtcNow, "Eastern Standard Time")
    $to = if ($ThroughToday) { $eastern.Date } else { $eastern.Date.AddDays(-1) }

    Say "building oe"
    cargo build --release -q -p oe-cli
    if ($LASTEXITCODE -ne 0) { throw "build failed" }
    $oe = Join-Path $repo "target\release\oe.exe"

    docker info *> $null
    if ($LASTEXITCODE -ne 0) {
        Say "starting Docker Desktop"
        Start-Process "C:\Program Files\Docker\Docker\Docker Desktop.exe"
        $t = Get-Date
        do { Start-Sleep 5; docker info *> $null } while ($LASTEXITCODE -ne 0 -and ((Get-Date) - $t).TotalMinutes -lt 5)
    }
    docker-compose -f docker/compose.yaml up -d *> $null
    Start-Sleep 10
    & $oe db-migrate | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "the database is not reachable" }

    # Where to start: -From, else the last finished day, else the newest
    # filing in the database (3 days back, to be safe).
    $stateFile = Join-Path $stateDir "last-day.txt"
    if ($From) { $start = [DateTime]::ParseExact($From, "yyyy-MM-dd", $null) }
    elseif (Test-Path $stateFile) { $start = [DateTime]::ParseExact((Get-Content $stateFile -First 1).Trim(), "yyyy-MM-dd", $null).AddDays(1) }
    else {
        $status = & $oe db-status | Out-String
        if ($status -notmatch 'accepted between .+? and (\d{4}-\d{2}-\d{2})') { throw "cannot tell the newest filing in the database; use -From YYYY-MM-DD" }
        $start = [DateTime]::ParseExact($Matches[1], "yyyy-MM-dd", $null).AddDays(-3)
        Say "first run: starting 3 days before the newest filing in the database ($($Matches[1]))"
    }
    if ($start -gt $to) { Say "already up to date (through $($to.ToString('yyyy-MM-dd')))"; exit 0 }
    $a = $start.ToString("yyyy-MM-dd"); $b = $to.ToString("yyyy-MM-dd")
    Say "updating $a to $b (weekdays; holidays are skipped)"

    $failed = @()
    $steps = @(
        @{ Name = "Forms 3/4/5";      Args = @("daily-form4", $a, "--to", $b, "--all", "--include-amendments") },
        @{ Name = "Form 144";         Args = @("daily-form144", $a, "--to", $b, "--all", "--include-amendments") },
        @{ Name = "Schedule 13D/G";   Args = @("daily-13dg", $a, "--to", $b, "--all", "--include-amendments") },
        @{ Name = "contested proxies"; Args = @("daily-proxy", $a, "--to", $b, "--all") },
        @{ Name = "10-Q/10-K";        Args = @("daily-periodic", $a, "--to", $b, "--all") }
    )
    foreach ($s in $steps) {
        Say "downloading $($s.Name)"
        & $oe @($s.Args) | Select-Object -Last 3
        if ($LASTEXITCODE -ne 0) { $failed += $s.Name; Say "$($s.Name): some days failed (listed above); run again to retry them" }
    }

    $days = [int][Math]::Ceiling(((Get-Date) - $start).TotalDays) + 1
    Say "loading files written in the last $days day(s)"
    foreach ($dir in "data/form4", "data/form144", "data/sched13", "data/proxy", "data/periodic") {
        if (Test-Path $dir) { & $oe db-load $dir --changed-within $days | Select-Object -Last 1 }
    }

    Say "refreshing the database statistics"
    & $oe db-maintain

    Say "rebuilding the events (about 4 minutes)"
    & $oe events-build | Select-Object -Last 1
    Say "rebuilding the name index"
    & $oe people-index

    if ($failed.Count -eq 0) {
        Set-Content $stateFile $b
        Say "done: up to date through $b"
    } else {
        Say "done, but not marked up to date: $($failed -join ', ') had failed days. Run .\scripts\update.ps1 again."
        exit 2
    }
}
finally {
    Remove-Item $lock -ErrorAction SilentlyContinue
}
