# Gate 1 measurement: follow every live feed with alerts for one trading day,
# then report how long after SEC acceptance filings were saved and alerts went
# out. Start it on a weekday before or during market hours; it stops by itself
# at 18:00 New York time (most Form 4s arrive in the two hours after the close).
#   powershell -NoProfile -ExecutionPolicy Bypass -File <repository>\scripts\measure-trading-day.ps1
# Alerts go to the console (and Discord or a webhook if .env sets one). The log
# and the report go to data\alerts\trading_day_<date>.log and .txt.
$repo = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Set-Location $repo
Get-Content .env -Raw | Invoke-Expression
$env:DATABASE_URL = "postgres://oe:oe_dev_only@127.0.0.1:55432/ownership"
$oe = ".\target\release\oe.exe"

function Say([string]$t) { Write-Host "$(Get-Date -Format o) $t" }

if (@(Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'oe.exe' }).Count -gt 0) {
    Say "another oe is running; start this when it has finished (one at a time keeps within the SEC limit)"; exit 1
}

# Today in New York, and 18:00 there in UTC.
$ny = [TimeZoneInfo]::FindSystemTimeZoneById("Eastern Standard Time")
$nowNy = [TimeZoneInfo]::ConvertTimeFromUtc((Get-Date).ToUniversalTime(), $ny)
if ($nowNy.DayOfWeek -in 'Saturday', 'Sunday') { Say "it is the weekend in New York; start this on a weekday"; exit 1 }
$stopNy = $nowNy.Date.AddHours(18)
if ($nowNy -ge $stopNy) { Say "it is after 18:00 in New York; start this before then"; exit 1 }
$untilUtc = [TimeZoneInfo]::ConvertTimeToUtc([DateTime]::SpecifyKind($stopNy, 'Unspecified'), $ny).ToString("yyyy-MM-ddTHH:mm:ssZ")
$day = $nowNy.ToString("yyyy-MM-dd")
# Filings accepted before the first poll (start-up indexing takes about a
# minute and a half) are catching up, not live: count from two minutes in.
$sinceUtc = (Get-Date).ToUniversalTime().AddMinutes(2).ToString("yyyy-MM-ddTHH:mm:ssZ")

docker info *> $null
if ($LASTEXITCODE -ne 0) {
    Say "starting Docker Desktop"
    Start-Process "C:\Program Files\Docker\Docker\Docker Desktop.exe"
    $t = Get-Date
    do { Start-Sleep 5; docker info *> $null } while ($LASTEXITCODE -ne 0 -and ((Get-Date) - $t).TotalMinutes -lt 5)
}
docker-compose -f docker/compose.yaml up -d *> $null
Start-Sleep 10

$out = Join-Path $repo "data\alerts"
New-Item -ItemType Directory -Force $out | Out-Null
$log = Join-Path $out "trading_day_$day.log"
$report = Join-Path $out "trading_day_$day.txt"
Say "following until $untilUtc (18:00 New York); log: $log"
cmd /c "$oe follow --alert --db --until $untilUtc >> `"$log`" 2>&1"
Say "follow exit $LASTEXITCODE"
cmd /c "$oe alert-delays $day --since $sinceUtc > `"$report`" 2>&1"
Get-Content $report
Say "report saved: $report"
