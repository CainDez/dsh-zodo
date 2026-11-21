/** Safe reporting helpers: never include launcher logs, environment values or
 * exception messages in the shareable fixture report. Keep raw errors on stderr.
 */
export function fixtureFailure(stage, error) {
  const message = String(error?.message ?? error ?? '')
  if (stage === 'browser-launch' && /(?:socket\(\).*Operation not permitted|ptrace: Operation not permitted|No usable sandbox)/i.test(message)) {
    return { stage, code: 'BROWSER_ENVIRONMENT_RESTRICTED', status: 'blocked',
      guidance: 'Run on a supported disposable browser runner; do not weaken sandbox or syscall protections.' }
  }
  if (stage === 'browser-launch' && /(?:executable.*(?:not found|doesn.t exist)|ENOENT)/i.test(message)) {
    return { stage, code: 'BROWSER_EXECUTABLE_UNAVAILABLE', status: 'blocked',
      guidance: 'Set ZODO_CHROME to an already provisioned supported Chrome/Chromium executable.' }
  }
  return { stage, code: 'FIXTURE_EXECUTION_ERROR', status: 'error',
    guidance: 'Inspect local stderr and prerequisites; no successful browser validation is established.' }
}

export function fixtureSummary(cases, fatal) {
  const passed = cases.filter(row => row.passed === true).length
  const failed = cases.filter(row => row.passed === false).length
  return { passed, failed, executed: cases.length,
    // A blocked startup is not an assertion failure, and zero cases is not green.
    status: fatal?.status ?? (failed ? 'failed' : cases.length ? 'passed' : 'not-run') }
}
