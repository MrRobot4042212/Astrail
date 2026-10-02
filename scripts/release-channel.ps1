# SPDX-FileCopyrightText: 2026 Diego Alfonso Chicoma Ibañez (Dalfon.dev)
# SPDX-License-Identifier: GPL-3.0-only
# Additional terms under GPL-3.0 section 7 apply: see ADDITIONAL-TERMS.md

<#
.SYNOPSIS
  Move a published release between the update channels.

.DESCRIPTION
  `release.yml` publishes every version as a GitHub pre-release, which the stable
  updater endpoint ("releases/latest/download/latest.json") never serves.

  point-beta  Copies that release's `latest.json` to the fixed `channel-beta`
              release, the endpoint of installations that opted in to beta.
  promote     Clears the pre-release flag and marks the release as latest, so the
              stable endpoint starts serving it. Nothing is rebuilt: the asset URLs
              inside `latest.json` point at the versioned tag.
  self-test   Runs the decision logic against fixed data. No network, no `gh`.

  Both real actions refuse to move a channel backwards. The updater never
  downgrades, so an older pointer would only strand new installations on an old
  build while looking like a success.

  Needs the GitHub CLI (`gh`) authenticated through GH_TOKEN.

.EXAMPLE
  pwsh scripts/release-channel.ps1 -Action promote -Version 0.3.1 -Repo MrRobot4042212/Astrail
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory)] [ValidateSet('point-beta', 'promote', 'self-test')] [string]$Action,
  [string]$Version,
  [string]$Repo,
  # Commit the `channel-beta` tag is created on, the first time only.
  [string]$Target
)

$ErrorActionPreference = 'Stop'
$BetaTag = 'channel-beta'

function ConvertTo-PlainVersion([string]$Text) {
  $plain = $Text -replace '^v', ''
  if ($plain -notmatch '^\d+\.\d+\.\d+$') { return $null }
  return [version]$plain
}

# Why `$Release` must not become the stable release, or $null when it may.
# `$Release` is the object `gh release view --json tagName,isDraft,isPrerelease,assets`
# prints; `$LatestTag` is the tag "releases/latest" serves now ('' when none).
function Get-PromotionProblem($Release, [string]$LatestTag, [string]$Version) {
  $wanted = ConvertTo-PlainVersion $Version
  if (-not $wanted) { return "'$Version' is not a plain X.Y.Z version." }
  if (-not $Release) { return "There is no release v$Version." }
  if ($Release.isDraft) { return "v$Version is a draft." }
  if (-not $Release.isPrerelease) { return "v$Version is already on the stable channel." }

  $names = @($Release.assets | ForEach-Object { $_.name })
  if ($names -notcontains 'latest.json') { return "v$Version has no latest.json: the updater could not read it." }
  if (-not ($names -like '*-setup.exe')) { return "v$Version has no installer asset." }
  if (-not ($names -like '*-setup.exe.sig')) { return "v$Version has no updater signature asset." }

  if ($LatestTag) {
    $current = ConvertTo-PlainVersion $LatestTag
    if ($current -and $current -ge $wanted) {
      return "releases/latest already serves $LatestTag; promoting v$Version would move the stable channel backwards."
    }
  }
  return $null
}

# Arguments of the `gh release view` the promotion check reads. The field list is
# one quoted string: written bare, PowerShell reads `a,b,c` as an array and hands
# gh one argument per field, gh refuses them, and the release looked missing. The
# first promotion ever (1.2.0) failed on exactly that.
function Get-ReleaseViewArguments([string]$Version, [string]$Repo) {
  return @('release', 'view', "v$Version", '--repo', $Repo, '--json', 'tagName,isDraft,isPrerelease,assets')
}

