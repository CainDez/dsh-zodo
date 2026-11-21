/** Public CI output is allowlisted. Raw errors, screenshots, paths and data stay local. */
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

export const LOST_RECEIPT_CASE = 'Lost commit receipt blocks resubmit while refresh reconciles the single saved allocation'
export const LOST_RECEIPT_CHECKPOINTS = Object.freeze([
  'not-started',
  'create-synthetic-task',
  'read-setup',
  'require-task-and-capacity',
  'create-strict-proposal',
  'require-validated-proposal',
  'read-preview-facts',
  'require-no-preview-allocation',
  'open-proposals-page',
  'open-proposal-review',
  'review-mounted',
  'install-response-loss',
  'click-approve',
  'wait-uncertain-notice',
  'require-upstream-success',
  'require-one-commit',
  'require-disabled-approval',
  'require-no-retry-control',
  'read-committed-facts',
  'require-one-new-allocation',
  'require-allocation-fields',
  'require-preserved-allocations',
  'require-preserved-facts',
  'require-applied-proposal',
  'capture-local-evidence',
  'close-review',
  'review-closed',
  'reload-agenda',
  'select-single-day',
  'select-date',
  'allocation-visible-after-reload',
  'require-no-duplicate-after-reload',
  'require-no-resubmit',
  'remove-response-interceptor',
  'restore-agenda-view',
  'restore-seven-day-view',
  'restore-today'
])

export const FIRST_SCHEDULE_CASE = 'First schedule guidance exposes task creation on Today and Schedule without writing'
const FIRST_SCHEDULE_STAGES = ['navigate', 'verify-cta', 'open-keyboard', 'wait-open', 'dismiss-keyboard', 'wait-hidden', 'restore-focus']
export const FIRST_SCHEDULE_CHECKPOINTS = Object.freeze([
  'baseline-snapshot',
  ...['today', 'schedule'].flatMap(view => FIRST_SCHEDULE_STAGES.map(stage => `${view}-${stage}`)),
  'after-snapshot', 'verify-no-writes',
])
const checkpointLabels = new Map([
  [LOST_RECEIPT_CASE, LOST_RECEIPT_CHECKPOINTS],
  [FIRST_SCHEDULE_CASE, FIRST_SCHEDULE_CHECKPOINTS],
])

export function summarizeForCi(report, knownCases) {
  if (!Array.isArray(report?.cases) || !Array.isArray(knownCases) || !knownCases.length) throw new Error('INVALID_REPORT')
  const statuses = ['passed', 'failed', 'blocked', 'error', 'not-run']
  if (!statuses.includes(report.status)) throw new Error('INVALID_REPORT')
  const seen = new Set()
  const failureCheckpoints = []
  for (const row of report.cases) {
    if (!knownCases.includes(row?.name) || seen.has(row.name) || typeof row.passed !== 'boolean') throw new Error('INVALID_REPORT')
    if (row.checkpoint !== undefined) {
      const checkpoint = checkpointLabels.get(row.name)?.find(label => label === row.checkpoint)
      if (row.passed || !checkpoint) throw new Error('INVALID_REPORT')
      failureCheckpoints.push({ name: knownCases.find(name => name === row.name), checkpoint })
    }
    seen.add(row.name)
  }
  const passed = report.cases.filter(row => row.passed).length
  const failed = report.cases.length - passed
  if (report.executed !== report.cases.length || report.passed !== passed || report.failed !== failed) throw new Error('INVALID_REPORT')
  const cleanup = {
    coreStopped: report.cleanup?.coreStopped === true,
    serverStopped: report.cleanup?.serverStopped === true,
    dataRemoved: report.cleanup?.dataRemoved === true,
  }
  const complete = report.status === 'passed' && passed === knownCases.length && failed === 0 && Object.values(cleanup).every(Boolean)
  return { status: complete ? 'passed' : report.status === 'passed' ? 'incomplete' : report.status,
    executed: report.cases.length, expected: knownCases.length, passed, failed,
    ...(failureCheckpoints.length ? { failureCheckpoints } : {}),
    failedCases: knownCases.filter(name => report.cases.some(row => row.name === name && !row.passed)), cleanup }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const source = readFileSync(new URL('./harness-isolated-regression.mjs', import.meta.url), 'utf8')
    const names = [...source.matchAll(/^  await check\('([^'\n]+)',/gm)].map(match => match[1])
    const result = summarizeForCi(JSON.parse(readFileSync(process.argv[2], 'utf8')), names)
    console.log(JSON.stringify(result))
    if (result.status !== 'passed') process.exitCode = 1
  } catch {
    console.log(JSON.stringify({ status: 'invalid-or-missing-report', cleanup: 'unverified' }))
    process.exitCode = 1
  }
}
