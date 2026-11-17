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
await page.waitForTimeout(2000)
for (const label of ['继续', '开始使用', '稍后', '关闭']) {
  const control = page.getByRole('button', { name: label, exact: true }).first()
  if (await control.count()) {
    await control.click({ timeout: 2000 }).catch(() => {})
    await page.waitForTimeout(400)
  }
}
const model = page.getByRole('button', { name: '请选择模型' }).first()
if (await model.count()) {
  await model.click().catch(() => {})
  await page.waitForTimeout(800)
}
await page.screenshot({ path: path.join(outDir, 'model-menu.png'), fullPage: true })
const text = await page.locator('body').innerText()
const controls = await page.locator('button, [role="menuitem"], [role="option"], [role="dialog"]').evaluateAll(nodes => nodes.map(node => ({
  tag: node.tagName,
  role: node.getAttribute('role'),
  text: node.innerText?.slice(0, 200),
  testid: node.getAttribute('data-testid'),
  aria: node.getAttribute('aria-label'),
})))
await writeFile(path.join(outDir, 'model-menu.json'), JSON.stringify({ text, controls, consoleMessages }, null, 2))
console.log(JSON.stringify({ text: text.slice(0, 2500), controls: controls.filter(c => c.text || c.aria).slice(-40), consoleMessages }, null, 2))
await browser.close()
