<#
.SYNOPSIS
  Produce the two metrics binaries Meteor bundles as resources.

.DESCRIPTION
  `src-tauri/tauri.conf.json` declares `binaries/PresentMon.exe` and
  `binaries/cputemp.exe` as bundle resources, and both are gitignored (one is a
  third-party download, the other is built from `src-tauri/sidecar/cputemp`).
  A fresh clone therefore cannot `cargo check`/`cargo build` until this runs.

  PresentMon is pinned by version **and verified by SHA-256**: it is a
  third-party executable that gets bundled into the installer and then runs
  elevated on the user's machine, so a silent upstream swap must fail the build,
  not ship. (The installer itself is not Authenticode-signed yet, which is
  exactly why this hash check carries the weight.)

.EXAMPLE
  pwsh scripts/fetch-binaries.ps1
  pwsh scripts/fetch-binaries.ps1 -SkipSidecar   # PresentMon only (no .NET SDK)
#>
[CmdletBinding()]
param(
    [switch]$SkipPresentMon,
    [switch]$SkipSidecar
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# Pinned PresentMon release. Bump both values together, never just the URL.
$PresentMonVersion = '2.4.1'
$PresentMonUrl = "https://github.com/GameTechDev/PresentMon/releases/download/v$PresentMonVersion/PresentMon-$PresentMonVersion-x64.exe"
$PresentMonSha256 = 'D74183E7AE630F72CD3690BE0373ECBFDC6CBB86578148AAB8FA2A7166068F34'

$repoRoot = Split-Path -Parent $PSScriptRoot
$binDir = Join-Path $repoRoot 'src-tauri/binaries'
New-Item -ItemType Directory -Force -Path $binDir | Out-Null

if (-not $SkipPresentMon) {
    $target = Join-Path $binDir 'PresentMon.exe'
    Write-Host "==> PresentMon v$PresentMonVersion"
    Invoke-WebRequest -Uri $PresentMonUrl -OutFile $target -UseBasicParsing
    $actual = (Get-FileHash -Algorithm SHA256 -Path $target).Hash
    if ($actual -ne $PresentMonSha256) {
        Remove-Item $target -Force
        throw "PresentMon SHA-256 mismatch. Expected $PresentMonSha256, got $actual. Refusing to bundle it."
    }
    Write-Host "    SHA-256 verified: $actual"
    # The hash pins the bytes; the signature says who produced them, so a pin
    # bumped to a tampered upstream asset is still refused.
    $signature = Get-AuthenticodeSignature -FilePath $target
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch '^CN=Intel Corporation,') {
        Remove-Item $target -Force
        throw "PresentMon is not validly signed by Intel ($($signature.Status), $($signature.SignerCertificate.Subject)). Refusing to bundle it."
    }
    Write-Host "    Authenticode verified: $($signature.SignerCertificate.Subject)"
}

if (-not $SkipSidecar) {
    Write-Host '==> cputemp sidecar (dotnet publish)'
    $csproj = Join-Path $repoRoot 'src-tauri/sidecar/cputemp/cputemp.csproj'
    # Publish settings live in the csproj so this and CI produce the same binary.
    & dotnet publish $csproj -c Release -r win-x64 -o $binDir
    if ($LASTEXITCODE -ne 0) { throw "dotnet publish failed ($LASTEXITCODE)" }
}

Get-ChildItem $binDir -Filter *.exe |
    Select-Object Name, @{n = 'MB'; e = { [math]::Round($_.Length / 1MB, 1) } } |
    Format-Table -AutoSize
