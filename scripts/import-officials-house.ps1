# Opens the result of the GitHub workflow "Download House PTRs": the
# `officials-house-encrypted` artifact (a .zip from the run's page, or the
# folder it unzips to). Decrypts it with the password you saved as the secret
# OE_OFFICIALS_KEY (asked for here, or taken from $env:OE_OFFICIALS_KEY) and
# unpacks the reports into data\officials\house. Running it again replaces the
# same files with identical ones. Makes no requests.
#   .\scripts\import-officials-house.ps1 -From "$HOME\Downloads\officials-house-encrypted.zip"
# These reports may not be used commercially; never commit or publish them.
param([Parameter(Mandatory = $true)][string]$From)
$repo = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Set-Location $repo

function Say([string]$t) { Write-Host "$(Get-Date -Format T) $t" }

$openssl = @("C:\Program Files\Git\usr\bin\openssl.exe", "C:\Program Files\Git\mingw64\bin\openssl.exe") |
    Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $openssl) { $openssl = (Get-Command openssl -ErrorAction SilentlyContinue).Source }
if (-not $openssl) { Say "openssl not found; it comes with Git for Windows"; exit 1 }

if (-not (Test-Path $From)) { Say "not found: $From"; exit 1 }
$work = Join-Path $repo ("data\officials-import-" + (Get-Date -Format "yyyyMMdd-HHmmss"))
New-Item -ItemType Directory $work | Out-Null
$src = (Resolve-Path $From).Path
if ($src -like '*.zip') {
    Expand-Archive -Path $src -DestinationPath $work
    $src = $work
}
$enc = Join-Path $src "officials-house.tgz.enc"
if (-not (Test-Path $enc)) { Say "$src does not look like the officials-house-encrypted artifact"; exit 1 }
if (Test-Path (Join-Path $src "summary.txt")) { Get-Content (Join-Path $src "summary.txt") }

if (-not $env:OE_OFFICIALS_KEY) {
    $secure = Read-Host "Password (the OE_OFFICIALS_KEY secret)" -AsSecureString
    $env:OE_OFFICIALS_KEY = [Runtime.InteropServices.Marshal]::PtrToStringAuto(
        [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure))
}
$tgz = Join-Path $work "house.tgz"
& $openssl enc -d -aes-256-cbc -pbkdf2 -iter 600000 -in $enc -out $tgz -pass env:OE_OFFICIALS_KEY
$ok = $LASTEXITCODE -eq 0
Remove-Item Env:\OE_OFFICIALS_KEY
if (-not $ok) { Say "could not decrypt: wrong password, or a damaged download"; Remove-Item $work -Recurse -Force; exit 1 }

$dest = Join-Path $repo "data\officials"
New-Item -ItemType Directory -Force $dest | Out-Null
tar -xzf $tgz -C $dest
if ($LASTEXITCODE -ne 0) { Say "could not unpack $tgz"; exit 1 }
Remove-Item $work -Recurse -Force
$pdfs = @(Get-ChildItem (Join-Path $dest "house") -Recurse -Filter *.pdf).Count
Say "unpacked into $dest\house: $pdfs PTR PDF(s) in all"
