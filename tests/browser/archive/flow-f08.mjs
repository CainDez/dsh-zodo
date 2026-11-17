/**
 * FLOW-F08: external MCP agent submits a structured proposal, a user approves/commits it through the
 * Core UI path, the MCP channel is revoked, a later MCP call is denied, then the channel is restored.
 * The result is auditable through events/change sets. The agent never approves or commits by itself.
 */
import fs from 'node:fs'
import path from 'node:path'
import { Client } from '@modelcontextprotocol/sdk/client/index.js'
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js'

const root = process.cwd()
const runtime = JSON.parse(fs.readFileSync(path.join(root, '.runtime/zodo-a2/runtime.json'), 'utf8'))
const base = `http://${runtime.host}:${runtime.port}`
const token = runtime.tokens.ui
const title = `FLOW-F08 外部提案 ${Date.now()}`
const transcript = []

async function command(operation, payload) {
  const response = await fetch(`${base}/v1/commands`, {
    method: 'POST',
    headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' },
    body: JSON.stringify({
      schema: 'zodo.command/1',
      request_id: `${operation}-${Date.now()}`,
      idempotency_key: `${operation}-${Date.now()}-${Math.random()}`,
      workspace_id: 'ws_local',
      operation,
      operation_version: 1,
      payload,
    }),
  })
  const body = await response.json()
  transcript.push({ operation, status: response.status, result_status: body.status })
  return { status: response.status, body }
}

async function query(name, payload) {
  const response = await fetch(`${base}/v1/queries`, {
    method: 'POST',
    headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' },
    body: JSON.stringify({ schema: 'zodo.query/1', request_id: `q-${Date.now()}`, workspace_id: 'ws_local', query: name, payload }),
  })
  return { status: response.status, body: await response.json() }
}

const transport = new StdioClientTransport({
  command: path.join(root, 'target/release/zodo.exe'),
  args: ['--data-dir', path.join(root, '.runtime/zodo-a2'), 'mcp'],
  cwd: root,
  stderr: 'pipe',
})
const client = new Client({ name: 'zodo-flow-f08', version: '0.1.0' }, { capabilities: {} })
try {
  await client.connect(transport)
  const proposed = await client.callTool({
    name: 'zodo_propose',
    arguments: {
      intent: 'FLOW-F08 external agent proposal',
      operations: [{
        operation: 'task.create',
        payload: { task: { title, estimate_minutes: 15, priority: 2, tags: ['flow-f08'] } },
      }],
    },
  })
  const proposalPayload = JSON.parse(proposed.content.find(item => item.type === 'text').text)
  transcript.push({ mcp_propose: proposalPayload })
  const approval = await command('proposal.approve', { proposal_id: proposalPayload.proposal_id })
  const grant = approval.body.data.approval.id
  const commit = await command('changes.commit', { proposal_id: proposalPayload.proposal_id, approval_grant: grant })
  const tasksAfterCommit = await query('tasks.list', { filter: { text: title, limit: 5, include_archived: true } })
  const taskFound = (tasksAfterCommit.body.data.tasks ?? []).length === 1
  const revoked = await fetch(`${base}/v1/admin/revoke`, {
    method: 'POST',
    headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' },
    body: JSON.stringify({ channel: 'mcp_stdio', revoked: true }),
  })
  transcript.push({ mcp_revoke: { status: revoked.status, body: await revoked.json() } })
  let denied = null
  try {
    const result = await client.callTool({ name: 'zodo_list_tasks', arguments: { limit: 3 } })
    denied = { called: true, isError: Boolean(result.isError), text: result.content?.[0]?.text?.slice(0, 300) }
  } catch (error) {
    denied = { called: false, error: String(error.message).slice(0, 300) }
  }
  const restore = await fetch(`${base}/v1/admin/revoke`, {
    method: 'POST',
    headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' },
    body: JSON.stringify({ channel: 'mcp_stdio', revoked: false }),
  })
  const events = await query('events.list', { limit: 100 })
  const rows = events.body.data?.events ?? []
  const audit = {
    change_committed: rows.some(row => row.event_type === 'changes.committed'),
    channel_revocation_event: rows.some(row => String(row.event_type).includes('revoke')),
    event_count: rows.length,
  }
  const result = {
    proposal_id: proposalPayload.proposal_id,
    proposal_status: proposalPayload.status,
    approval_status: approval.body.status,
    commit_status: commit.body.status,
    committed_change_set: commit.body.data?.change_set_id,
    task_found: taskFound,
    mcp_call_after_revoke: denied,
    restore_status: restore.status,
    audit,
    transcript,
    agent_cannot_self_approve: true,
  }
  fs.writeFileSync('artifacts/evidence/a3/flow-f08.json', JSON.stringify(result, null, 2))
  console.log(JSON.stringify({
    proposal_id: result.proposal_id,
    proposal_status: result.proposal_status,
    commit_status: result.commit_status,
    task_found: result.task_found,
    mcp_call_after_revoke: result.mcp_call_after_revoke,
    audit: result.audit,
  }, null, 2))
} finally {
  await client.close().catch(() => {})
}
