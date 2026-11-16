import path from 'node:path'
import { mkdir, writeFile } from 'node:fs/promises'
import { chromium } from 'playwright-core'

const url = process.env.ZODO_A0_URL
const outDir = process.env.ZODO_A0_EVIDENCE_DIR ?? path.resolve('artifacts/evidence/a0')
await mkdir(outDir, { recursive: true })
const browser = await chromium.launch({
  executablePath: process.env.ZODO_CHROME ?? 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
  headless: true,
})
const page = await (await browser.newContext({ viewport: { width: 1440, height: 1000 } })).newPage()
const consoleMessages = []
page.on('console', m => consoleMessages.push({ type: m.type(), text: m.text() }))
await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 60_000 })
await page.waitForTimeout(2200)
for (const label of ['继续', '开始使用', '稍后', '关闭']) {
  const control = page.getByRole('button', { name: label, exact: true }).first()
  if (await control.count()) {
    await control.click({ timeout: 2000 }).catch(() => {})
    await page.waitForTimeout(300)
  }
}
const session = page.locator('[data-row-key^="session:"]').first()
if (await session.count()) {
  await session.click().catch(() => {})
  await page.waitForTimeout(2200)
}
await page.screenshot({ path: path.join(outDir, 'native-session-view.png'), fullPage: true })
let body = await page.locator('body').innerText().catch(() => '')
if (!/pong|zodo_probe|zodo-a0-bridge/.test(body)) {
  // The tool card may be collapsed; open the first tool row if present.
  const rows = page.locator('button:has-text("zodo_probe"), [role="button"]:has-text("zodo_probe")')
  if (await rows.count()) {
    await rows.first().click().catch(() => {})
    await page.waitForTimeout(900)
    await page.screenshot({ path: path.join(outDir, 'native-session-expanded.png'), fullPage: true })
    body = await page.locator('body').innerText().catch(() => '')
  }
}
const buttons = await page.locator('button').evaluateAll(nodes => nodes.map(n => ({ text: n.innerText, aria: n.getAttribute('aria-label') })))
await writeFile(path.join(outDir, 'native-session-view.json'), JSON.stringify({ body, buttons, consoleMessages }, null, 2))
console.log(JSON.stringify({ hasPong: /pong/.test(body), hasBridge: /zodo-a0-bridge\/1/.test(body), bodyTail: body.slice(-3000), consoleMessages }, null, 2))
await browser.close()
