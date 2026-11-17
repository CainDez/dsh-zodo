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
const pageErrors = []
page.on('console', m => consoleMessages.push({ type: m.type(), text: m.text() }))
page.on('pageerror', e => pageErrors.push(String(e)))
await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 60_000 })
await page.waitForTimeout(2500)
for (const name of ['继续', '开始使用', '稍后', '关闭']) {
  const button = page.getByRole('button', { name, exact: true }).first()
  if (await button.count()) {
    await button.click({ timeout: 2000 }).catch(() => {})
    await page.waitForTimeout(600)
  }
}
// Open the right sidebar if it is closed.
const openSidebar = page.getByRole('button', { name: '打开右侧边栏' }).first()
if (await openSidebar.count()) await openSidebar.click().catch(() => {})
await page.waitForTimeout(700)
// The add-tab control is the only button with aria-label 新标签页.
const newTab = page.getByRole('button', { name: '新标签页' }).first()
if (await newTab.count()) await newTab.click().catch(() => {})
await page.waitForTimeout(700)
await page.screenshot({ path: path.join(outDir, 'sidebar-menu.png'), fullPage: true })
const text = await page.locator('body').innerText()
const buttons = await page.locator('button').evaluateAll(nodes => nodes.map(node => ({
  text: node.innerText,
  testid: node.getAttribute('data-testid'),
  aria: node.getAttribute('aria-label'),
  title: node.getAttribute('title'),
})))
await writeFile(path.join(outDir, 'sidebar-menu.json'), JSON.stringify({ text, buttons, consoleMessages, pageErrors }, null, 2))
console.log(JSON.stringify({ hasZodoText: text.includes('Zodo'), buttons: buttons.filter(x => /Zodo|新标签|指南|工具/i.test(`${x.text} ${x.aria} ${x.title}`)), consoleMessages, pageErrors }, null, 2))
await browser.close()
