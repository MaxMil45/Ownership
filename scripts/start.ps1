<#
.SYNOPSIS
  Starts everything with one command: the database, the API and the web app,
  then opens the app in your browser, already signed in.

.EXAMPLE
  .\scripts\start.ps1            # start and open the app
  .\scripts\start.ps1 -Update    # first bring the data up to date (scripts\update.ps1), then start
  .\scripts\start.ps1 -NoBrowser # do not open a browser; open the printed link yourself

.NOTES
  - Needs Docker Desktop, Rust (cargo) and Node.js 20 or later.
  - Builds `oe`, the web app and the browser extension only when their sources changed.
  - The app runs at http://127.0.0.1:8730 on this computer only. Keep this
    window open while you use it; Ctrl+C stops it.
  - -Update needs OE_USER_AGENT (see scripts\run-day.ps1), as every download does.
#>
[CmdletBinding()]   # an unknown or mistyped switch is refused, not ignored
param([switch]$Update, [switch]$NoBrowser)
$ErrorActionPreference = 'Continue'   # native programs write progress to stderr; each step checks its exit code
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo
$env:DATABASE_URL = "postgres://oe:oe_dev_only@127.0.0.1:55432/ownership"
$oe = Join-Path $repo "target\release\oe.exe"

function Say([string]$t) { Write-Host "$(Get-Date -Format T) $t" }
function Need([string]$cmd, [string]$what) {
    if (-not (Get-Command $cmd -ErrorAction SilentlyContinue)) { Say "$what is not installed ($cmd not found)"; exit 1 }
}
Need docker "Docker Desktop"; Need cargo "Rust"; Need npm "Node.js"

# Only one oe serve at a time on the port.
$serving = Get-CimInstance Win32_Process -Filter "Name = 'oe.exe'" -ErrorAction SilentlyContinue |
    Where-Object { $_.CommandLine -match '\sserve(\s|$)' }
if ($serving) { Say "oe serve is already running (process $($serving[0].ProcessId)). Use that window, or close it first."; exit 1 }

# 1. The database.
docker info *> $null
if ($LASTEXITCODE -ne 0) {
    Say "starting Docker Desktop"
    Start-Process "C:\Program Files\Docker\Docker\Docker Desktop.exe"
    $t = Get-Date
    do { Start-Sleep 5; docker info *> $null } while ($LASTEXITCODE -ne 0 -and ((Get-Date) - $t).TotalMinutes -lt 5)
    if ($LASTEXITCODE -ne 0) { Say "Docker did not start within 5 minutes"; exit 1 }
}
Say "starting the database"
docker-compose -f docker/compose.yaml up -d *> $null
$t = Get-Date
do { Start-Sleep 2; docker exec oe-postgres pg_isready -U oe -d ownership *> $null } while ($LASTEXITCODE -ne 0 -and ((Get-Date) - $t).TotalSeconds -lt 120)
if ($LASTEXITCODE -ne 0) { Say "the database did not become ready"; exit 1 }

# 2. Fresh data, if asked (it builds oe itself).
if ($Update) {
    & (Join-Path $PSScriptRoot "update.ps1")
    if ($LASTEXITCODE -ne 0) { Say "the update did not finish cleanly; starting with the data there is" }
}

# 3. oe, then the tables (cargo does nothing when nothing changed).
Say "building oe"
cargo build --release -q -p oe-cli
if ($LASTEXITCODE -ne 0) { Say "the build failed"; exit 1 }
& $oe db-migrate | Out-Null
if ($LASTEXITCODE -ne 0) { Say "the database is not reachable"; exit 1 }

# 4. The web app: install once, build when a source file is newer than the build.
Push-Location web
try {
    if (-not (Test-Path node_modules)) {
        Say "installing the web app's packages (once)"
        npm ci --no-audit --no-fund
        if ($LASTEXITCODE -ne 0) { Say "npm ci failed"; exit 1 }
    }
    $built = Get-Item dist\index.html -ErrorAction SilentlyContinue
    $newest = Get-ChildItem src, index.html, package.json, vite.config.ts -Recurse -File |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if (-not $built -or $newest.LastWriteTime -gt $built.LastWriteTime) {
        Say "building the web app"
        npm run build --silent
        if ($LASTEXITCODE -ne 0) { Say "the web app build failed"; exit 1 }
    }
}
finally { Pop-Location }

# 4b. The browser extension, the same way (load extension\dist once in the
#     browser: see extension\README.md; reload it there after a rebuild).
Push-Location extension
try {
    if (-not (Test-Path node_modules)) {
        Say "installing the extension's packages (once)"
        npm ci --no-audit --no-fund
        if ($LASTEXITCODE -ne 0) { Say "npm ci failed (extension)"; exit 1 }
    }
    $built = Get-Item dist\manifest.json -ErrorAction SilentlyContinue
    $newest = Get-ChildItem src, static, package.json, build.mjs -Recurse -File |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if (-not $built -or $newest.LastWriteTime -gt $built.LastWriteTime) {
        Say "building the browser extension (extension\dist)"
        npm run build --silent | Out-Null
        if ($LASTEXITCODE -ne 0) { Say "the extension build failed"; exit 1 }
    }
}
finally { Pop-Location }

# 5. Serve it and open the browser (signed in by the link oe serve opens).
$feed = docker exec oe-postgres psql -U oe -d ownership -Atc "SELECT count(*) FROM event_feed" 2>$null
if ($feed -eq "0") { Say "no events yet: run .\scripts\update.ps1 (or & $oe events-build) to fill them" }
Say "starting the app at http://127.0.0.1:8730 (Ctrl+C stops it)"
if ($NoBrowser) { & $oe serve } else { & $oe serve --open }
