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
const page = await (await browser.newContext({ viewport: { width: 1440, height: 1000 } })).newPage()
const consoleMessages = []
const pageErrors = []
const bridgeTraffic = []
page.on('console', message => consoleMessages.push({ type: message.type(), text: message.text() }))
page.on('pageerror', error => pageErrors.push(String(error)))
page.on('response', async response => {
  if (response.url().includes('/zodo-bridge/')) {
    bridgeTraffic.push({ url: response.url(), status: response.status(), request: response.request().postData() })
  }
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
if (await openSidebar.count()) {
  await openSidebar.click().catch(() => {})
  await page.waitForTimeout(500)
}
let guide = page.locator('button:has-text("Zodo 工作台")').first()
if (!(await guide.count())) {
  const newTab = page.getByRole('button', { name: '新标签页' }).first()
  if (await newTab.count()) await newTab.click().catch(() => {})
  await page.waitForTimeout(500)
  guide = page.locator('button:has-text("Zodo 工作台")').first()
}
if (!(await guide.count())) {
  await page.screenshot({ path: path.join(outDir, 'a2-guide-missing.png'), fullPage: true })
  await writeFile(path.join(outDir, 'a2-guide-missing.txt'), await page.locator('body').innerText())
  throw new Error('Zodo guide not found')
}
await guide.click()
await page.waitForTimeout(3500)
await page.screenshot({ path: path.join(outDir, 'a2-initial.png'), fullPage: true })
const body = await page.locator('body').innerText().catch(() => '')
const errors = await page.locator('[data-testid="zodo-error"]').allInnerTexts().catch(() => [])
const toast = await page.locator('[data-testid="zodo-toast"]').allInnerTexts().catch(() => [])
const buttons = await page.locator('button').evaluateAll(nodes => nodes.map(node => ({
  text: node.innerText,
  testid: node.getAttribute('data-testid'),
  aria: node.getAttribute('aria-label'),
})).filter(row => row.testid || row.text?.trim()))
await writeFile(path.join(outDir, 'a2-initial.json'), JSON.stringify({ body, errors, toast, buttons, consoleMessages, pageErrors, bridgeTraffic }, null, 2))
console.log(JSON.stringify({ bodyTail: body.slice(-2000), errors, toast, bridgeTraffic, pageErrors, consoleErrors: consoleMessages.filter(x => x.type === 'error') }, null, 2))
await browser.close()
