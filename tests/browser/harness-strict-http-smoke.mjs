/** Same-process HTTP smoke through the unmodified Host bridge, NOT rendered UI.
 * Owns a fresh synthetic Core; never accepts a live origin/profile/data directory.
 * Run with ZODO_FIXTURE_CORE_BIN pointing to a binary built from this checkout.
 */
import assert from 'node:assert/strict'
import { randomUUID, createHash } from 'node:crypto'
import { readFile, stat } from 'node:fs/promises'
import { startFixture } from './harness-isolated-fixture.mjs'

assert.ok(process.env.ZODO_FIXTURE_CORE_BIN, 'Set ZODO_FIXTURE_CORE_BIN to this checkout’s test binary')
const binaryHash = createHash('sha256').update(await readFile(process.env.ZODO_FIXTURE_CORE_BIN)).digest('hex')
const fixture = await startFixture({ binary: process.env.ZODO_FIXTURE_CORE_BIN })
let passed = false
try {
  const acquireSession = async () => {
    const session = await fetch(fixture.url + '/zodo-bridge/v1/session', { method: 'POST', headers: { Origin: fixture.url }, body: '{}' })
    assert.equal(session.status, 200)
    const cookie = session.headers.getSetCookie().map(value => value.split(';')[0]).join('; ')
    assert.ok(cookie)
    return cookie
  }
  let cookie = await acquireSession()
  const rpc = async (kind, name, payload = {}) => {
    const response = await fetch(fixture.url + '/zodo-bridge/v1/rpc', {
      method: 'POST', headers: { Origin: fixture.url, Cookie: cookie, 'Content-Type': 'application/json' },
      body: JSON.stringify({ kind, envelope: { schema: `zodo.${kind}/1`, workspace_id: 'ws_local', request_id: randomUUID(),
        ...(kind === 'command' ? { operation: name, operation_version: 1, idempotency_key: randomUUID() } : { query: name }), payload } }),
    })
    const body = await response.json()
    assert.equal(response.status, 200, `${name} HTTP status`)
    assert.equal(body.ok, true, `${name} bridge status`)
    return body.result.data
  }
  const command = (name, payload) => rpc('command', name, payload)
  const snapshot = () => rpc('query', 'workspace.snapshot')
  const future = new Date(Date.now() + 2 * 86400000)
  while (['Sat', 'Sun'].includes(future.toLocaleDateString('en-US', { weekday: 'short', timeZone: 'Asia/Shanghai' }))) future.setUTCDate(future.getUTCDate() + 1)
  const day = future.toLocaleDateString('en-CA', { timeZone: 'Asia/Shanghai' })
  await command('task.create', { task: { title: 'HTTP strict target', estimate_minutes: 30 } })
  await command('task.create', { task: { title: 'HTTP locked existing', estimate_minutes: 30 } })
  await command('capacity.create', { capacity: { title: 'HTTP synthetic capacity', start: `${day}T09:00:00+08:00`, end: `${day}T12:00:00+08:00`, capacity_minutes: 180 } })
  await command('commitment.create', { commitment: { title: 'HTTP fixed appointment', start: `${day}T08:00:00+08:00`, end: `${day}T08:30:00+08:00`, locked: true } })
  let state = await snapshot()
  assert.equal(state.workspace.timezone ?? state.workspace.settings?.timezone, 'Asia/Shanghai')
  const task = state.tasks.find(row => row.title === 'HTTP strict target')
  const oldTask = state.tasks.find(row => row.title === 'HTTP locked existing')
  const capacity = state.capacity_blocks[0]
  // Fixture setup only; the new task below must use the strict proposal flow.
  await command('allocation.create', { allocation: { task_id: oldTask.id, capacity_block_id: capacity.id,
    start: `${day}T09:00:00+08:00`, end: `${day}T09:30:00+08:00`, minutes: 30, locked: true } })
  const before = await snapshot()
  const operation = { operation: 'allocation.schedule', payload: { allocation: {
    task_id: task.id, expected_task_version: task.version, capacity_block_id: capacity.id,
    start: `${day}T10:00:00+08:00`, end: `${day}T10:30:00+08:00`,
  } } }
  const { proposal } = await command('proposal.create', { intent: 'HTTP synthetic strict preview only', operations: [operation] })
  assert.equal(proposal.status, 'validated')
  assert.equal(proposal.operations.length, 1)
  const reviewed = proposal.operations[0]
  assert.equal(reviewed.operation, operation.operation)
  const normalize = allocation => ({ ...allocation, start: new Date(allocation.start).toISOString(), end: new Date(allocation.end).toISOString() })
  assert.deepEqual(normalize(reviewed.payload.allocation), normalize(operation.payload.allocation))
  assert.deepEqual((await snapshot()).allocations, before.allocations, 'preview cannot allocate')
  const { approval } = await command('proposal.approve', { proposal_id: proposal.id })
  assert.deepEqual((await snapshot()).allocations, before.allocations, 'approval cannot allocate')
  await command('changes.commit', { proposal_id: proposal.id, approval_grant: approval.id })
  state = await snapshot()
  const added = state.allocations.filter(row => !before.allocations.some(old => old.id === row.id))
  assert.equal(added.length, 1)
  assert.equal(added[0].task_id, task.id)
  assert.equal(added[0].capacity_block_id, capacity.id)
  assert.equal(added[0].minutes, 30)
  assert.equal(Date.parse(added[0].start), Date.parse(operation.payload.allocation.start))
  assert.equal(Date.parse(added[0].end), Date.parse(operation.payload.allocation.end))
  for (const old of before.allocations) assert.deepEqual(state.allocations.find(row => row.id === old.id), old)
  for (const key of ['tasks', 'capacity_blocks', 'commitments', 'calendar_events', 'focus_sessions']) assert.deepEqual(state[key], before[key], key)
  assert.equal(state.proposals.find(row => row.id === proposal.id).status, 'applied')
  assert.equal(fixture.llmCalls.streams.length, 0)
  assert.ok(state.allocations.some(row => row.locked), 'non-vacuous locked persistence check')
  assert.ok(state.commitments.some(row => row.locked), 'non-vacuous fixed persistence check')
  const persisted = state
  assert.deepEqual(await fixture.restartCore(), { restarted: true })
  // Reacquire via the real adapter, not copied runtime credentials or old cookies.
  cookie = await acquireSession()
  state = await snapshot()
  assert.equal(state.workspace.revision, persisted.workspace.revision)
  for (const key of ['tasks', 'allocations', 'commitments', 'capacity_blocks', 'calendar_events', 'focus_sessions', 'proposals']) {
    assert.deepEqual(state[key], persisted[key], `${key} persisted across actual Core process restart`)
  }
  assert.equal(state.allocations.filter(row => row.task_id === task.id).length, 1)
  assert.equal(fixture.llmCalls.streams.length, 0)
  passed = true
} finally {
  await fixture.stop()
  const removed = !(await stat(fixture.dataDirectory).then(() => true, () => false))
  assert.equal(removed, true)
  console.log(JSON.stringify({ test: 'strict-schedule-owned-http-bridge', status: passed ? 'passed' : 'failed', renderedUI: false, fullHost: false, coreRestartPersistence: passed, coreSha256: binaryHash, dataRemoved: removed }))
}