# Why `$Manifest` (a parsed latest.json) must not become the beta pointer, or $null.
function Get-PointerProblem($Manifest, [string]$CurrentPointerVersion, [string]$Version) {
  $wanted = ConvertTo-PlainVersion $Version
  if (-not $wanted) { return "'$Version' is not a plain X.Y.Z version." }
  if (-not $Manifest) { return "latest.json of v$Version could not be read." }
  if ($Manifest.version -ne $Version) {
    return "latest.json announces '$($Manifest.version)', not $Version."
  }
  $platform = $Manifest.platforms.'windows-x86_64'
  if (-not $platform) { return 'latest.json has no windows-x86_64 entry.' }
  if ([string]::IsNullOrWhiteSpace($platform.signature)) { return 'latest.json has an empty signature.' }
  # The URL must name the versioned tag: a ".../latest/download/..." URL would stop
  # resolving to this build the moment another release is promoted.
  if ($platform.url -notlike "https://github.com/*/releases/download/v$Version/*") {
    return "latest.json points at '$($platform.url)', not at the v$Version tag."
  }
  if ($CurrentPointerVersion) {
    $current = ConvertTo-PlainVersion $CurrentPointerVersion
    if ($current -and $current -gt $wanted) {
      return "The beta channel already points at $CurrentPointerVersion; $Version would move it backwards."
    }
  }
  return $null
}

function Invoke-SelfTest {
  $script:failed = 0
  # A hashtable sum refuses a key both sides carry, so the variants are copies.
  function Copy-With($Base, $Changes) {
    $copy = $Base.Clone()
    foreach ($key in $Changes.Keys) { $copy[$key] = $Changes[$key] }
    return $copy
  }
  function Assert([string]$Name, $Actual, [bool]$ExpectProblem) {
    $ok = [bool]$Actual -eq $ExpectProblem
    if (-not $ok) { $script:failed++ }
    Write-Host ("{0} {1}{2}" -f $(if ($ok) { 'ok  ' } else { 'FAIL' }), $Name, $(if ($Actual) { " -> $Actual" } else { '' }))
  }
  $assets = @(
    @{ name = 'latest.json' }, @{ name = 'Astrail_0.3.1_x64-setup.exe' }, @{ name = 'Astrail_0.3.1_x64-setup.exe.sig' }
  )
  $beta = @{ tagName = 'v0.3.1'; isDraft = $false; isPrerelease = $true; assets = $assets }

  Assert 'a pre-release newer than latest is promotable' (Get-PromotionProblem $beta 'v0.3.0' '0.3.1') $false
  Assert 'the first release ever is promotable' (Get-PromotionProblem $beta '' '0.3.1') $false
  Assert 'a missing release is refused' (Get-PromotionProblem $null 'v0.3.0' '0.3.1') $true
  Assert 'a draft is refused' (Get-PromotionProblem (Copy-With $beta @{ isDraft = $true }) 'v0.3.0' '0.3.1') $true
  Assert 'an already stable release is refused' (Get-PromotionProblem (Copy-With $beta @{ isPrerelease = $false }) 'v0.3.0' '0.3.1') $true
  Assert 'stable never moves backwards' (Get-PromotionProblem $beta 'v0.3.2' '0.3.1') $true
  Assert 'versions compare as numbers (0.10.0 > 0.9.0)' (Get-PromotionProblem (Copy-With $beta @{ tagName = 'v0.10.0' }) 'v0.9.0' '0.10.0') $false
  Assert 'a release without latest.json is refused' (Get-PromotionProblem (Copy-With $beta @{ assets = @($assets | Select-Object -Skip 1) }) 'v0.3.0' '0.3.1') $true
  Assert 'a release without the signature is refused' (Get-PromotionProblem (Copy-With $beta @{ assets = @($assets | Select-Object -First 2) }) 'v0.3.0' '0.3.1') $true
  Assert 'a suffixed version is refused' (Get-PromotionProblem $beta 'v0.3.0' '0.3.1-beta.1') $true

  $url = 'https://github.com/MrRobot4042212/Astrail/releases/download/v0.3.1/Astrail_0.3.1_x64-setup.exe'
  $manifest = @{ version = '0.3.1'; platforms = @{ 'windows-x86_64' = @{ signature = 'sig'; url = $url } } }
  Assert 'a matching manifest becomes the pointer' (Get-PointerProblem $manifest '0.3.0' '0.3.1') $false
  Assert 'the first pointer ever is accepted' (Get-PointerProblem $manifest '' '0.3.1') $false
  Assert 'repeating the same version is accepted' (Get-PointerProblem $manifest '0.3.1' '0.3.1') $false
  Assert 'the pointer never moves backwards' (Get-PointerProblem $manifest '0.4.0' '0.3.1') $true
  Assert 'a manifest of another version is refused' (Get-PointerProblem $manifest '0.3.0' '0.3.2') $true
  $moving = @{ version = '0.3.1'; platforms = @{ 'windows-x86_64' = @{ signature = 'sig'; url = $url -replace 'download/v0.3.1', 'latest/download' } } }
  Assert 'a URL that is not pinned to the tag is refused' (Get-PointerProblem $moving '0.3.0' '0.3.1') $true
  $unsigned = @{ version = '0.3.1'; platforms = @{ 'windows-x86_64' = @{ signature = ' '; url = $url } } }
  Assert 'an empty signature is refused' (Get-PointerProblem $unsigned '0.3.0' '0.3.1') $true

  # Regression: the field list was split into one argument per field.
  $view = @(Get-ReleaseViewArguments '0.3.1' 'owner/repo')
  $fieldsIntact = $view.Count -eq 7 -and $view[5] -eq '--json' -and $view[6] -is [string] -and
    $view[6] -eq 'tagName,isDraft,isPrerelease,assets'
  Assert 'gh receives the release fields as one argument' $(if ($fieldsIntact) { $null } else { "got: $($view -join ' | ')" }) $false

  if ($script:failed) { throw "$script:failed self-test check(s) failed." }
  Write-Host 'release-channel self-test passed.'
}

