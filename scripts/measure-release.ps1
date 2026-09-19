param(
  [string]$ExecutablePath = (Join-Path (Get-Location) 'src-tauri\target\release\plastic-sales-erp.exe'),
  [int]$StartupTargetMs = 3000,
  [int]$MemoryTargetMb = 200,
  [int]$ReadyTimeoutSeconds = 30
)

$ErrorActionPreference = 'Stop'
$sourceExe = (Resolve-Path -LiteralPath $ExecutablePath).Path
$runRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("erp-perf-" + [guid]::NewGuid().ToString('N'))
$runExe = Join-Path $runRoot 'plastic-sales-erp.exe'
$process = $null
$result = [ordered]@{
  executable = [System.IO.Path]::GetFileName($sourceExe)
  measured_at = (Get-Date).ToUniversalTime().ToString('o')
  dataset_rows = 10000
  startup_target_ms = $StartupTargetMs
  memory_target_mb = $MemoryTargetMb
  ready_signal = 'state/ui_ready.json'
  ready = $false
  app_version = $null
  startup_ms = $null
  idle_working_set_mb = $null
  peak_working_set_mb = $null
  status = 'failed'
  error = $null
}

try {
  New-Item -ItemType Directory -Path $runRoot -Force | Out-Null
  Copy-Item -LiteralPath $sourceExe -Destination $runExe -Force

  $clock = [System.Diagnostics.Stopwatch]::StartNew()
  $process = Start-Process -FilePath $runExe -WorkingDirectory $runRoot -PassThru
  $readyPath = Join-Path $runRoot 'state\ui_ready.json'
  $deadline = [DateTime]::UtcNow.AddSeconds($ReadyTimeoutSeconds)
  while ([DateTime]::UtcNow -lt $deadline) {
    if ($process.HasExited) {
      throw "release process exited before UI ready; exit_code=$($process.ExitCode)"
    }
    if (Test-Path -LiteralPath $readyPath) {
      $readyPayload = Get-Content -Raw -LiteralPath $readyPath | ConvertFrom-Json
      if ($readyPayload.ready_at) {
        $result.ready = $true
        $result.app_version = [string]$readyPayload.app_version
        $result.startup_ms = [math]::Round($clock.Elapsed.TotalMilliseconds, 3)
        break
      }
    }
    Start-Sleep -Milliseconds 50
  }
  if (-not $result.ready) {
    throw "UI ready signal was not received within ${ReadyTimeoutSeconds}s"
  }

  $workingSetSamples = New-Object System.Collections.Generic.List[double]
  for ($sample = 0; $sample -lt 20; $sample++) {
    if ($process.HasExited) { throw 'release process exited during memory sampling' }
    $process.Refresh()
    $workingSetSamples.Add($process.WorkingSet64 / 1MB)
    Start-Sleep -Milliseconds 100
  }
  $result.idle_working_set_mb = [math]::Round(($workingSetSamples | Measure-Object -Average).Average, 3)
  $result.peak_working_set_mb = [math]::Round(($workingSetSamples | Measure-Object -Maximum).Maximum, 3)
  $result.status = if ($result.startup_ms -le $StartupTargetMs -and $result.peak_working_set_mb -le $MemoryTargetMb) { 'passed' } else { 'failed' }
}
catch {
  $result.error = $_.Exception.Message
}
finally {
  if ($process -and -not $process.HasExited) {
    Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
    [void]$process.WaitForExit(5000)
  }
  if (Test-Path -LiteralPath $runRoot) {
    Remove-Item -LiteralPath $runRoot -Recurse -Force -ErrorAction SilentlyContinue
  }
}

$result | ConvertTo-Json -Compress
if ($result.status -ne 'passed') { exit 1 }
