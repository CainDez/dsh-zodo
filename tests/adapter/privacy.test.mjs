import assert from 'node:assert/strict'
import { test } from 'node:test'
import { containsDeviceOnlyData, assertHostExportAllowed, privacyProjection, hasRestrictedExport, sessionHasRestrictedExport } from '../../adapters/deepseek-harness/plugin/privacy.js'

const call = (callId = 'c1', name = 'zodo_list_tasks') => ({ type: 'tool/call', data: { callId, name } })
const result = (value, callId = 'c1') => ({ type: 'tool/result', data: { callId, value } })
const secret = { nested: [{ data_class: 'device_only', body: 'PRIVATE_SENTINEL' }] }
const restricted = events => hasRestrictedExport(events)

test('nested arrays and objects detect structured device-only classification', () => {
  assert.equal(containsDeviceOnlyData(secret), true)
  for (const value of [null, undefined, false, 1, 'device_only', { text: 'data_class: device_only' }, { data_class: 'cloud_allowed' }]) {
    assert.equal(containsDeviceOnlyData(value), false)
  }
})
test('export guard preserves public payload identity', () => {
  const publicValue = { tasks: [{ title: 'public', data_class: 'public' }] }
  assert.equal(assertHostExportAllowed(publicValue), publicValue)
})
test('export rejection never echoes sensitive values', () => {
  assert.throws(() => assertHostExportAllowed(secret), error => {
    assert.equal(error.code, 'ZODO_DEVICE_ONLY_EXPORT_DENIED')
    assert.doesNotMatch(error.message, /PRIVATE_SENTINEL|nested/)
    return true
  })
})
test('native results are restricted only when correlated to Zodo tools', () => {
  assert.equal(restricted([call(), result(secret)]), true)
  assert.equal(restricted([result(secret)]), false)
  assert.equal(restricted([call('c1', 'unrelated_tool'), result(secret)]), false)
})
test('all six registered Zodo tool names participate', () => {
  for (const name of ['zodo_capture', 'zodo_list_tasks', 'zodo_propose', 'zodo_plan_propose', 'zodo_replan', 'zodo_get_proposal']) {
    assert.equal(restricted([call('c1', name), result(secret)]), true, name)
  }
})
test('reused provider call ID belongs to its latest tool', () => {
  assert.equal(restricted([call(), call('c1', 'unrelated_tool'), result(secret)]), false)
  assert.equal(restricted([call('c1', 'unrelated_tool'), call(), result(secret)]), true)
})
test('public native result consumes pending correlation', () => {
  const state = [call(), result({ tasks: [] })].reduce(privacyProjection.apply, privacyProjection.init())
  assert.deepEqual(state, { restricted: false, calls: [] })
  assert.equal(privacyProjection.apply(state, result(secret)).restricted, false)
})
test('native message source and toolCallId shapes are supported', () => {
  for (const message of [
    { source: { callId: 'c1' }, content: [{ type: 'text', text: JSON.stringify(secret) }] },
    { toolCallId: 'c1', content: [{ type: 'text', text: JSON.stringify(secret) }] },
  ]) assert.equal(restricted([call(), { type: 'tool/result', data: { message } }]), true)
})
test('PTC results do not need native call correlation', () => {
  assert.equal(restricted([{ type: 'tool/ptc-dispatch', data: { name: 'zodo_capture', value: secret } }]), true)
  assert.equal(restricted([{ type: 'tool/ptc-dispatch', data: { name: 'unrelated_tool', value: secret } }]), false)
})
test('opaque restricted history markers survive without bodies', () => {
  assert.equal(restricted([call(), result({ restricted_history_marker: 'opaque' })]), true)
  assert.equal(restricted([call(), result({ restricted_history_marker: '' })]), false)
})
test('plain user/assistant text cannot forge structured restriction', () => {
  const data = { content: [{ type: 'text', text: JSON.stringify(secret) }] }
  assert.equal(restricted([{ type: 'message', data }, { type: 'user/message', data }]), false)
  assert.equal(restricted([call(), result({ text: JSON.stringify(secret) })]), false)
})
test('malformed result text is ignored without throwing', () => {
  assert.equal(restricted([call(), { type: 'tool/result', data: { callId: 'c1', content: [{ type: 'text', text: '{broken' }] } }]), false)
  assert.equal(restricted([{ type: 'other' }, null]), false)
})
test('restriction is monotonic and clears correlation metadata', () => {
  const state = [call(), result(secret)].reduce(privacyProjection.apply, privacyProjection.init())
  assert.deepEqual(state, { restricted: true, calls: [] })
  assert.equal(privacyProjection.apply(state, call()), state)
})
test('checkpoint parser validates shape and returns independent call arrays', () => {
  for (const value of [null, {}, { restricted: 'false', calls: [] }, { restricted: false, calls: [1] }]) {
    assert.throws(() => privacyProjection.stateSchema.parse(value), /Invalid Zodo privacy checkpoint/)
  }
  const source = { restricted: false, calls: ['c1'] }
  const parsed = privacyProjection.stateSchema.parse(source)
  parsed.calls.push('c2')
  assert.deepEqual(source.calls, ['c1'])
  assert.equal(privacyProjection.stateVersion, 2)
})
test('session privacy fails closed for missing or unreadable projections', () => {
  for (const [handle, session] of [
    [{ stateOf: () => ({ restricted: false }) }, null],
    [{ stateOf: () => undefined }, {}],
    [{ stateOf: () => ({ restricted: 'false' }) }, {}],
    [{ stateOf: () => { throw new Error('PRIVATE_SENTINEL') } }, {}],
  ]) assert.throws(() => sessionHasRestrictedExport(handle, session), error => {
    assert.equal(error.code, 'ZODO_PRIVACY_HISTORY_UNAVAILABLE')
    assert.doesNotMatch(error.message, /PRIVATE_SENTINEL/)
    return true
  })
  assert.equal(sessionHasRestrictedExport({ stateOf: () => ({ restricted: false }) }, {}), false)
  assert.equal(sessionHasRestrictedExport({ stateOf: () => ({ restricted: true }) }, {}), true)
})
