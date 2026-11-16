import fs from 'node:fs'
import { chromium } from 'playwright-core'

// The browser is only used to establish the Host's normal same-origin session path; event
// reconnection is then exercised over the BFF with the same cookie.
const runtime = JSON.parse(fs.readFileSync('.runtime/zodo-a2/runtime.json', 'utf8'))
const host = 'http://127.0.0.1:3181'
const origin = host

async function mint() {
  const response = await fetch(`${host}/zodo-bridge/v1/session`, {
    method: 'POST', headers: { Origin: origin, 'Content-Type': 'application/json' }, body: '{}',
  })
  const cookie = response.headers.get('set-cookie')?.split(';')[0] ?? ''
  return { status: response.status, cookie, body: await response.json() }
}

async function events(cookie, after) {
  const response = await fetch(`${host}/zodo-bridge/v1/events?after=${encodeURIComponent(after)}&limit=50`, {
    headers: { Cookie: cookie, Origin: origin },
  })
  return { status: response.status, body: await response.json() }
}

async function rpc(cookie, kind, envelope) {
  const response = await fetch(`${host}/zodo-bridge/v1/rpc`, {
    method: 'POST', headers: { Cookie: cookie, Origin: origin, 'Content-Type': 'application/json' },
    body: JSON.stringify({ kind, envelope }),
  })
  return { status: response.status, body: await response.json() }
}

const firstSession = await mint()
const first = await events(firstSession.cookie, 'ev_0')
const cursor = first.body?.result?.data?.next_cursor
const created = await rpc(firstSession.cookie, 'command', {
  schema: 'zodo.command/1',
  request_id: `events-${Date.now()}`,
  idempotency_key: `events-${Date.now()}`,
  workspace_id: runtime.workspace_id ?? 'ws_local',
  operation: 'task.create',
  operation_version: 1,
  payload: { task: { title: '事件重连测试任务', estimate_minutes: 10 } },
})
const second = await events(firstSession.cookie, cursor)
const reconnectSession = await mint()
const third = await events(reconnectSession.cookie, cursor)
const revoke = await fetch(`${host}/zodo-bridge/v1/admin/revoke`, {
  method: 'POST', headers: { Cookie: firstSession.cookie, Origin: origin, 'Content-Type': 'application/json' },
  body: JSON.stringify({ channel: 'agent', revoked: true }),
})
const agentEvents = await fetch(`http://${runtime.host}:${runtime.port}/v1/events?after=ev_0&limit=10`, {
  headers: { Authorization: `Bearer ${runtime.tokens.agent}` },
})
const restore = await fetch(`${host}/zodo-bridge/v1/admin/revoke`, {
  method: 'POST', headers: { Cookie: firstSession.cookie, Origin: origin, 'Content-Type': 'application/json' },
  body: JSON.stringify({ channel: 'agent', revoked: false }),
})
const result = {
  first_session: firstSession.status,
  first_events_status: first.status,
  first_events_count: first.body?.result?.data?.events?.length ?? null,
  cursor,
  create_status: created.status,
  second_events_status: second.status,
  second_events_count: second.body?.result?.data?.events?.length ?? null,
  reconnect_session: reconnectSession.status,
  reconnect_events_status: third.status,
  reconnect_events_count: third.body?.result?.data?.events?.length ?? null,
  revoke_status: revoke.status,
  agent_events_after_revoke: agentEvents.status,
  restore_status: restore.status,
  token_values_recorded: false,
}
fs.writeFileSync('artifacts/evidence/a3/h22-events-reconnect.json', JSON.stringify(result, null, 2))
console.log(JSON.stringify(result, null, 2))
