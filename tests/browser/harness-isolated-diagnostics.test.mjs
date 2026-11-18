import assert from 'node:assert/strict'
import { test } from 'node:test'
import { fixtureFailure, fixtureSummary } from './harness-isolated-diagnostics.mjs'

test('Chromium syscall/sandbox startup failures are blocked, never green', () => {
  for (const message of ['socket() failed: Operation not permitted (1)', 'ptrace: Operation not permitted', 'No usable sandbox!']) {
    const fatal = fixtureFailure('browser-launch', new Error(message))
    assert.equal(fatal.code, 'BROWSER_ENVIRONMENT_RESTRICTED')
    assert.deepEqual(fixtureSummary([], fatal), { passed: 0, failed: 0, executed: 0, status: 'blocked' })
    assert.match(fatal.guidance, /do not weaken/)
  }
})

test('missing installed browser is an explicit prerequisite blocker', () => {
  const fatal = fixtureFailure('browser-launch', new Error("Executable doesn't exist at /private/path"))
  assert.equal(fatal.code, 'BROWSER_EXECUTABLE_UNAVAILABLE')
  assert.equal(fatal.status, 'blocked')
})

test('other stages and unknown browser errors remain errors, not misclassified restrictions', () => {
  for (const [stage, message] of [['fixture-start', 'socket() failed: Operation not permitted'], ['browser-launch', 'Target closed unexpectedly'], ['artifacts', 'ENOENT']]) {
    assert.equal(fixtureFailure(stage, new Error(message)).status, 'error')
  }
})

test('diagnostics do not copy launcher credentials, paths or message content', () => {
  const fatal = fixtureFailure('browser-launch', new Error('socket() failed: Operation not permitted SECRET_SENTINEL /private/profile --token=secret'))
  assert.doesNotMatch(JSON.stringify(fatal), /SECRET_SENTINEL|private\/profile|token=secret/)
})

test('case summaries distinguish failure, success, no execution and fatal partial runs', () => {
  assert.deepEqual(fixtureSummary([]), { passed: 0, failed: 0, executed: 0, status: 'not-run' })
  assert.equal(fixtureSummary([{ passed: true }]).status, 'passed')
  assert.deepEqual(fixtureSummary([{ passed: true }, { passed: false }]), { passed: 1, failed: 1, executed: 2, status: 'failed' })
  const fatal = fixtureFailure('browser-context', new Error('closed'))
  assert.deepEqual(fixtureSummary([{ passed: true }], fatal), { passed: 1, failed: 0, executed: 1, status: 'error' })
})
