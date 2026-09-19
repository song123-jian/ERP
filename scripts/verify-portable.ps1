[CmdletBinding()]
param(
  [string]$PackagePath = '',
  [string]$ArchivePath = '',
  [switch]$LaunchCheck,
  [int]$ReadyTimeoutSeconds = 30
)

$ErrorActionPreference = 'Stop'
if ($PSVersionTable.PSEdition -eq 'Desktop') {
  $env:PSModulePath = Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\Modules'
}
Import-Module Microsoft.PowerShell.Security -ErrorAction Stop
$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$packageJson = Get-Content -Raw -LiteralPath (Join-Path $repoRoot 'package.json') | ConvertFrom-Json
$expectedVersion = [string]$packageJson.version
$expectedPackageName = "plastic-sales-erp-$expectedVersion-windows-x64-portable"
$expectedRuntimeVersion = '153.0.4234.32'
$expectedRuntimeFileCount = 257
$expectedRuntimeBytes = 699494316L

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

function Get-SafePackagePath([string]$Root, [string]$RelativePath) {
  if ([string]::IsNullOrWhiteSpace($RelativePath) -or [System.IO.Path]::IsPathRooted($RelativePath)) {
    throw "Package path must be relative: $RelativePath"
  }
  $rootPrefix = [System.IO.Path]::GetFullPath($Root).TrimEnd('\') + '\'
  $fullPath = [System.IO.Path]::GetFullPath((Join-Path $Root $RelativePath.Replace('/', '\')))
  if (-not $fullPath.StartsWith($rootPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Package path escapes the package root: $RelativePath"
  }
  return $fullPath
}
if ([string]::IsNullOrWhiteSpace($PackagePath)) {
  $PackagePath = Join-Path $repoRoot "artifacts\portable\$expectedPackageName"
}
if ([string]::IsNullOrWhiteSpace($ArchivePath)) {
  $candidateArchive = "$PackagePath.zip"
  if (Test-Path -LiteralPath $candidateArchive -PathType Leaf) {
    $ArchivePath = $candidateArchive
  }
}

$packageRoot = (Resolve-Path -LiteralPath $PackagePath).Path
$manifestPath = Join-Path $packageRoot 'release-manifest.json'
$manifestHashPath = Join-Path $packageRoot 'release-manifest.sha256'
$manifest = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json
$expectedManifestHash = ((Get-Content -Raw -LiteralPath $manifestHashPath).Trim() -split '\s+')[0]
$actualManifestHash = Get-Sha256 $manifestPath
if ($actualManifestHash -ne $expectedManifestHash) {
  throw "Release manifest checksum mismatch. Expected $expectedManifestHash, found $actualManifestHash."
}
if ([string]$manifest.version -ne $expectedVersion) {
  throw "Package version mismatch. Expected $expectedVersion, found $($manifest.version)."
}

foreach ($directory in @($manifest.required_directories)) {
  $directoryPath = Get-SafePackagePath $packageRoot ([string]$directory)
  if (-not (Test-Path -LiteralPath $directoryPath -PathType Container)) {
    throw "Required portable directory is missing: $directory"
  }
  if ((Get-ChildItem -LiteralPath $directoryPath -Force | Measure-Object).Count -ne 0) {
    throw "Required portable directory is not empty: $directory"
  }
}

$verifiedBytes = 0L
foreach ($file in @($manifest.files)) {
  $filePath = Get-SafePackagePath $packageRoot ([string]$file.path)
  if (-not (Test-Path -LiteralPath $filePath -PathType Leaf)) {
    throw "Manifest file is missing: $($file.path)"
  }
  $item = Get-Item -LiteralPath $filePath
  if ($item.Length -ne [long]$file.size) {
    throw "Manifest size mismatch: $($file.path)"
  }
  $hash = Get-Sha256 $filePath
  if ($hash -ne [string]$file.sha256) {
    throw "Manifest checksum mismatch: $($file.path)"
  }
  $verifiedBytes += $item.Length
}
if (@($manifest.files).Count -ne [int]$manifest.static_file_count -or $verifiedBytes -ne [long]$manifest.static_file_bytes) {
  throw 'Manifest file count or byte total does not match the verified files.'
}

$manifestPaths = @{}
foreach ($file in @($manifest.files)) {
  $manifestPaths[[string]$file.path] = $true
}
$unexpectedFiles = @(
  Get-ChildItem -LiteralPath $packageRoot -File -Recurse |
    ForEach-Object {
      $relative = $_.FullName.Substring($packageRoot.TrimEnd('\').Length + 1).Replace('\', '/')
      if (-not $manifestPaths.ContainsKey($relative) -and $relative -notin @('release-manifest.json', 'release-manifest.sha256')) {
        $relative
      }
    }
)
if ($unexpectedFiles.Count -gt 0) {
  throw "Package contains files outside the release manifest: $($unexpectedFiles -join ', ')"
}

$exePath = Get-SafePackagePath $packageRoot ([string]$manifest.entrypoint)
$exeVersion = (Get-Item -LiteralPath $exePath).VersionInfo.FileVersion
if ($exeVersion -ne $expectedVersion) {
  throw "Executable version mismatch. Expected $expectedVersion, found $exeVersion."
}
$runtimeRoot = Get-SafePackagePath $packageRoot ([string]$manifest.webview2.relative_path)
$runtimeExe = Join-Path $runtimeRoot 'msedgewebview2.exe'
$runtimeVersion = (Get-Item -LiteralPath $runtimeExe).VersionInfo.ProductVersion
if ($runtimeVersion -ne $expectedRuntimeVersion -or [string]$manifest.webview2.version -ne $expectedRuntimeVersion) {
  throw "WebView2 runtime version mismatch. Expected $expectedRuntimeVersion, found $runtimeVersion."
}
$runtimeFiles = @(Get-ChildItem -LiteralPath $runtimeRoot -File -Recurse)
$actualRuntimeBytes = ($runtimeFiles | Measure-Object Length -Sum).Sum
if ($runtimeFiles.Count -ne $expectedRuntimeFileCount -or $actualRuntimeBytes -ne $expectedRuntimeBytes) {
  throw "WebView2 runtime content mismatch. Expected $expectedRuntimeFileCount files and $expectedRuntimeBytes bytes, found $($runtimeFiles.Count) files and $actualRuntimeBytes bytes."
}
if ([int]$manifest.webview2.file_count -ne $expectedRuntimeFileCount -or [long]$manifest.webview2.bytes -ne $expectedRuntimeBytes) {
  throw 'WebView2 runtime totals in the release manifest do not match the pinned runtime.'
}
$runtimeSignature = Microsoft.PowerShell.Security\Get-AuthenticodeSignature -LiteralPath $runtimeExe
if ($runtimeSignature.Status -ne 'Valid' -or $runtimeSignature.SignerCertificate.Subject -notmatch 'Microsoft Corporation') {
  throw "WebView2 runtime signature is not a valid Microsoft signature: $($runtimeSignature.Status)."
}

$archiveVerified = $false
if (-not [string]::IsNullOrWhiteSpace($ArchivePath)) {
  $resolvedArchive = (Resolve-Path -LiteralPath $ArchivePath).Path
  Add-Type -AssemblyName System.IO.Compression.FileSystem
  $archive = [System.IO.Compression.ZipFile]::OpenRead($resolvedArchive)
  try {
    $basePath = (Split-Path -Leaf $packageRoot) + '/'
    $entryMap = @{}
    foreach ($entry in $archive.Entries) {
      $normalizedEntryName = $entry.FullName.Replace('\', '/')
      if ($entryMap.ContainsKey($normalizedEntryName)) {
        throw "Archive contains a duplicate entry: $normalizedEntryName"
      }
      $entryMap[$normalizedEntryName] = $entry
    }
    foreach ($file in @($manifest.files)) {
      $entryName = $basePath + [string]$file.path
      if (-not $entryMap.ContainsKey($entryName)) {
        throw "Archive entry is missing: $entryName"
      }
      $entry = $entryMap[$entryName]
      if ($entry.Length -ne [long]$file.size) {
        throw "Archive entry size mismatch: $entryName"
      }
      $stream = $entry.Open()
      try {
        $sha = [System.Security.Cryptography.SHA256]::Create()
        try {
          $hashBytes = $sha.ComputeHash($stream)
        } finally {
          $sha.Dispose()
        }
      } finally {
        $stream.Dispose()
      }
      $entryHash = ([System.BitConverter]::ToString($hashBytes)).Replace('-', '')
      if ($entryHash -ne [string]$file.sha256) {
        throw "Archive entry checksum mismatch: $entryName"
      }
    }
    $requiredArchiveFiles = [ordered]@{
      'release-manifest.json' = $actualManifestHash
      'release-manifest.sha256' = Get-Sha256 $manifestHashPath
    }
    foreach ($requiredFile in $requiredArchiveFiles.Keys) {
      $requiredEntryName = $basePath + $requiredFile
      if (-not $entryMap.ContainsKey($requiredEntryName)) {
        throw "Archive entry is missing: $requiredEntryName"
      }
      $requiredEntryStream = $entryMap[$requiredEntryName].Open()
      try {
        $requiredSha = [System.Security.Cryptography.SHA256]::Create()
        try {
          $requiredHashBytes = $requiredSha.ComputeHash($requiredEntryStream)
        } finally {
          $requiredSha.Dispose()
        }
      } finally {
        $requiredEntryStream.Dispose()
      }
      $requiredEntryHash = ([System.BitConverter]::ToString($requiredHashBytes)).Replace('-', '')
      if ($requiredEntryHash -ne $requiredArchiveFiles[$requiredFile]) {
        throw "Archive entry checksum mismatch: $requiredEntryName"
      }
    }
    foreach ($directory in @($manifest.required_directories)) {
      $directoryPrefix = $basePath + [string]$directory + '/'
      if (-not @($archive.Entries | Where-Object { $_.FullName.Replace('\', '/') -eq $directoryPrefix }).Count) {
        throw "Archive does not preserve the empty directory: $directory"
      }
    }
    $archiveVerified = $true
  } finally {
    $archive.Dispose()
  }
}

$launchResult = $null
if ($LaunchCheck) {
  $existingApp = @(Get-Process -Name 'plastic-sales-erp' -ErrorAction SilentlyContinue)
  if ($existingApp.Count -gt 0) {
    throw 'Launch verification requires all existing plastic-sales-erp processes to be closed.'
  }

  $smokeRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("erp-portable-smoke-" + [guid]::NewGuid().ToString('N'))
  $process = $null
  $observedRuntimeProcesses = @()
  New-Item -ItemType Directory -Path $smokeRoot | Out-Null
  try {
    Copy-Item -LiteralPath $exePath -Destination (Join-Path $smokeRoot 'plastic-sales-erp.exe')
    New-Item -ItemType Directory -Path (Join-Path $smokeRoot 'runtime') | Out-Null
    New-Item -ItemType Junction -Path (Join-Path $smokeRoot 'runtime\webview2') -Target $runtimeRoot | Out-Null
    $smokeExe = Join-Path $smokeRoot 'plastic-sales-erp.exe'
    $process = Start-Process -FilePath $smokeExe -WorkingDirectory $smokeRoot -WindowStyle Hidden -PassThru
    $readyPath = Join-Path $smokeRoot 'state\ui_ready.json'
    $deadline = [DateTime]::UtcNow.AddSeconds($ReadyTimeoutSeconds)
    $readyPayload = $null
    while ([DateTime]::UtcNow -lt $deadline) {
      if ($process.HasExited) {
        throw "Portable application exited before UI ready; exit code $($process.ExitCode)."
      }
      if (Test-Path -LiteralPath $readyPath) {
        $readyPayload = Get-Content -Raw -LiteralPath $readyPath | ConvertFrom-Json
      }
      $observedRuntimeProcesses = @(
        Get-CimInstance Win32_Process -Filter "Name = 'msedgewebview2.exe'" |
          Where-Object {
            $_.ExecutablePath -and
            [System.IO.Path]::GetFullPath($_.ExecutablePath).StartsWith(
              [System.IO.Path]::GetFullPath($runtimeRoot).TrimEnd('\') + '\',
              [System.StringComparison]::OrdinalIgnoreCase
            )
          }
      )
      if ($readyPayload.ready_at -and $observedRuntimeProcesses.Count -gt 0) {
        break
      }
      Start-Sleep -Milliseconds 100
    }
    if (-not $readyPayload.ready_at) {
      throw "UI ready signal was not received within ${ReadyTimeoutSeconds}s."
    }
    if ([string]$readyPayload.app_version -ne $expectedVersion) {
      throw "UI ready version mismatch. Expected $expectedVersion, found $($readyPayload.app_version)."
    }
    if ($observedRuntimeProcesses.Count -eq 0) {
      throw 'No WebView2 process was observed from the bundled fixed runtime.'
    }
    $launchResult = [ordered]@{
      ready = $true
      app_version = [string]$readyPayload.app_version
      runtime_process_count = $observedRuntimeProcesses.Count
      runtime_executable = [string]$observedRuntimeProcesses[0].ExecutablePath
    }
  } finally {
    if ($process -and -not $process.HasExited) {
      Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
      [void]$process.WaitForExit(5000)
    }
    foreach ($runtimeProcess in $observedRuntimeProcesses) {
      Stop-Process -Id $runtimeProcess.ProcessId -Force -ErrorAction SilentlyContinue
    }
    if (Test-Path -LiteralPath $smokeRoot) {
      $fullSmokeRoot = [System.IO.Path]::GetFullPath($smokeRoot)
      $fullTempRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
      if ($fullSmokeRoot.StartsWith($fullTempRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
        Remove-Item -LiteralPath $fullSmokeRoot -Recurse -Force
      }
    }
  }
}

[ordered]@{
  status = 'passed'
  package = $packageRoot
  version = $expectedVersion
  manifest_sha256 = $actualManifestHash
  static_file_count = [int]$manifest.static_file_count
  static_file_bytes = $verifiedBytes
  webview2_version = $runtimeVersion
  webview2_signature = [string]$runtimeSignature.Status
  archive_verified = $archiveVerified
  launch = $launchResult
} | ConvertTo-Json -Depth 5 -Compress
