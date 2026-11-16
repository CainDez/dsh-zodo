/**
 * A3 MCP: use the official @modelcontextprotocol/sdk client over stdio to list tools, read tasks,
 * submit a planning proposal and a structured task proposal, then read the result. The MCP token
 * cannot approve or commit.
 */
import path from 'node:path'
import { mkdir, writeFile } from 'node:fs/promises'
import { Client } from '@modelcontextprotocol/sdk/client/index.js'
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js'

const root = path.resolve('..', '..')
const zodo = path.join(root, 'target', 'release', 'zodo.exe')
const dataDir = path.join(root, '.runtime', 'zodo-a2')
const outDir = path.join(root, 'artifacts', 'evidence', 'a3')
await mkdir(outDir, { recursive: true })

const transport = new StdioClientTransport({
  command: zodo,
  args: ['--data-dir', dataDir, 'mcp'],
  cwd: root,
  stderr: 'pipe',
})
const client = new Client({ name: 'zodo-a3-validation', version: '0.1.0' }, { capabilities: {} })
const toolCalls = []
try {
  await client.connect(transport)
  const server = client.getServerVersion()
  const tools = await client.listTools()
  const list = await client.callTool({ name: 'zodo_list_tasks', arguments: { limit: 10 } })
  toolCalls.push({ name: 'zodo_list_tasks', list })
  const listPayload = JSON.parse(list.content.find(item => item.type === 'text').text)
  const taskId = listPayload.tasks?.[0]?.id
  const horizonStart = new Date(Date.now() + 60 * 60 * 1000).toISOString()
  const horizonEnd = new Date(Date.now() + 7 * 24 * 60 * 60 * 1000).toISOString()
  const plan = await client.callTool({
    name: 'zodo_plan_propose',
    arguments: {
      task_ids: taskId ? [taskId] : [],
      horizon_start: horizonStart,
      horizon_end: horizonEnd,
    },
  })
  toolCalls.push({ name: 'zodo_plan_propose', plan })
  const planPayload = JSON.parse(plan.content.find(item => item.type === 'text').text)
  const proposal = await client.callTool({
    name: 'zodo_propose',
    arguments: {
      intent: 'MCP 客户端测试：提交一个待人工审阅的任务',
      operations: [{
        operation: 'task.create',
        payload: {
          task: {
            title: 'MCP 提议任务（尚未批准）',
            estimate_minutes: 20,
            priority: 2,
            tags: ['mcp', 'proposal-only'],
          },
        },
      }],
    },
  })
  toolCalls.push({ name: 'zodo_propose', proposal })
  const proposalPayload = JSON.parse(proposal.content.find(item => item.type === 'text').text)
  const fetched = await client.callTool({
    name: 'zodo_get_proposal',
    arguments: { proposal_id: proposalPayload.proposal_id },
  })
  toolCalls.push({ name: 'zodo_get_proposal', fetched })
  const fetchedPayload = JSON.parse(fetched.content.find(item => item.type === 'text').text)
  const result = {
    server,
    tools: tools.tools.map(tool => tool.name),
    listPayload,
    planPayload,
    proposalPayload,
    fetchedPayload,
    toolCalls,
    noApprovalTool: !tools.tools.some(tool => /approve|commit/.test(tool.name)),
  }
  await writeFile(path.join(outDir, 'a3-mcp-sdk.json'), JSON.stringify(result, null, 2))
  console.log(JSON.stringify({
    server,
    tools: result.tools,
    taskCount: listPayload.tasks?.length,
    planStatus: planPayload.status,
    planProposalId: planPayload.proposal_id,
    proposalStatus: proposalPayload.status,
    proposalId: proposalPayload.proposal_id,
    fetchedStatus: fetchedPayload.status,
    noApprovalTool: result.noApprovalTool,
  }, null, 2))
} finally {
  await client.close().catch(() => {})
}
