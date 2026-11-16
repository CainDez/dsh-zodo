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
await page.waitForTimeout(1500)
const continueButton = page.getByRole('button', { name: '继续' })
if (await continueButton.count()) {
  await continueButton.first().click()
  await page.waitForTimeout(1500)
}
const newSession = page.getByRole('button', { name: '新建会话', exact: true }).first()
if (await newSession.count()) {
  await newSession.click().catch(() => {})
  await page.waitForTimeout(1500)
}
await page.screenshot({ path: path.join(outDir, 'after-onboarding.png'), fullPage: true })
const body = await page.locator('body').innerText()
const buttons = await page.locator('button').evaluateAll(nodes => nodes.map(node => ({
  text: node.innerText,
  testid: node.getAttribute('data-testid'),
  aria: node.getAttribute('aria-label'),
  title: node.getAttribute('title'),
})))
const boot = await page.evaluate(() => {
  const value = globalThis.__DSH_BOOT__
  if (!value) return null
  try {
    return JSON.parse(JSON.stringify(value))
  } catch {
    return { unreadable: true }
  }
})
const loaderKeys = await page.evaluate(() => Object.keys(globalThis.__ModuleLoader__ ?? {}))
await writeFile(path.join(outDir, 'after-onboarding.json'), JSON.stringify({ body, buttons, boot, loaderKeys, consoleMessages }, null, 2))
console.log(JSON.stringify({ buttons: buttons.filter(x => x.testid || /Zodo|新会话|继续/.test(x.text)), consoleMessages, boot: boot && Object.keys(boot), loaderKeys }, null, 2))
await browser.close()
