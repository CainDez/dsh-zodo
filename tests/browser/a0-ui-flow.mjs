/**
 * A0 real-host UI round trip: open the Zodo tab, mint the UI bridge session, ping the Rust probe,
 * echo, start an asynchronous operation, cancel it, and capture the actual rendered states.
 */
import path from 'node:path'
import { mkdir, writeFile } from 'node:fs/promises'
import { chromium } from 'playwright-core'

const url = process.env.ZODO_A0_URL
if (!url) {
  console.error('ZODO_A0_URL is required')
  process.exit(2)
}
const outDir = process.env.ZODO_A0_EVIDENCE_DIR ?? path.resolve('artifacts/evidence/a0')
await mkdir(outDir, { recursive: true })
const browser = await chromium.launch({
  executablePath: process.env.ZODO_CHROME ?? 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
  headless: true,
})
const context = await browser.newContext({ viewport: { width: 1440, height: 1000 } })
const page = await context.newPage()
const consoleMessages = []
const pageErrors = []
const bridgeTraffic = []
page.on('console', m => consoleMessages.push({ type: m.type(), text: m.text() }))
page.on('pageerror', e => pageErrors.push(String(e)))
page.on('response', async response => {
  if (!response.url().includes('/zodo-bridge/')) return
  let body
  try {
    body = await response.text()
  } catch {
    body = undefined
  }
  bridgeTraffic.push({
    url: response.url(),
    status: response.status(),
    request: response.request().postData(),
    body: body?.slice(0, 4000),
  })
})
await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 60_000 })
await page.waitForTimeout(2000)
for (const label of ['继续', '开始使用', '稍后', '关闭']) {
  const control = page.getByRole('button', { name: label, exact: true }).first()
  if (await control.count()) {
    await control.click({ timeout: 2000 }).catch(() => {})
    await page.waitForTimeout(400)
  }
}
const openSidebar = page.getByRole('button', { name: '打开右侧边栏' }).first()
if (await openSidebar.count()) {
  await openSidebar.click().catch(() => {})
  await page.waitForTimeout(700)
}
let guide = page.locator('button:has-text("Zodo 桥接验证")').first()
if (!(await guide.count())) {
  const newTab = page.getByRole('button', { name: '新标签页' }).first()
  if (await newTab.count()) await newTab.click().catch(() => {})
  await page.waitForTimeout(500)
  guide = page.locator('button:has-text("Zodo 桥接验证")').first()
}
if (!(await guide.count())) {
  await page.screenshot({ path: path.join(outDir, 'zodo-guide-missing.png'), fullPage: true })
  await writeFile(path.join(outDir, 'zodo-guide-missing.txt'), await page.locator('body').innerText())
  throw new Error('Zodo guide entry did not appear in the live right Sidebar')
}
await guide.click({ timeout: 10000 })
await page.waitForSelector('[data-testid="zodo-a0-panel"]', { timeout: 15_000 })
await page.screenshot({ path: path.join(outDir, 'zodo-tab-initial.png'), fullPage: true })

const status = page.locator('[data-testid="zodo-a0-status"]')
await status.waitFor({ timeout: 15_000 })
await page.waitForFunction(() => {
  const el = document.querySelector('[data-testid="zodo-a0-status"]')
  return el && el.textContent && (el.textContent.includes('桥接可用') || el.textContent.includes('桥接异常'))
}, undefined, { timeout: 20_000 }).catch(() => {})

const initialStatus = await status.innerText().catch(() => '')
const initialResult = await page.locator('[data-testid="zodo-a0-result"]').innerText().catch(() => '')
await page.screenshot({ path: path.join(outDir, 'zodo-tab-ping.png'), fullPage: true })

// Echo round trip.
await page.locator('[data-testid="zodo-a0-echo-input"]').fill('A0 回显：你好 Zodo')
await page.locator('[data-testid="zodo-a0-echo"]').click()
await page.waitForTimeout(800)
const echoResult = await page.locator('[data-testid="zodo-a0-result"]').innerText().catch(() => '')
await page.screenshot({ path: path.join(outDir, 'zodo-tab-echo.png'), fullPage: true })

// Start a long operation, then cancel while it is in flight.
await page.locator('[data-testid="zodo-a0-sleep"]').click()
await page.waitForTimeout(500)
const pendingStatus = await page.locator('[data-testid="zodo-a0-result"]').innerText().catch(() => '')
await page.locator('[data-testid="zodo-a0-cancel"]').click()
await page.waitForTimeout(1000)
const cancelResult = await page.locator('[data-testid="zodo-a0-result"]').innerText().catch(() => '')
const finalStatus = await status.innerText().catch(() => '')
await page.screenshot({ path: path.join(outDir, 'zodo-tab-cancel.png'), fullPage: true })

const audit = {
  panelPresent: await page.locator('[data-testid="zodo-a0-panel"]').count() > 0,
  initialStatus,
  initialResult,
  echoResult,
  pendingStatus,
  cancelResult,
  finalStatus,
  bridgeTraffic,
  consoleMessages,
  pageErrors,
}
await writeFile(path.join(outDir, 'zodo-ui-flow.json'), JSON.stringify(audit, null, 2))
console.log(JSON.stringify({
  panelPresent: audit.panelPresent,
  initialStatus,
  initialResult: initialResult.slice(0, 800),
  echoResult: echoResult.slice(0, 800),
  pendingStatus: pendingStatus.slice(0, 400),
  cancelResult: cancelResult.slice(0, 800),
  finalStatus,
  bridgeTraffic: bridgeTraffic.map(x => ({ url: x.url, status: x.status, request: x.request })),
  pageErrors,
  consoleErrors: consoleMessages.filter(x => x.type === 'error'),
}, null, 2))
await browser.close()
