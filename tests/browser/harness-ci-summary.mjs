/** Public CI output is allowlisted. Raw errors, screenshots, paths and data stay local. */
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

export function summarizeForCi(report, knownCases) {
  if (!Array.isArray(report?.cases) || !Array.isArray(knownCases) || !knownCases.length) throw new Error('INVALID_REPORT')
  const statuses = ['passed', 'failed', 'blocked', 'error', 'not-run']
  if (!statuses.includes(report.status)) throw new Error('INVALID_REPORT')
  const seen = new Set()
  for (const row of report.cases) {
    if (!knownCases.includes(row?.name) || seen.has(row.name) || typeof row.passed !== 'boolean') throw new Error('INVALID_REPORT')
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
