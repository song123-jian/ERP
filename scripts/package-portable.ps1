[CmdletBinding()]
param(
  [string]$ExecutablePath = '',
  [string]$RuntimePath = '',
  [string]$OutputDirectory = '',
  [switch]$SkipArchive
)

$ErrorActionPreference = 'Stop'
if ($PSVersionTable.PSEdition -eq 'Desktop') {
  $env:PSModulePath = Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\Modules'
}
Import-Module Microsoft.PowerShell.Security -ErrorAction Stop
$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$runtimeVersion = '153.0.4234.32'
$runtimeArchitecture = 'x64'
$runtimeFolderName = "Microsoft.WebView2.FixedVersionRuntime.$runtimeVersion.$runtimeArchitecture"
$runtimeFileCount = 257
$runtimeBytes = 699494316L
$runtimeSourceUrl = 'https://msedge.sf.dl.delivery.mp.microsoft.com/filestreamingservice/files/c3d95bc1-a0a7-4ca6-aaa1-fa0ac3dd1a37/Microsoft.WebView2.FixedVersionRuntime.153.0.4234.32.x64.cab'
$runtimeArchiveSha256 = '2CB653A74426F0AA802C2396775C6BC674FD662D5396BD677F47BFA6E12EBA9C'
$requiredDirectories = @('data', 'backup', 'export', 'files', 'logs', 'state', 'config')

if ([string]::IsNullOrWhiteSpace($ExecutablePath)) {
  $ExecutablePath = Join-Path $repoRoot 'src-tauri\target\release\plastic-sales-erp.exe'
}
if ([string]::IsNullOrWhiteSpace($RuntimePath)) {
  $RuntimePath = Join-Path $repoRoot "artifacts\webview2-runtime\$runtimeFolderName"
}
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
  $OutputDirectory = Join-Path $repoRoot 'artifacts\portable'
}

function Write-Utf8NoBom([string]$Path, [string]$Content) {
  $utf8 = New-Object System.Text.UTF8Encoding($false)
  [System.IO.File]::WriteAllText($Path, $Content, $utf8)
}

function Get-Sha256([string]$Path) {
  $stream = [System.IO.File]::OpenRead($Path)
  try {
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
      return ([System.BitConverter]::ToString($sha.ComputeHash($stream))).Replace('-', '')
    } finally {
      $sha.Dispose()
    }
  } finally {
    $stream.Dispose()
  }
}

