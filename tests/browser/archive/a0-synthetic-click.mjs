import { chromium } from 'playwright-core'

const browser = await chromium.launch({
  executablePath: 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
  headless: true,
})
const page = await (await browser.newContext({ viewport: { width: 1440, height: 1000 } })).newPage()
const reqs = []
page.on('request', request => {
  if (request.url().includes('/api/')) reqs.push(request.url())
})
await page.goto(process.env.ZODO_A0_URL, { waitUntil: 'domcontentloaded', timeout: 60_000 })
await page.waitForTimeout(2500)
const row = page.locator('[data-row-key^="session:"]').first()
console.log('count', await row.count())
await row.evaluate(el => {
  el.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }))
  el.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }))
  el.dispatchEvent(new MouseEvent('mouseup', { bubbles: true }))
  el.dispatchEvent(new MouseEvent('click', { bubbles: true }))
})
await page.waitForTimeout(2000)
console.log('body', (await page.locator('body').innerText()).slice(0, 800))
console.log('reqs', reqs.slice(-15))
await page.screenshot({ path: 'artifacts/evidence/a0/session-synthetic-click.png', fullPage: true })
await browser.close()
