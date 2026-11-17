import assert from 'node:assert/strict'
import { existsSync, readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { test } from 'node:test'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..')
const text = relative => readFileSync(path.join(root, relative), 'utf8')
const plugin = 'adapters/deepseek-harness/plugin/'

test('retired standalone UI and its generators/proxy/tests stay removed', () => {
  for (const relative of ['clients/flutter', 'scripts/generate-contract-client.mjs', 'tools/flutter_web_proxy.py', 'tests/browser/a5-flutter-web.mjs']) {
    assert.equal(existsSync(path.join(root, relative)), false, relative)
  }
})
test('release and regression entrypoints have no standalone-client route', () => {
  for (const relative of ['scripts/build-release.ps1', 'scripts/run-regression.ps1']) {
    assert.doesNotMatch(text(relative), /flutter|clients[\\/]flutter|a5-flutter|generate-contract-client/i, relative)
  }
})
test('DSH UI and its structured privacy dependency remain packaged', () => {
  const manifest = JSON.parse(text(plugin + 'package.json'))
  assert.equal(manifest.exports['./client'], './client.js')
  assert.ok(!manifest.files.some(file => file.includes('a0-probe')), 'no deprecated probe is packaged')
  assert.ok(!manifest.dsh.client.inject.includes('@deepseek-ai/dsh-client-ui-sidebar-right'), 'no old right-sidebar dependency')
  assert.doesNotMatch(text('Cargo.toml'), /zodo-a0-probe/)
  assert.doesNotMatch(text('scripts/build-release.ps1'), /zodo-a0-probe/)
  for (const relative of ['client.js', 'index.js', 'privacy.js']) {
    assert.ok(manifest.files.includes(relative), relative + ' is listed for packaging')
    assert.ok(existsSync(path.join(root, plugin, relative)))
    assert.ok(text('scripts/build-release.ps1').includes("'" + relative + "'"), relative + ' is staged')
  }
  assert.ok(text(plugin + 'index.js').includes("from './privacy.js'"))
})
test('non-UI Core kernel runtime and native ABI remain available', () => {
  for (const relative of ['crates/zodo-kernel/Cargo.toml', 'crates/zodo-runtime/Cargo.toml', 'crates/zodo-ffi/Cargo.toml', 'contracts/client-contract.json']) {
    assert.ok(existsSync(path.join(root, relative)), relative)
  }
})