function Get-RelativePackagePath([string]$Root, [string]$Path) {
  $rootPrefix = [System.IO.Path]::GetFullPath($Root).TrimEnd('\') + '\'
  $fullPath = [System.IO.Path]::GetFullPath($Path)
  if (-not $fullPath.StartsWith($rootPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Path is outside the package root: $fullPath"
  }
  return $fullPath.Substring($rootPrefix.Length).Replace('\', '/')
}

$sourceExe = (Resolve-Path -LiteralPath $ExecutablePath).Path
$sourceRuntime = (Resolve-Path -LiteralPath $RuntimePath).Path
$packageJson = Get-Content -Raw -LiteralPath (Join-Path $repoRoot 'package.json') | ConvertFrom-Json
$tauriConfig = Get-Content -Raw -LiteralPath (Join-Path $repoRoot 'src-tauri\tauri.conf.json') | ConvertFrom-Json
$cargoToml = Get-Content -Raw -LiteralPath (Join-Path $repoRoot 'src-tauri\Cargo.toml')
$cargoVersionMatch = [regex]::Match($cargoToml, '(?m)^version\s*=\s*"([^"]+)"')
if (-not $cargoVersionMatch.Success) {
  throw 'Could not read the application version from src-tauri/Cargo.toml.'
}
$version = [string]$packageJson.version
$versions = @($version, [string]$tauriConfig.version, $cargoVersionMatch.Groups[1].Value) | Select-Object -Unique
if ($versions.Count -ne 1) {
  throw "Source versions do not match: $($versions -join ', ')."
}

$exeVersion = (Get-Item -LiteralPath $sourceExe).VersionInfo.FileVersion
if ($exeVersion -ne $version) {
  throw "Release executable version mismatch. Expected $version, found $exeVersion. Rebuild the release executable."
}

$runtimeExe = Join-Path $sourceRuntime 'msedgewebview2.exe'
if (-not (Test-Path -LiteralPath $runtimeExe -PathType Leaf)) {
  throw "WebView2 fixed runtime is incomplete: $runtimeExe is missing."
}
$actualRuntimeVersion = (Get-Item -LiteralPath $runtimeExe).VersionInfo.ProductVersion
if ($actualRuntimeVersion -ne $runtimeVersion) {
  throw "WebView2 runtime version mismatch. Expected $runtimeVersion, found $actualRuntimeVersion."
}
$runtimeFiles = @(Get-ChildItem -LiteralPath $sourceRuntime -File -Recurse)
$actualRuntimeBytes = ($runtimeFiles | Measure-Object Length -Sum).Sum
if ($runtimeFiles.Count -ne $runtimeFileCount -or $actualRuntimeBytes -ne $runtimeBytes) {
  throw "WebView2 runtime content mismatch. Expected $runtimeFileCount files and $runtimeBytes bytes, found $($runtimeFiles.Count) files and $actualRuntimeBytes bytes."
}
$runtimeSignature = Microsoft.PowerShell.Security\Get-AuthenticodeSignature -LiteralPath $runtimeExe
if ($runtimeSignature.Status -ne 'Valid' -or $runtimeSignature.SignerCertificate.Subject -notmatch 'Microsoft Corporation') {
  throw "WebView2 runtime signature is not a valid Microsoft signature: $($runtimeSignature.Status)."
}

$outputRoot = [System.IO.Path]::GetFullPath($OutputDirectory)
$packageName = "plastic-sales-erp-$version-windows-x64-portable"
$finalRoot = Join-Path $outputRoot $packageName
$archivePath = Join-Path $outputRoot "$packageName.zip"
if (Test-Path -LiteralPath $finalRoot) {
  throw "Portable package already exists: $finalRoot"
}
if (-not $SkipArchive -and (Test-Path -LiteralPath $archivePath)) {
  throw "Portable archive already exists: $archivePath"
}

New-Item -ItemType Directory -Path $outputRoot -Force | Out-Null
$stagingRoot = Join-Path $outputRoot (".staging-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $stagingRoot | Out-Null

try {
  Copy-Item -LiteralPath $sourceExe -Destination (Join-Path $stagingRoot 'plastic-sales-erp.exe')
  foreach ($directory in $requiredDirectories) {
    New-Item -ItemType Directory -Path (Join-Path $stagingRoot $directory) | Out-Null
  }
  New-Item -ItemType Directory -Path (Join-Path $stagingRoot 'runtime') | Out-Null
  Copy-Item -LiteralPath $sourceRuntime -Destination (Join-Path $stagingRoot 'runtime\webview2') -Recurse
  New-Item -ItemType Directory -Path (Join-Path $stagingRoot 'licenses') | Out-Null

  $frontendLicenseOutput = & pnpm licenses list --prod --json
  if ($LASTEXITCODE -ne 0) {
    throw "pnpm licenses failed with exit code $LASTEXITCODE."
  }
  $frontendLicenseGroups = ($frontendLicenseOutput -join "`n") | ConvertFrom-Json
  $frontendDependencies = @()
  foreach ($licenseGroup in $frontendLicenseGroups.PSObject.Properties) {
    foreach ($dependency in @($licenseGroup.Value)) {
      $frontendDependencies += [ordered]@{
        name = [string]$dependency.name
        versions = @($dependency.versions)
        license = [string]$dependency.license
        homepage = [string]$dependency.homepage
      }
    }
  }
  $frontendDependencies = @($frontendDependencies | Sort-Object name)

  $cargoMetadataOutput = & cargo metadata --manifest-path (Join-Path $repoRoot 'src-tauri\Cargo.toml') --format-version 1 --locked --filter-platform x86_64-pc-windows-msvc
  if ($LASTEXITCODE -ne 0) {
    throw "cargo metadata failed with exit code $LASTEXITCODE."
  }
  $cargoMetadata = ($cargoMetadataOutput -join "`n") | ConvertFrom-Json
  $rustDependencies = @(
    $cargoMetadata.packages |
      Where-Object { $_.name -ne 'plastic-sales-erp' } |
      Sort-Object name, version |
      ForEach-Object {
        [ordered]@{
          name = [string]$_.name
          version = [string]$_.version
          license = [string]$_.license
          repository = [string]$_.repository
        }
      }
  )

  $dependencyManifest = [ordered]@{
    generated_at_utc = (Get-Date).ToUniversalTime().ToString('o')
    application_version = $version
    frontend = $frontendDependencies
    rust = $rustDependencies
    webview2 = [ordered]@{
      name = 'Microsoft Edge WebView2 Fixed Version Runtime'
      version = $runtimeVersion
      architecture = $runtimeArchitecture
      file_count = $runtimeFileCount
      bytes = $runtimeBytes
      source = $runtimeSourceUrl
      archive_sha256 = $runtimeArchiveSha256
      license_information = 'https://developer.microsoft.com/en-us/microsoft-edge/webview2/'
      bundled_third_party_notices = 'runtime/webview2/show_third_party_software_licenses.bat'
    }
  }
  Write-Utf8NoBom (Join-Path $stagingRoot 'licenses\DEPENDENCIES.json') ($dependencyManifest | ConvertTo-Json -Depth 8)

  $notices = @"
Third-party dependency and license inventory
============================================

Application version: $version
WebView2 Fixed Version Runtime: $runtimeVersion ($runtimeArchitecture)

The machine-readable dependency inventory is licenses/DEPENDENCIES.json.
Microsoft WebView2 license information and downloads:
https://developer.microsoft.com/en-us/microsoft-edge/webview2/
WebView2 third-party notices can be opened with:
runtime/webview2/show_third_party_software_licenses.bat

The SPDX identifiers and project links in DEPENDENCIES.json are generated from
the locked pnpm and Cargo dependency metadata used to build this release.
"@
  Write-Utf8NoBom (Join-Path $stagingRoot 'licenses\THIRD-PARTY-NOTICES.txt') $notices

  $portableReadme = @"
Plastic Sales ERP $version - Windows x64 portable release
==========================================================

Start: plastic-sales-erp.exe
Installation: none. Extract the complete directory before running.
Runtime: Microsoft Edge WebView2 Fixed Version Runtime $runtimeVersion is bundled
         in runtime/webview2 and is selected automatically before the UI starts.
Data: data, backup, export, files, logs, state, and config are writable directories.
Move: close the application cleanly before moving the complete directory or USB drive.
Integrity: verify release-manifest.json and release-manifest.sha256 before delivery.
Licenses: see licenses/DEPENDENCIES.json and licenses/THIRD-PARTY-NOTICES.txt.
"@
  Write-Utf8NoBom (Join-Path $stagingRoot 'PORTABLE-README.txt') $portableReadme

  $staticFileBytes = 0L
  $staticFiles = @(
    Get-ChildItem -LiteralPath $stagingRoot -File -Recurse |
      Sort-Object FullName |
      ForEach-Object {
        $staticFileBytes += $_.Length
        [ordered]@{
          path = Get-RelativePackagePath $stagingRoot $_.FullName
          size = $_.Length
          sha256 = Get-Sha256 $_.FullName
        }
      }
  )
  $manifest = [ordered]@{
    schema_version = 1
    product = 'Plastic Sales ERP'
    version = $version
    platform = 'windows'
    architecture = 'x64'
    entrypoint = 'plastic-sales-erp.exe'
    generated_at_utc = (Get-Date).ToUniversalTime().ToString('o')
    required_directories = $requiredDirectories
    webview2 = [ordered]@{
      mode = 'fixed_runtime'
      relative_path = 'runtime/webview2'
      version = $runtimeVersion
      architecture = $runtimeArchitecture
      file_count = $runtimeFileCount
      bytes = $runtimeBytes
      archive_sha256 = $runtimeArchiveSha256
      executable_signature = [string]$runtimeSignature.Status
      signer = [string]$runtimeSignature.SignerCertificate.Subject
    }
    static_file_count = $staticFiles.Count
    static_file_bytes = $staticFileBytes
    files = $staticFiles
  }
  $manifestPath = Join-Path $stagingRoot 'release-manifest.json'
  Write-Utf8NoBom $manifestPath ($manifest | ConvertTo-Json -Depth 8)
  $manifestHash = Get-Sha256 $manifestPath
  Write-Utf8NoBom (Join-Path $stagingRoot 'release-manifest.sha256') "$manifestHash  release-manifest.json`n"

  Move-Item -LiteralPath $stagingRoot -Destination $finalRoot

  if (-not $SkipArchive) {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [System.IO.Compression.ZipFile]::CreateFromDirectory(
      $finalRoot,
      $archivePath,
      [System.IO.Compression.CompressionLevel]::Optimal,
      $true
    )
  }
} catch {
  if (Test-Path -LiteralPath $stagingRoot) {
    $fullStagingRoot = [System.IO.Path]::GetFullPath($stagingRoot)
    $fullOutputRoot = [System.IO.Path]::GetFullPath($outputRoot).TrimEnd('\') + '\'
    if ($fullStagingRoot.StartsWith($fullOutputRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
      Remove-Item -LiteralPath $fullStagingRoot -Recurse -Force
    }
  }
  throw
}

$result = [ordered]@{
  status = 'created'
  package = $finalRoot
  archive = if ($SkipArchive) { $null } else { $archivePath }
  version = $version
  webview2_version = $runtimeVersion
  manifest_sha256 = (Get-Content -Raw -LiteralPath (Join-Path $finalRoot 'release-manifest.sha256')).Split(' ')[0]
}
if (-not $SkipArchive) {
  $result.archive_sha256 = Get-Sha256 $archivePath
  $result.archive_bytes = (Get-Item -LiteralPath $archivePath).Length
}
$result | ConvertTo-Json -Compress
