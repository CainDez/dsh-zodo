/**
 * A2 agent-proposal review arm: the proposal was created by the native-tool agent path; this flow
 * opens the React proposal surface, checks the diff, approves and commits with the interactive UI.
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
  bridgeTraffic.push({ url: response.url(), status: response.status(), request: response.request().postData()?.slice(0, 1200), body: body?.slice(0, 3000) })
})
await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 60_000 })
await page.waitForTimeout(2200)
for (const label of ['继续', '开始使用', '稍后', '关闭']) {
  const control = page.getByRole('button', { name: label, exact: true }).first()
  if (await control.count()) {
    await control.click({ timeout: 2000 }).catch(() => {})
    await page.waitForTimeout(300)
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
await page.waitForTimeout(1200)
await page.getByTestId('nav-proposals').click()
await page.waitForTimeout(700)
await page.screenshot({ path: path.join(outDir, 'agent-proposal-list.png'), fullPage: true })
const listText = await page.locator('[data-zodo-workbench="a2"]').innerText()
const applyButton = page.getByRole('button', { name: '检查并批准' }).first()
if (!(await applyButton.count())) throw new Error('no pending agent proposal to approve')
await applyButton.click()
const dialog = page.getByRole('dialog')
await dialog.waitFor({ timeout: 10_000 })
const planText = await dialog.innerText()
await page.screenshot({ path: path.join(outDir, 'agent-proposal-review.png'), fullPage: true })
await dialog.getByTestId('plan-apply').click()
await page.waitForTimeout(1600)
const toast = await page.getByTestId('zodo-toast').innerText().catch(() => '')
await page.screenshot({ path: path.join(outDir, 'agent-proposal-applied.png'), fullPage: true })
const finalText = await page.locator('[data-zodo-workbench="a2"]').innerText()
await writeFile(path.join(outDir, 'agent-approval.json'), JSON.stringify({
  listText,
  planText,
  toast,
  finalText,
  bridgeTraffic,
  consoleMessages,
  pageErrors,
}, null, 2))
console.log(JSON.stringify({
  hasAgentProposal: /delegated_agent|Schedule work within capacity|提案/.test(listText),
  applyButtonFound: true,
  planText: planText.slice(0, 2000),
  toast,
  finalHasApplied: /已应用|applied|r5/.test(finalText),
  errors: pageErrors,
  consoleErrors: consoleMessages.filter(x => x.type === 'error'),
  bridgeCalls: bridgeTraffic.map(row => ({ url: row.url, status: row.status, request: row.request?.slice(0, 200) })),
}, null, 2))
await browser.close()
