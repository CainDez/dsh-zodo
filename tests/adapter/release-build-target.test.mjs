import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'

// Static assembly guards only. These do not execute PowerShell/Cargo, produce a
// Windows package, or establish installed DSH compatibility.
const script = readFileSync(new URL('../../scripts/build-release.ps1', import.meta.url), 'utf8')

test('release build explicitly selects the Windows x64 MSVC target', () => {
  assert.match(script, /\$cargoTarget = 'x86_64-pc-windows-msvc'/)
  assert.match(script, /OSVersion\.Platform -ne \[PlatformID\]::Win32NT/)
  const builds = script.split('\n').filter(line => /^\s*cargo build /.test(line))
  assert.equal(builds.length, 1)
  assert.match(builds[0], /--release --workspace --locked --target \$cargoTarget --target-dir \$buildDirectory/)
  assert.ok(script.indexOf("throw 'cargo build failed'") < script.indexOf('$core = '))
})

test('each package build uses a fresh owned target rather than inherited or stale output', () => {
  assert.match(script, /\$buildDirectory = Join-Path \(\[IO.Path\]::GetTempPath\(\)\) \('zodo-release-build-' \+ \[Guid\]::NewGuid\(\)\.ToString\('N'\)\)/)
  assert.match(script, /New-Item -ItemType Directory -Path \$buildDirectory/)
  assert.match(script, /\$cargoOutput = Join-Path \$buildDirectory "\$cargoTarget\/release"/)
  assert.doesNotMatch(script, /\$env:CARGO_(?:TARGET_DIR|BUILD_TARGET)\s*=/i, 'caller environment is not mutated')
  assert.doesNotMatch(script, /Join-Path \$root 'target\/release\//)
})

test('both release binaries and embedded Core originate solely in the selected build', () => {
  assert.match(script, /\$core = Join-Path \$cargoOutput 'zodo.exe'/)
  assert.match(script, /\$ffi = Join-Path \$cargoOutput 'zodo_ffi.dll'/)
  assert.equal((script.match(/\$core\s*=/g) ?? []).length, 1)
  assert.equal((script.match(/\$ffi\s*=/g) ?? []).length, 1)
  assert.match(script, /Copy-Item -LiteralPath \$core -Destination \(Join-Path \$release 'zodo.exe'\)/)
  assert.match(script, /Copy-Item -LiteralPath \$ffi -Destination \(Join-Path \$release 'zodo_ffi.dll'\)/)
  assert.match(script, /Copy-Item -LiteralPath \$core -Destination \(Join-Path \$stage 'bin\/win32-x64\/zodo-core.exe'\)/)
})

test('metadata records target and cleanup only removes the owned build tree', () => {
  assert.match(script, /cargo_target = \$cargoTarget/)
  assert.match(script, /cargo_profile = 'release'/)
  assert.match(script, /finally \{\s*Pop-Location[\s\S]*Remove-Item -LiteralPath \$buildDirectory -Recurse -Force\s*\}/)
  assert.doesNotMatch(script, /Remove-Item[^\n]*\$env:CARGO_TARGET_DIR/)
})

test('only manifest-matched locale JSON resources are staged', () => {
  assert.match(script, /Get-ChildItem -LiteralPath \(Join-Path \$plugin 'locale'\) -Filter '\*\.json' -File/)
  assert.doesNotMatch(script, /Copy-Item[^\n]*\$plugin 'locale'[^\n]*-Recurse/)
})
