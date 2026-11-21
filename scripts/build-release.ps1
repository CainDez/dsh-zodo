param()

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$release = Join-Path $root 'artifacts/release'
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
  throw 'Release packaging requires Windows and the x86_64-pc-windows-msvc Rust target/toolchain.'
}
$cargoTarget = 'x86_64-pc-windows-msvc'
# A fresh owned directory prevents stale binaries and ignores inherited target-dir defaults.
# Cargo's explicit --target also overrides build.target/CARGO_BUILD_TARGET selection.
$buildDirectory = Join-Path ([IO.Path]::GetTempPath()) ('zodo-release-build-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $buildDirectory | Out-Null
$cargoOutput = Join-Path $buildDirectory "$cargoTarget/release"
Push-Location $root
try {
  Write-Host 'Building Rust Core and non-UI native ABI...'
  $savedPreference = $ErrorActionPreference
  try {
    # Cargo writes progress to stderr; Windows PowerShell 5 must not mistake that for failure.
    $ErrorActionPreference = 'Continue'
    cargo build --release --workspace --locked --target $cargoTarget --target-dir $buildDirectory 2>&1 | Out-Host
    $buildCode = $LASTEXITCODE
  } finally { $ErrorActionPreference = $savedPreference }
  if ($buildCode -ne 0) { throw 'cargo build failed' }
  $core = Join-Path $cargoOutput 'zodo.exe'
  $ffi = Join-Path $cargoOutput 'zodo_ffi.dll'
  foreach ($file in @($core, $ffi)) {
    if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { throw "Required binary missing: $file" }
  }

  # Replace only the repository's generated release output, never installed plugins or live data.
  if (Test-Path -LiteralPath $release) {
    if ((Get-Item -LiteralPath $release).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Refusing linked release output' }
    Remove-Item -LiteralPath $release -Recurse -Force
  }
  New-Item -ItemType Directory -Path $release | Out-Null
  Copy-Item -LiteralPath $core -Destination (Join-Path $release 'zodo.exe')
  Copy-Item -LiteralPath $ffi -Destination (Join-Path $release 'zodo_ffi.dll')
  $plugin = Join-Path $root 'adapters/deepseek-harness/plugin'
  $manifest = Get-Content -LiteralPath (Join-Path $plugin 'package.json') -Raw | ConvertFrom-Json
  $pluginName = "zodo-harness-plugin-$($manifest.version).zip"
  $stage = Join-Path $release '.plugin-stage'
  New-Item -ItemType Directory -Force -Path (Join-Path $stage 'bin/win32-x64') | Out-Null
  foreach ($file in @('package.json', 'cordis.patch.yml', 'client.js', 'index.js', 'privacy.js', 'host-gateway.js', 'icon.svg', 'README.md')) {
    Copy-Item -LiteralPath (Join-Path $plugin $file) -Destination (Join-Path $stage $file)
  }
  $locales = @(Get-ChildItem -LiteralPath (Join-Path $plugin 'locale') -Filter '*.json' -File)
  if ($locales.Count -eq 0) { throw 'No locale JSON resources found' }
  $localeStage = Join-Path $stage 'locale'
  New-Item -ItemType Directory -Path $localeStage | Out-Null
  foreach ($locale in $locales) {
    Copy-Item -LiteralPath $locale.FullName -Destination $localeStage
  }
  Copy-Item -LiteralPath $core -Destination (Join-Path $stage 'bin/win32-x64/zodo-core.exe')
  Compress-Archive -Path (Join-Path $stage '*') -DestinationPath (Join-Path $release $pluginName)
  Remove-Item -LiteralPath $stage -Recurse -Force

  $rustc = (& rustc --version).Trim()
  if ($LASTEXITCODE -ne 0) { throw 'rustc version failed' }
  $commit = (& git rev-parse HEAD).Trim()
  if ($LASTEXITCODE -ne 0) { throw 'git revision failed' }
  $dirty = [bool](& git status --porcelain --untracked-files=normal)
  if ($LASTEXITCODE -ne 0) { throw 'git status failed' }
  [ordered]@{
    schema = 'zodo.release/1'
    built_at = (Get-Date).ToString('o')
    commit = $commit
    working_tree_dirty = $dirty
    rustc = $rustc
    cargo_target = $cargoTarget
    cargo_profile = 'release'
    ui = 'deepseek-harness-plugin-only'
    artifacts = @('zodo.exe', 'zodo_ffi.dll', $pluginName)
    note = 'Build checkpoint only, not exhaustive acceptance. No standalone UI client is shipped.'
  } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $release 'BUILD_INFO.json') -Encoding UTF8
  Get-ChildItem -LiteralPath $release -File | Where-Object { $_.Name -ne 'SHA256SUMS.txt' } | Sort-Object Name | ForEach-Object {
    $hash = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  $($_.Name)"
  } | Set-Content -LiteralPath (Join-Path $release 'SHA256SUMS.txt') -Encoding ASCII
  Write-Host "Core and DSH-only release written to $release"
} finally {
  Pop-Location
  # Remove only this invocation's owned build tree; never the caller's Cargo target.
  Remove-Item -LiteralPath $buildDirectory -Recurse -Force
}
