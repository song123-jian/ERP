[CmdletBinding()]
param(
  [string]$OutputDirectory = ''
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
if ($PSVersionTable.PSEdition -eq 'Desktop') {
  $env:PSModulePath = Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\Modules'
}
Import-Module Microsoft.PowerShell.Security -ErrorAction Stop

$runtimeVersion = '153.0.4234.32'
$runtimeArchitecture = 'x64'
$runtimeFolderName = "Microsoft.WebView2.FixedVersionRuntime.$runtimeVersion.$runtimeArchitecture"
$runtimeFileCount = 257
$runtimeBytes = 699494316L
$archiveName = "$runtimeFolderName.cab"
$archiveSha256 = '2CB653A74426F0AA802C2396775C6BC674FD662D5396BD677F47BFA6E12EBA9C'
$downloadUrl = 'https://msedge.sf.dl.delivery.mp.microsoft.com/filestreamingservice/files/c3d95bc1-a0a7-4ca6-aaa1-fa0ac3dd1a37/Microsoft.WebView2.FixedVersionRuntime.153.0.4234.32.x64.cab'
$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path

if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
  $OutputDirectory = Join-Path $repoRoot "artifacts\webview2-runtime\$runtimeFolderName"
}
$destination = [System.IO.Path]::GetFullPath($OutputDirectory)
$runtimeExe = Join-Path $destination 'msedgewebview2.exe'

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

function Assert-Runtime([string]$Root) {
  $exe = Join-Path $Root 'msedgewebview2.exe'
  if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) {
    throw "WebView2 fixed runtime is incomplete: $exe is missing."
  }
  $version = (Get-Item -LiteralPath $exe).VersionInfo.ProductVersion
  if ($version -ne $runtimeVersion) {
    throw "WebView2 runtime version mismatch. Expected $runtimeVersion, found $version."
  }
  $runtimeFiles = @(Get-ChildItem -LiteralPath $Root -File -Recurse)
  $actualRuntimeBytes = ($runtimeFiles | Measure-Object Length -Sum).Sum
  if ($runtimeFiles.Count -ne $runtimeFileCount -or $actualRuntimeBytes -ne $runtimeBytes) {
    throw "WebView2 runtime content mismatch. Expected $runtimeFileCount files and $runtimeBytes bytes, found $($runtimeFiles.Count) files and $actualRuntimeBytes bytes."
  }
  $signature = Microsoft.PowerShell.Security\Get-AuthenticodeSignature -LiteralPath $exe
  if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'Microsoft Corporation') {
    throw "WebView2 runtime signature is not a valid Microsoft signature: $($signature.Status)."
  }
}

if (Test-Path -LiteralPath $destination) {
  Assert-Runtime $destination
  [ordered]@{
    status = 'reused'
    path = $destination
    version = $runtimeVersion
    architecture = $runtimeArchitecture
    file_count = $runtimeFileCount
    bytes = $runtimeBytes
    archive_sha256 = $archiveSha256
  } | ConvertTo-Json -Compress
  exit 0
}

$archivePath = Join-Path ([System.IO.Path]::GetTempPath()) $archiveName
if (Test-Path -LiteralPath $archivePath) {
  $actualArchiveHash = Get-Sha256 $archivePath
  if ($actualArchiveHash -ne $archiveSha256) {
    throw "Cached WebView2 archive checksum mismatch: $archivePath"
  }
} else {
  $partialPath = "$archivePath.$([guid]::NewGuid().ToString('N')).partial"
  try {
    Invoke-WebRequest -UseBasicParsing -Uri $downloadUrl -OutFile $partialPath -TimeoutSec 1800
    $actualArchiveHash = Get-Sha256 $partialPath
    if ($actualArchiveHash -ne $archiveSha256) {
      throw "Downloaded WebView2 archive checksum mismatch. Expected $archiveSha256, found $actualArchiveHash."
    }
    Move-Item -LiteralPath $partialPath -Destination $archivePath
  } finally {
    if (Test-Path -LiteralPath $partialPath) {
      Remove-Item -LiteralPath $partialPath -Force
    }
  }
}

$extractRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("erp-webview2-extract-" + [guid]::NewGuid().ToString('N'))
$destinationParent = Split-Path -Parent $destination
New-Item -ItemType Directory -Path $extractRoot | Out-Null
New-Item -ItemType Directory -Path $destinationParent -Force | Out-Null

try {
  & "$env:SystemRoot\System32\expand.exe" -F:* $archivePath $extractRoot | Out-Null
  if ($LASTEXITCODE -ne 0) {
    throw "expand.exe failed with exit code $LASTEXITCODE."
  }
  $expandedRuntime = Join-Path $extractRoot $runtimeFolderName
  Assert-Runtime $expandedRuntime
  Move-Item -LiteralPath $expandedRuntime -Destination $destination
} finally {
  if (Test-Path -LiteralPath $extractRoot) {
    $fullExtractRoot = [System.IO.Path]::GetFullPath($extractRoot)
    $fullTempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
    if ($fullExtractRoot.StartsWith($fullTempRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
      Remove-Item -LiteralPath $fullExtractRoot -Recurse -Force
    }
  }
}

Assert-Runtime $destination
[ordered]@{
  status = 'downloaded'
  path = $destination
  version = $runtimeVersion
  architecture = $runtimeArchitecture
  file_count = $runtimeFileCount
  bytes = $runtimeBytes
  archive_sha256 = $archiveSha256
  source = $downloadUrl
} | ConvertTo-Json -Compress
