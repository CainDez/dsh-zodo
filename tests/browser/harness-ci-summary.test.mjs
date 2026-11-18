import assert from 'node:assert/strict'
import { test } from 'node:test'
import { summarizeForCi, LOST_RECEIPT_CASE, LOST_RECEIPT_CHECKPOINTS } from './harness-ci-summary.mjs'
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

const receiptFailure = checkpoint => ({ status: 'failed', executed: 1, passed: 0, failed: 1,
  cases: [{ name: LOST_RECEIPT_CASE, passed: false, checkpoint, error: 'SECRET_SENTINEL' }], cleanup: clean })
test('lost receipt checkpoint is allowlisted without exposing raw failure content', () => {
  for (const checkpoint of LOST_RECEIPT_CHECKPOINTS) {
    const result = summarizeForCi(receiptFailure(checkpoint), [LOST_RECEIPT_CASE])
    assert.deepEqual(result.failureCheckpoints, [{ name: LOST_RECEIPT_CASE, checkpoint }])
    assert.doesNotMatch(JSON.stringify(result), /SECRET_SENTINEL/)
    assert.equal(result.status, 'failed')
  }
})
test('unknown, non-string and misattributed checkpoints fail closed', () => {
  for (const checkpoint of ['SECRET_SENTINEL', null, { label: 'close-review' }]) {
    assert.throws(() => summarizeForCi(receiptFailure(checkpoint), [LOST_RECEIPT_CASE]), /INVALID_REPORT/)
  }
  const other = receiptFailure('close-review')
  other.cases[0].name = 'known case'
  assert.throws(() => summarizeForCi(other, ['known case']), /INVALID_REPORT/)
  const success = receiptFailure('close-review')
  success.cases[0].passed = true
  assert.throws(() => summarizeForCi(success, [LOST_RECEIPT_CASE]), /INVALID_REPORT/)
})
