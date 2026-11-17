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

// These are settings-boundary checks, not sidebar/slot registration constraints.
const viewSource = name => {
  const client = text(plugin + 'client.js')
  const start = client.indexOf('    function ' + name + '(')
  assert.ok(start >= 0, name + ' is an independent view')
  const end = client.indexOf('\n    function ', start + 1)
  return client.slice(start, end < 0 ? undefined : end)
}

test('model and privacy navigation render independent settings views', () => {
  const client = text(plugin + 'client.js')
  assert.match(client, /case 'models': return h\(ModelsView,/)
  assert.match(client, /case 'privacy': return h\(PrivacyView,/)
  const models = viewSource('ModelsView')
  const privacy = viewSource('PrivacyView')
  assert.match(models, /h\('h1', null, '模型'\)/)
  assert.match(privacy, /h\('h1', null, '隐私'\)/)
  assert.doesNotMatch(privacy, /h\(ModelsView\b/, 'privacy must not alias the model editor')
  assert.doesNotMatch(models, /workspace\.update|privacy-save/, 'saving a model is not a policy command')
  assert.doesNotMatch(privacy, /updateModel\s*\(|model-save|model-source-/, 'privacy cannot rewrite model configuration')
})

test('model source editor has only explicit DSH/custom choices and no legacy routing form', () => {
  const models = viewSource('ModelsView')
  assert.match(models, /kind: 'dsh', provider: null, model: null/)
  assert.match(models, /kind: 'custom'/)
  assert.match(models, /version: 2, source/)
  for (const selector of ['model-dsh-select', 'model-save', 'model-refresh', 'model-discard', 'model-stale', 'model-effective-status']) {
    assert.ok(models.includes(selector), selector)
  }
  for (const label of ['API 地址', '模型 ID', '密钥环境变量名（可选）']) assert.ok(models.includes(label), label)
  assert.doesNotMatch(models, /config\.editable|local_only|cloud_assisted|allowed_cloud_providers|ai-mode-|ai-cloud-consent|目标拆解路由|收集澄清路由/)
  assert.doesNotMatch(models, /type: ['"]password['"]|currentSelection|agentDefaultModel/, 'DSH choice neither asks for a secret nor inherits chat selection')
})

test('privacy editor exposes independent context switches and diagnostic control', () => {
  const privacy = viewSource('PrivacyView')
  for (const field of ['goals', 'projects', 'captures']) assert.ok(privacy.includes(field), field)
  for (const field of ['model_access', 'diagnostic_capture', 'privacy-save', 'privacy-advanced']) assert.ok(privacy.includes(field), field)
  assert.doesNotMatch(privacy, /model_route|allowed_cloud_providers|private_notes|local_only|cloud_assisted/)
})
