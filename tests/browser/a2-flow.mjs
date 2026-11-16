/**
 * A2 real-host interaction flow: capture, create work, add capacity, generate a plan, approve and
 * commit it through the Host BFF, then verify the rendered Core receipt and persisted projections.
 */
import path from 'node:path'
import { mkdir, writeFile } from 'node:fs/promises'
import { chromium } from 'playwright-core'

const url = process.env.ZODO_A2_URL
if (!url) {
  console.error('ZODO_A2_URL is required')
  process.exit(2)
}
const outDir = path.resolve('artifacts/evidence/a2')
await mkdir(outDir, { recursive: true })
const browser = await chromium.launch({
  executablePath: process.env.ZODO_CHROME ?? 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
  headless: true,
})
const page = await (await browser.newContext({ viewport: { width: 1440, height: 1050 } })).newPage()
const consoleMessages = []
const pageErrors = []
const bridgeTraffic = []
page.on('console', message => consoleMessages.push({ type: message.type(), text: message.text() }))
page.on('pageerror', error => pageErrors.push(String(error)))
page.on('response', async response => {
  if (!response.url().includes('/zodo-bridge/')) return
  let body
  try { body = await response.text() } catch { body = undefined }
  bridgeTraffic.push({
    url: response.url(),
    status: response.status(),
    request: response.request().postData()?.slice(0, 1600),
    body: body?.slice(0, 5000),
  })
})

await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 60_000 })
await page.waitForTimeout(2000)
for (const label of ['继续', '开始使用', '稍后', '关闭']) {
  const control = page.getByRole('button', { name: label, exact: true }).first()
  if (await control.count()) {
    await control.click({ timeout: 2000 }).catch(() => {})
    await page.waitForTimeout(300)
  }
}
const openSidebar = page.getByRole('button', { name: '打开右侧边栏' }).first()
if (await openSidebar.count()) {
  await openSidebar.click().catch(() => {})
  await page.waitForTimeout(400)
}
let guide = page.locator('button:has-text("Zodo 工作台")').first()
if (!(await guide.count())) {
  const newTab = page.getByRole('button', { name: '新标签页' }).first()
  if (await newTab.count()) await newTab.click().catch(() => {})
  await page.waitForTimeout(400)
  guide = page.locator('button:has-text("Zodo 工作台")').first()
}
await guide.click()
await page.waitForSelector('[data-zodo-workbench="a2"]', { timeout: 20_000 })
await page.waitForTimeout(1200)
await page.screenshot({ path: path.join(outDir, 'flow-00-workbench.png'), fullPage: true })

// Capture raw text.
await page.getByTestId('top-capture').click()
const captureDialog = page.getByRole('dialog', { name: '快速收集' })
await captureDialog.locator('textarea').fill('明天下午联系印刷店，并确认作品集封面')
await captureDialog.getByRole('button', { name: '保存原文' }).click()
await page.waitForTimeout(900)
const captureToast = await page.getByTestId('zodo-toast').innerText().catch(() => '')
await page.screenshot({ path: path.join(outDir, 'flow-01-capture.png'), fullPage: true })

// Create a task.
await page.getByTestId('nav-tasks').click()
await page.getByRole('button', { name: '新建任务' }).click()
const taskDialog = page.getByRole('dialog', { name: '新建任务' })
await taskDialog.getByLabel('标题').fill('筛选 3 个项目素材')
await taskDialog.getByLabel('预计分钟').fill('45')
await taskDialog.getByTestId('task-save').click()
await page.waitForTimeout(900)
const taskCreated = await page.locator('[data-testid^="task-row-"]').count()
await page.screenshot({ path: path.join(outDir, 'flow-02-task.png'), fullPage: true })

// Add capacity for tomorrow afternoon.
await page.getByTestId('nav-schedule').click()
await page.getByRole('button', { name: '增加容量' }).click()
const capacityDialog = page.getByRole('dialog', { name: '增加可用容量' })
await capacityDialog.getByLabel('名称').fill('作品集专注时间')
await capacityDialog.getByLabel('开始').fill('2026-10-26T14:00')
await capacityDialog.getByLabel('结束').fill('2026-10-26T16:00')
await capacityDialog.getByLabel('可用分钟').fill('120')
await capacityDialog.getByRole('button', { name: '保存' }).click()
await page.waitForTimeout(900)
await page.screenshot({ path: path.join(outDir, 'flow-03-capacity.png'), fullPage: true })

// Generate the plan.
await page.getByRole('button', { name: '生成计划' }).first().click()
await page.waitForTimeout(1200)
const planDialog = page.getByRole('dialog', { name: '检查计划建议' })
await planDialog.waitFor({ timeout: 15_000 })
const planText = await planDialog.innerText()
await page.screenshot({ path: path.join(outDir, 'flow-04-plan.png'), fullPage: true })
await planDialog.getByTestId('plan-apply').click()
await page.waitForTimeout(1500)
const planToast = await page.getByTestId('zodo-toast').innerText().catch(() => '')
const afterPlanBody = await page.locator('[data-zodo-workbench="a2"]').innerText()
await page.screenshot({ path: path.join(outDir, 'flow-05-applied.png'), fullPage: true })

// Inspect the persisted schedule via the Host bridge from the live page (UI face only).
const schedule = await page.evaluate(async () => {
  const response = await fetch('/zodo-bridge/v1/rpc', {
    method: 'POST',
    credentials: 'same-origin',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      kind: 'query',
      envelope: {
        schema: 'zodo.query/1',
        request_id: `flow-${Date.now()}`,
        workspace_id: 'ws_local',
        query: 'schedule.get',
        payload: { start: '2026-10-25T00:00:00Z', end: '2026-11-02T00:00:00Z' },
      },
    }),
  })
  return response.json()
})

const result = {
  captureToast,
  taskCreated,
  planText,
  planToast,
  afterPlanHasCommitted: /作品集专注时间|任务分配/.test(afterPlanBody),
  schedule,
  bridgeTraffic,
  consoleMessages,
  pageErrors,
}
await writeFile(path.join(outDir, 'flow-a2.json'), JSON.stringify(result, null, 2))
console.log(JSON.stringify({
  captureToast,
  taskCreated,
  planText: planText.slice(0, 2000),
  planToast,
  afterPlanHasCommitted: result.afterPlanHasCommitted,
  scheduleStatus: schedule?.ok,
  scheduleData: schedule?.result?.data,
  pageErrors,
  consoleErrors: consoleMessages.filter(x => x.type === 'error'),
  bridgeCalls: bridgeTraffic.map(row => ({ url: row.url, status: row.status, request: row.request?.slice(0, 200) })),
}, null, 2))
await browser.close()
