import fs from 'node:fs'
import { chromium } from 'playwright-core'

const runtime = JSON.parse(fs.readFileSync('.runtime/zodo-a2/runtime.json', 'utf8'))
const core = `http://${runtime.host}:${runtime.port}`
async function snapshot() {
  const response = await fetch(`${core}/v1/queries`, {
    method: 'POST',
    headers: { Authorization: `Bearer ${runtime.tokens.ui}`, 'Content-Type': 'application/json' },
    body: JSON.stringify({ schema: 'zodo.query/1', request_id: 'h19-core', workspace_id: 'ws_local', query: 'workspace.snapshot', payload: {} }),
  })
  return (await response.json()).data
}

const before = await snapshot()
const browser = await chromium.launch({
  executablePath: 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
  headless: true,
})
const page = await (await browser.newContext({ viewport: { width: 1440, height: 1000 } })).newPage()
const errors = []
page.on('pageerror', error => errors.push(String(error)))
await page.goto(process.env.ZODO_A2_URL, { waitUntil: 'domcontentloaded', timeout: 60000 })
await page.waitForTimeout(2500)
const fork = await page.evaluate(async () => {
  const current = JSON.parse(localStorage.getItem('dsh.sessions.current') || '{}')
  const sessionId = current.sessionId
  const payload = {
    type: 'client-request',
    rpcId: 'h19-fork-evidence',
    method: 'session/fork',
    payload: { args: { request: { sessionId, atSeq: 1 } } },
  }
  const response = await fetch('/api/session/fork', {
    method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(payload),
  })
  return { sourceSessionId: sessionId, status: response.status, body: await response.json() }
})
await browser.close()
const after = await snapshot()
const result = {
  source_session_id: fork.sourceSessionId,
  fork_status: fork.status,
  fork_error: fork.body?.result?.ok === false ? fork.body.result.error : null,
  child_session_id: fork.body?.result?.value?.sessionId ?? null,
  core_revision_before: before.workspace.revision,
  core_revision_after: after.workspace.revision,
  tasks_before: before.tasks.length,
  tasks_after: after.tasks.length,
  notes_before: before.notes.length,
  notes_after: after.notes.length,
  page_errors: errors,
  facts_duplicated: before.tasks.length !== after.tasks.length || before.workspace.revision !== after.workspace.revision,
}
fs.writeFileSync('artifacts/evidence/a3/h19-fork.json', JSON.stringify(result, null, 2))
console.log(JSON.stringify(result, null, 2))
