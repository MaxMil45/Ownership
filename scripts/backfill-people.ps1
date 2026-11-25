# Fills the insider names, roles and company names (migration 0007_people)
# for the Form 3/4/5 history already in the database, then builds the name
# index for `oe lookup`. Reloads the quarterly data sets already in data\bulk
# and the Form 3/4/5 XML in data\form4 and data\form4_extra: a repeat load
# adds only the new name rows and changes nothing else (the ledger is
# append-only). About 30 minutes; no SEC requests. Safe to stop and run again.
#   .\scripts\backfill-people.ps1
#   .\scripts\backfill-people.ps1 -From 2020q1    # resume from a quarter
param([string]$From = "2015q1")
$repo = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Set-Location $repo
$env:DATABASE_URL = "postgres://oe:oe_dev_only@127.0.0.1:55432/ownership"
$oe = ".\target\release\oe.exe"

function Say([string]$t) { Write-Host "$(Get-Date -Format T) $t" }

docker info *> $null
if ($LASTEXITCODE -ne 0) {
    Say "starting Docker Desktop"
    Start-Process "C:\Program Files\Docker\Docker\Docker Desktop.exe"
    $t = Get-Date
    do { Start-Sleep 5; docker info *> $null } while ($LASTEXITCODE -ne 0 -and ((Get-Date) - $t).TotalMinutes -lt 5)
}
docker-compose -f docker/compose.yaml up -d *> $null
Start-Sleep 10

Say "building oe"
cargo build --release -p oe-cli
if ($LASTEXITCODE -ne 0) { Say "build failed"; exit 1 }
& $oe db-migrate
if ($LASTEXITCODE -ne 0) { Say "migration failed"; exit 1 }

Say "reloading the quarterly data sets from $From (about 35 s a quarter)"
& $oe bulk-insider --offline --from $From
if ($LASTEXITCODE -ne 0) { Say "bulk reload stopped; run again with -From <the last quarter shown>"; exit 1 }

foreach ($dir in "data/form4", "data/form4_extra") {
    if (Test-Path $dir) {
        Say "reloading $dir"
        & $oe db-load $dir
        if ($LASTEXITCODE -ne 0) { Say "db-load $dir failed"; exit 1 }
    }
}

Say "building the name index"
& $oe people-index
& $oe db-status
Say "done. Try: & $oe lookup elon musk"