# Run `gh`, returning its output, or $null when it exits non-zero (a 404 is an
# answer here, not an error: "there is no such release yet").
function Invoke-Gh {
  $output = & gh @args 2>$null
  if ($LASTEXITCODE -ne 0) { return $null }
  return $output
}

function Read-Manifest([string]$Tag, [string]$Directory) {
  Remove-Item -Recurse -Force $Directory -ErrorAction SilentlyContinue
  New-Item -ItemType Directory -Force $Directory | Out-Null
  & gh release download $Tag --repo $Repo --pattern latest.json --dir $Directory 2>$null
  $file = Join-Path $Directory 'latest.json'
  if ($LASTEXITCODE -ne 0 -or -not (Test-Path $file)) { return $null }
  return Get-Content $file -Raw | ConvertFrom-Json
}

if ($Action -eq 'self-test') {
  Invoke-SelfTest
  return
}

if (-not $Version -or -not $Repo) { throw '-Version and -Repo are required.' }
if (-not (Get-Command gh -ErrorAction SilentlyContinue)) { throw 'The GitHub CLI (gh) is not installed.' }
$scratch = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [IO.Path]::GetTempPath() }

if ($Action -eq 'promote') {
  $viewArguments = Get-ReleaseViewArguments $Version $Repo
  $found = Invoke-Gh @viewArguments
  # No answer means no such release: let the check below say so.
  $release = if ($found) { $found | ConvertFrom-Json } else { $null }
  $latest = Invoke-Gh api "repos/$Repo/releases/latest" --jq .tag_name
  $problem = Get-PromotionProblem $release "$latest" $Version
  if ($problem) { throw $problem }

  & gh release edit "v$Version" --repo $Repo --prerelease=false --latest
  if ($LASTEXITCODE -ne 0) { throw "Could not promote v$Version." }

  $now = Invoke-Gh api "repos/$Repo/releases/latest" --jq .tag_name
  if ("$now" -ne "v$Version") { throw "Promoted, but releases/latest serves '$now' instead of v$Version." }
  Write-Host "v$Version is now the stable release (was '$latest')."
  return
}

# point-beta
$manifest = Read-Manifest "v$Version" (Join-Path $scratch 'beta-pointer-new')
$currentPointer = Read-Manifest $BetaTag (Join-Path $scratch 'beta-pointer-current')
$problem = Get-PointerProblem $manifest "$($currentPointer.version)" $Version
if ($problem) { throw $problem }

if (-not (Invoke-Gh release view $BetaTag --repo $Repo --json tagName)) {
  $notes = 'Not a version. This release only holds the `latest.json` that Astrail installations on the beta update channel read. It is overwritten by every publish.'
  $create = @('release', 'create', $BetaTag, '--repo', $Repo, '--prerelease', '--latest=false',
    '--title', 'Beta channel pointer', '--notes', $notes)
  if ($Target) { $create += @('--target', $Target) }
  & gh @create
  if ($LASTEXITCODE -ne 0) { throw "Could not create the $BetaTag release." }
}

& gh release upload $BetaTag (Join-Path $scratch 'beta-pointer-new/latest.json') --repo $Repo --clobber
if ($LASTEXITCODE -ne 0) { throw "Could not upload latest.json to $BetaTag." }
Write-Host "The beta channel now points at v$Version (was '$($currentPointer.version)')."
