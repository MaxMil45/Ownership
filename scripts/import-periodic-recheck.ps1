# Loads the result of the GitHub workflow "Recheck 10-Q/10-K history": the
# `periodic-recheck` artifact (a .zip from the run's page, or the folder it
# unzips to). Copies the report excerpts into data\periodic (keeping any file
# already there), adds the run's check log to data\periodic\checked.tsv (later
# lines win, so these replace the older "none" entries), then loads the folder
# into the database and reruns the plan-termination rule.
#   .\scripts\import-periodic-recheck.ps1 -From "$HOME\Downloads\periodic-recheck.zip"
# Safe to run twice. Makes no SEC requests.
param([Parameter(Mandatory = $true)][string]$From)
$repo = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Set-Location $repo
$env:DATABASE_URL = "postgres://oe:oe_dev_only@127.0.0.1:55432/ownership"
$oe = ".\target\release\oe.exe"
$dest = Join-Path $repo "data\periodic"

function Say([string]$t) { Write-Host "$(Get-Date -Format T) $t" }

if (-not (Test-Path $From)) { Say "not found: $From"; exit 1 }
$src = (Resolve-Path $From).Path
if ($src -like '*.zip') {
    # Unzip into a new, empty folder of its own.
    $unzipped = Join-Path $repo ("data\recheck-import-" + (Get-Date -Format "yyyyMMdd-HHmmss"))
    New-Item -ItemType Directory $unzipped | Out-Null
    Expand-Archive -Path $src -DestinationPath $unzipped
    $src = $unzipped
}
if (-not (Test-Path (Join-Path $src "checked-github.tsv"))) {
    Say "$src does not look like the periodic-recheck artifact (no checked-github.tsv)"; exit 1
}
if (Test-Path (Join-Path $src "summary.txt")) { Get-Content (Join-Path $src "summary.txt") }

# Excerpts and their sidecars, in their <CIK> folders.
New-Item -ItemType Directory -Force $dest | Out-Null
$copied = 0; $kept = 0
Get-ChildItem $src -Recurse -File | Where-Object { $_.Name -like '*.htm' -or $_.Name -like '*.meta.txt' } | ForEach-Object {
    $rel = $_.FullName.Substring($src.Length).TrimStart('\')
    $target = Join-Path $dest $rel
    if (Test-Path $target) { $kept++; return }
    New-Item -ItemType Directory -Force (Split-Path $target) | Out-Null
    Copy-Item $_.FullName $target
    $copied++
}
Say "copied $copied file(s), kept $kept already there"

# The run's check log, after the local one.
$log = Join-Path $dest "checked.tsv"
$lines = Get-Content (Join-Path $src "checked-github.tsv") | Where-Object { $_ -match "`t" }
if (Test-Path $log) {
    $have = [System.Collections.Generic.HashSet[string]]::new([string[]](Get-Content $log))
    $lines = $lines | Where-Object { -not $have.Contains($_) }
}
if ($lines) { [IO.File]::AppendAllLines($log, [string[]]$lines) }
Say "added $(@($lines).Count) line(s) to $log"

docker info *> $null
if ($LASTEXITCODE -ne 0) {
    Say "starting Docker Desktop"
    Start-Process "C:\Program Files\Docker\Docker\Docker Desktop.exe"
    $t = Get-Date
    do { Start-Sleep 5; docker info *> $null } while ($LASTEXITCODE -ne 0 -and ((Get-Date) - $t).TotalMinutes -lt 5)
}
docker-compose -f docker/compose.yaml up -d *> $null
Start-Sleep 10
Say "loading data\periodic into the database"
& $oe db-load data/periodic | Select-Object -Last 2
& $oe plan-terminations data/periodic --db | Select-Object -First 1
Say "done"
