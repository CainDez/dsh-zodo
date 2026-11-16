param(
  [switch]$WithBrowser,
  [switch]$WithIntegration,
  [string]$Node = 'node',
  [string]$Python = 'python'
)

# All mutating suites create their own temporary Core; never read a2-url.txt or live data.
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$results = [System.Collections.Generic.List[object]]::new()
$stamp = Get-Date -Format 'yyyyMMdd-HHmmss-fff'
$out = Join-Path $root "artifacts/evidence/regression-$stamp"
New-Item -ItemType Directory -Force -Path $out | Out-Null

function Invoke-Check {
  param([string]$Name, [string]$Executable, [string[]]$Arguments)
  Write-Host "== $Name =="
  $log = Join-Path $out ("{0:D2}.log" -f $results.Count)
  $code = 1
  try {
    $command = Get-Command $Executable -ErrorAction Stop
    # Windows PowerShell 5 treats native stderr as ErrorRecord; use the exit code, not log channel.
    $savedPreference = $ErrorActionPreference
    try {
      $ErrorActionPreference = 'Continue'
      & $command @Arguments 2>&1 | Tee-Object -FilePath $log | Out-Host
      $code = $LASTEXITCODE
    } finally { $ErrorActionPreference = $savedPreference }
    if ($null -eq $code) { $code = 1 }
  } catch {
    $_.ToString() | Tee-Object -FilePath $log -Append | Out-Host
  }
  $results.Add([pscustomobject]@{ name = $Name; exit_code = $code; passed = ($code -eq 0); log = $log })
  return ($code -eq 0)
}

Push-Location $root
try {
  $null = Invoke-Check 'Rust format' 'cargo' @('fmt', '--all', '--', '--check')
  $null = Invoke-Check 'Rust workspace' 'cargo' @('test', '--workspace', '--locked')
  foreach ($file in @('index.js', 'client.js', 'privacy.js')) {
    $null = Invoke-Check "Syntax $file" $Node @('--check', "adapters/deepseek-harness/plugin/$file")
  }
  $null = Invoke-Check 'Privacy and UI scope adapters' $Node @('--test', 'tests/adapter/privacy.test.mjs', 'tests/adapter/ui-scope.test.mjs')
  $null = Invoke-Check 'Sidebar lifecycle registration' $Node @('tests/browser/zodo-sidebar-registration.mjs')

  if ($WithIntegration -or $WithBrowser) {
    $built = Invoke-Check 'Isolated suites Core build' 'cargo' @('build', '-p', 'zodo-runtime', '--release', '--locked')
    if ($built) {
      if ($WithIntegration) {
        $null = Invoke-Check 'Integration runner safety guards' $Python @('-m', 'unittest', 'discover', '-s', 'tests/integration', '-p', 'test_runner_safety.py', '-v')
        $null = Invoke-Check 'Isolated API integration' $Python @('tools/run_isolated_integration.py')
      }
      if ($WithBrowser) {
        $null = Invoke-Check 'Isolated workbench UI' $Node @('tests/browser/harness-isolated-regression.mjs')
      }
    }
  }
} finally {
  Pop-Location
  [pscustomobject]@{ generated_at = (Get-Date -Format o); results = @($results.ToArray()) } |
    ConvertTo-Json -Depth 5 | Set-Content -Encoding UTF8 (Join-Path $out 'summary.json')
}
$failed = @($results | Where-Object { -not $_.passed })
Write-Host "Evidence: $out"
if ($failed.Count -gt 0) {
  Write-Host ('REGRESSION FAILURES: ' + (($failed | ForEach-Object { $_.name }) -join ', ')) -ForegroundColor Red
  exit 1
}
Write-Host 'Regression checks passed. See individual suite reports for scope and skips.' -ForegroundColor Green
