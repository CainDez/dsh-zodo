import fs from 'node:fs'

const host = process.env.ZODO_A2_URL.replace(/\/\?.*$/, '').replace(/\/$/, '')
const origin = new URL(host).origin
const response = await fetch(`${host}/zodo-bridge/v1/session`, {
  method: 'POST', headers: { Origin: origin, 'Content-Type': 'application/json' }, body: '{}',
})
const cookie = response.headers.get('set-cookie')?.split(';')[0] ?? ''
const sessionStatus = response.status
async function rpc(kind, envelope) {
  const r = await fetch(`${host}/zodo-bridge/v1/rpc`, {
    method: 'POST', headers: { Cookie: cookie, Origin: origin, 'Content-Type': 'application/json' },
    body: JSON.stringify({ kind, envelope }),
  })
  return { status: r.status, body: await r.json() }
}
const goal = await rpc('command', {
  schema: 'zodo.command/1', request_id: `h25-goal-${Date.now()}`, idempotency_key: `h25-goal-${Date.now()}`,
  workspace_id: 'ws_local', operation: 'goal.create', operation_version: 1,
  payload: { goal: { title: '宿主生命周期目标', success_criteria: '宿主退出后 Core Run 仍完成' } },
})
const goalId = goal.body?.result?.data?.changes?.[0]?.id ?? goal.body?.data?.changes?.[0]?.id
const runResponse = await fetch(`${host}/zodo-bridge/v1/runs`, {
  method: 'POST', headers: { Cookie: cookie, Origin: origin, 'Content-Type': 'application/json' },
  body: JSON.stringify({
    intent: 'goal.decompose', input: { goal_id: goalId },
    idempotency_key: `h25-run-${Date.now()}`,
  }),
})
const runBody = await runResponse.json()
const runId = runBody?.result?.run?.id ?? runBody?.run?.id ?? runBody?.result?.result?.run?.id
fs.writeFileSync('artifacts/evidence/a3/h25-run-start.json', JSON.stringify({
  run_body: runBody,
  session_status: sessionStatus, goal_status: goal.status, goal_id: goalId,
  run_status: runResponse.status, run_id: runId,
}, null, 2))
console.log(JSON.stringify({ goalId, runId, runStatus: runResponse.status }))
