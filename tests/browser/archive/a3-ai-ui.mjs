/**
 * A3 full UI path: goal card -> durable Core AI run -> local Ollama via Core ModelGateway -> proposal
 * -> React review -> explicit user approval -> Core commit receipt.
 */
import path from 'node:path'
import { mkdir, writeFile } from 'node:fs/promises'
import { chromium } from 'playwright-core'

const url = process.env.ZODO_A2_URL
if (!url) throw new Error('ZODO_A2_URL is required')
const outDir = path.resolve('artifacts/evidence/a3')
await mkdir(outDir, { recursive: true })
const browser = await chromium.launch({
  executablePath: process.env.ZODO_CHROME ?? 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
  headless: true,
})
const page = await (await browser.newContext({ viewport: { width: 1440, height: 1050 } })).newPage()
const consoleMessages = []
const pageErrors = []
const bridgeTraffic = []
page.on('console', m => consoleMessages.push({ type: m.type(), text: m.text() }))
page.on('pageerror', e => pageErrors.push(String(e)))
page.on('response', async response => {
  if (!response.url().includes('/zodo-bridge/')) return
  let body
  try { body = await response.text() } catch { body = undefined }
  bridgeTraffic.push({ url: response.url(), status: response.status(), body: body?.slice(0, 2000) })
})

const listRuns = () => page.evaluate(async () => {
  const response = await fetch('/zodo-bridge/v1/runs', { credentials: 'same-origin' })
  if (!response.ok) return { error: response.status, body: await response.text() }
  return response.json()
})

await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 60_000 })
await page.waitForTimeout(2200)
for (const label of ['继续', '开始使用', '稍后', '关闭']) {
  const control = page.getByRole('button', { name: label, exact: true }).first()
  if (await control.count()) {
    await control.click({ timeout: 2000 }).catch(() => {})
    await page.waitForTimeout(250)
  }
}
const openSidebar = page.getByRole('button', { name: '打开右侧边栏' }).first()
if (await openSidebar.count()) await openSidebar.click().catch(() => {})
await page.waitForTimeout(400)
let guide = page.locator('button:has-text("Zodo 工作台")').first()
if (!(await guide.count())) {
  const newTab = page.getByRole('button', { name: '新标签页' }).first()
  if (await newTab.count()) await newTab.click().catch(() => {})
  await page.waitForTimeout(400)
  guide = page.locator('button:has-text("Zodo 工作台")').first()
}
await guide.click()
await page.waitForSelector('[data-zodo-workbench="a2"]', { timeout: 20_000 })
await page.waitForTimeout(900)
await page.getByTestId('nav-goals').click()
await page.waitForTimeout(500)
await page.screenshot({ path: path.join(outDir, 'ai-00-goals.png'), fullPage: true })

const before = await listRuns()
const beforeIds = new Set((before.result?.runs ?? before.runs ?? []).map(run => run.id))
const aiButtons = page.getByRole('button', { name: 'AI 拆解' })
if (!(await aiButtons.count())) throw new Error('AI decompose button not available')
await aiButtons.first().click()
await page.waitForTimeout(1200)
let runId = null
for (let i = 0; i < 20 && !runId; i += 1) {
  const current = await listRuns()
  const runs = current.result?.runs ?? current.runs ?? []
  const created = runs.find(run => !beforeIds.has(run.id))
  if (created) runId = created.id
  else await page.waitForTimeout(400)
}
if (!runId) throw new Error('AI run was not created')
await page.screenshot({ path: path.join(outDir, 'ai-01-run-queued.png'), fullPage: true })

const started = Date.now()
let run = null
while (Date.now() - started < 240_000) {
  await page.waitForTimeout(2000)
  const current = await listRuns()
  const runs = current.result?.runs ?? current.runs ?? []
  run = runs.find(item => item.id === runId)
  if (run && ['succeeded', 'failed', 'cancelled'].includes(run.state)) break
}
if (!run) throw new Error('AI run disappeared')
await page.waitForTimeout(2500)
await page.screenshot({ path: path.join(outDir, 'ai-02-run-result.png'), fullPage: true })
const runText = await page.locator('[data-zodo-workbench="a2"]').innerText()
if (run.state !== 'succeeded') {
  throw new Error(`AI run ${run.state}: ${JSON.stringify(run.error)}`)
}
if (!/检查提案/.test(runText)) throw new Error('succeeded run did not expose a proposal action')

await page.getByRole('button', { name: '检查提案' }).first().click()
await page.waitForTimeout(900)
await page.screenshot({ path: path.join(outDir, 'ai-03-proposal.png'), fullPage: true })
const review = page.getByRole('button', { name: '检查并批准' }).first()
if (!(await review.count())) throw new Error('AI proposal could not be opened for review')
await review.click()
const dialog = page.getByRole('dialog')
await dialog.waitFor({ timeout: 10_000 })
const diffText = await dialog.innerText()
await page.screenshot({ path: path.join(outDir, 'ai-04-review.png'), fullPage: true })
await dialog.getByTestId('plan-apply').click()
await page.waitForTimeout(1800)
const toast = await page.getByTestId('zodo-toast').innerText().catch(() => '')
const finalText = await page.locator('[data-zodo-workbench="a2"]').innerText()
const result = {
  runId,
  run,
  runText,
  diffText,
  toast,
  finalText,
  consoleMessages,
  pageErrors,
  bridgeTraffic,
}
await writeFile(path.join(outDir, 'ai-ui-flow.json'), JSON.stringify(result, null, 2))
console.log(JSON.stringify({
  runId,
  state: run.state,
  taskCount: run.output?.task_count,
  proposalId: run.output?.proposal_id,
  toast,
  proposalDiff: diffText.slice(0, 1500),
  applied: /已保存|已应用/.test(toast + finalText),
  pageErrors,
  consoleErrors: consoleMessages.filter(x => x.type === 'error'),
  bridgeCalls: bridgeTraffic.map(row => ({ url: row.url, status: row.status })),
}, null, 2))
await browser.close()
