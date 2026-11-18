import assert from 'node:assert/strict'
import { test } from 'node:test'
import { summarizeForCi } from './harness-ci-summary.mjs'
const clean = { coreStopped: true, serverStopped: true, dataRemoved: true }
const valid = () => ({ status: 'passed', executed: 1, passed: 1, failed: 0, cases: [{ name: 'known case', passed: true }], cleanup: clean })
test('successful full suite retains counts and cleanup only', () => {
  assert.deepEqual(summarizeForCi(valid(), ['known case']), { status: 'passed', executed: 1, expected: 1, passed: 1, failed: 0, failedCases: [], cleanup: clean })
})
test('real failure names retained but arbitrary report strings cannot leak', () => {
  const report = { ...valid(), status: 'failed', passed: 0, failed: 1,
    cases: [{ name: 'known case', passed: false, error: 'SECRET_SENTINEL' }], output: '/private/SECRET_SENTINEL', fatal: { message: 'SECRET_SENTINEL' } }
  const result = summarizeForCi(report, ['known case'])
  assert.equal(result.status, 'failed')
  assert.deepEqual(result.failedCases, ['known case'])
  assert.doesNotMatch(JSON.stringify(result), /SECRET_SENTINEL|private/)
})
test('unknown or duplicate cases and inconsistent counts fail closed', () => {
  for (const report of [{ ...valid(), cases: [{ name: 'unknown SECRET', passed: true }] }, { ...valid(), executed: 99 }, { ...valid(), status: 'SECRET' }, { ...valid(), cases: [valid().cases[0], valid().cases[0]] }]) {
    assert.throws(() => summarizeForCi(report, ['known case']), /INVALID_REPORT/)
  }
})
test('partial execution and missing cleanup never pass', () => {
  assert.equal(summarizeForCi(valid(), ['known case', 'second']).status, 'incomplete')
  assert.equal(summarizeForCi({ ...valid(), cleanup: {} }, ['known case']).status, 'incomplete')
  assert.equal(summarizeForCi({ status: 'blocked', executed: 0, passed: 0, failed: 0, cases: [] }, ['known case']).status, 'blocked')
})
