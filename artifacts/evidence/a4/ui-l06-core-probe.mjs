// UI-L06 live probe: read the running Host 3181 bridge (never restarts it).
// Uses only the published same-origin bridge. Prints facts needed for the evidence report.
const BASE = 'http://127.0.0.1:3181'

async function main() {
  const session = await fetch(`${BASE}/zodo-bridge/v1/session`, {
    method: 'POST',
    headers: { Origin: BASE, 'Content-Type': 'application/json' },
    body: '{}',
  })
  const cookie = session.headers.get('set-cookie')?.split(';')[0] ?? ''
  console.log('session_status', session.status, 'cookie', cookie ? 'minted' : 'missing')
  const sessionBody = await session.json()
  console.log('core', JSON.stringify(sessionBody.core))

  const rpc = async (kind, envelope) => {
    const response = await fetch(`${BASE}/zodo-bridge/v1/rpc`, {
      method: 'POST',
      headers: { Origin: BASE, 'Content-Type': 'application/json', Cookie: cookie },
      body: JSON.stringify({ kind, envelope }),
    })
    const body = await response.json().catch(() => null)
    return { status: response.status, body }
  }

  const snapshot = await rpc('query', {
    schema: 'zodo.query/1',
    request_id: 'ui06_snapshot',
    workspace_id: 'ws_local',
    query: 'workspace.snapshot',
    payload: {},
  })
  const data = snapshot.body?.result?.data
  const tasks = data?.tasks ?? []
  const seriesTasks = tasks.filter(task => task.series_id)
  const rules = data?.recurrence_rules ?? []
  console.log('snapshot_status', snapshot.status, 'revision', data?.workspace?.revision)
  console.log('series_tasks', seriesTasks.length, 'rules', rules.length)
  console.log('rule0', JSON.stringify(rules[0] ?? null))
  for (const task of seriesTasks) {
    console.log('series_task', JSON.stringify({
      id: task.id,
      title: task.title,
      status: task.status,
      target_due: task.target_due,
      occurrence_key: task.occurrence_key,
      version: task.version,
    }))
  }

  // Capability probe: a missing rule id fails harmlessly. A Core without the new command answers
  // UnsupportedOperation; a Core with it answers NotFound. No fact is written either way.
  const probe = await rpc('command', {
    schema: 'zodo.command/1',
    request_id: 'ui06_probe',
    idempotency_key: 'ui06_probe_missing_rule',
    workspace_id: 'ws_local',
    operation: 'recurrence.update',
    operation_version: 1,
    payload: { rule_id: 'rec_missing_probe', title: 'probe' },
  })
  console.log('probe_status', probe.status)
  console.log('probe_body', JSON.stringify(probe.body))
}

main().catch(error => {
  console.error('probe failed:', error)
  process.exitCode = 1
})
