/**
 * A0 browser inspection. Drives the user's installed Chrome against an already-running Harness
 * instance and captures the page, not a mock. This is evidence tooling, not product code.
 */
import { mkdir, writeFile } from 'node:fs/promises'
import path from 'node:path'
import process from 'node:process'
import { chromium } from 'playwright-core'

const url = process.env.ZODO_A0_URL
if (!url) {
  console.error('ZODO_A0_URL is required')
  process.exit(2)
}
const outDir = process.env.ZODO_A0_EVIDENCE_DIR ?? path.resolve('artifacts/evidence/a0')
await mkdir(outDir, { recursive: true })
const chrome = process.env.ZODO_CHROME ?? 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe'
const browser = await chromium.launch({ executablePath: chrome, headless: true })
const context = await browser.newContext({ viewport: { width: 1440, height: 1000 } })
const page = await context.newPage()
const consoleMessages = []
const pageErrors = []
const failedRequests = []
page.on('console', message => consoleMessages.push({ type: message.type(), text: message.text() }))
page.on('pageerror', error => pageErrors.push(String(error)))
page.on('requestfailed', request => failedRequests.push({ url: request.url(), error: request.failure()?.errorText }))
await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 60_000 })
await page.waitForTimeout(4_000)
await page.screenshot({ path: path.join(outDir, 'initial-page.png'), fullPage: true })
const text = await page.locator('body').innerText().catch(() => '')
const buttons = await page.locator('button').evaluateAll(nodes => nodes.map(node => ({
  text: node.innerText,
  testid: node.getAttribute('data-testid'),
  aria: node.getAttribute('aria-label'),
  title: node.getAttribute('title'),
})))
const links = await page.locator('a').evaluateAll(nodes => nodes.map(node => ({ text: node.innerText, href: node.getAttribute('href') })))
await writeFile(path.join(outDir, 'initial-page.json'), JSON.stringify({
  url: page.url(),
  title: await page.title(),
  bodyText: text,
  buttons,
  links,
  consoleMessages,
  pageErrors,
  failedRequests,
}, null, 2))
console.log(JSON.stringify({ url: page.url(), title: await page.title(), buttons: buttons.slice(0, 20), pageErrors, failedCount: failedRequests.length, outDir }, null, 2))
await browser.close()
