/**
 * AI-04: official MCP client calls zodo_replan; the local deterministic planner must create a
 * proposal, never commit, and keep locked/started facts intact. The MCP token cannot approve.
 */
import path from 'node:path'
import { mkdir, writeFile } from 'node:fs/promises'
import { Client } from '@modelcontextprotocol/sdk/client/index.js'
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js'

const root = path.resolve('.')
const zodo = path.join(root, 'target', 'release', 'zodo.exe')
const dataDir = path.join(root, '.runtime', 'zodo-a2')
const outDir = path.join(root, 'artifacts', 'evidence', 'a4')
await mkdir(outDir, { recursive: true })
const transport = new StdioClientTransport({
  command: zodo, args: ['--data-dir', dataDir, 'mcp'], cwd: root, stderr: 'pipe',
})
const client = new Client({ name: 'zodo-a4-replan', version: '0.1.0' }, { capabilities: {} })
try {
  await client.connect(transport)
  const tools = await client.listTools()
  const list = await client.callTool({ name: 'zodo_list_tasks', arguments: { limit: 10 } })
  const listPayload = JSON.parse(list.content.find(item => item.type === 'text').text)
  const taskIds = (listPayload.tasks ?? []).slice(0, 2).map(task => task.id)
  const start = new Date(Date.now() + 60 * 60 * 1000).toISOString()
  const end = new Date(Date.now() + 7 * 24 * 60 * 60 * 1000).toISOString()
  const replan = await client.callTool({
    name: 'zodo_replan',
    arguments: { task_ids: taskIds, horizon_start: start, horizon_end: end },
  })
  const payload = JSON.parse(replan.content.find(item => item.type === 'text').text)
  const history = await client.callTool({ name: 'zodo_get_proposal', arguments: { proposal_id: payload.proposal_id } })
  const historyPayload = JSON.parse(history.content.find(item => item.type === 'text').text)
  const result = {
    tools: tools.tools.map(tool => tool.name),
    task_ids: taskIds,
    replan: payload,
    proposal: historyPayload,
    no_approve_or_commit_tool: !tools.tools.some(tool => /approve|commit/.test(tool.name)),
  }
  await writeFile(path.join(outDir, 'a4-replan-mcp.json'), JSON.stringify(result, null, 2))
  console.log(JSON.stringify({
    tools: result.tools,
    proposal_id: payload.proposal_id,
    status: payload.status,
    assigned_minutes: payload.assigned_minutes,
    unplaced_minutes: payload.unplaced_minutes,
    no_approve_or_commit_tool: result.no_approve_or_commit_tool,
  }, null, 2))
} finally {
  await client.close().catch(() => {})
}
