import path from 'node:path'
import { mkdir, writeFile } from 'node:fs/promises'
import { chromium } from 'playwright-core'

const url = process.env.ZODO_A0_URL
const outDir = path.resolve('artifacts/evidence/a0')
await mkdir(outDir, { recursive: true })
const browser = await chromium.launch({
  executablePath: 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
  headless: true,
})
const page = await (await browser.newContext({ viewport: { width: 1440, height: 1000 } })).newPage()
const reqs = []
page.on('request', request => {
  if (request.url().includes('/api/')) reqs.push(`${request.method()} ${request.url()}`)
})
await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 60_000 })
await page.waitForTimeout(2500)
const row = page.locator('[data-row-key^="session:"]').first()
console.log('rowCount', await row.count(), 'selected', await row.getAttribute('aria-selected'), 'tabindex', await row.getAttribute('tabindex'))
console.log('box', await row.boundingBox())
await row.click({ force: true })
await page.waitForTimeout(1600)
console.log('URL1', page.url())
await page.screenshot({ path: path.join(outDir, 'open-session-click1.png'), fullPage: true })
await row.dblclick({ force: true })
await page.waitForTimeout(1600)
console.log('URL2', page.url())
await page.keyboard.press('Enter')
await page.waitForTimeout(1600)
console.log('URL3', page.url())
const body = await page.locator('body').innerText()
console.log('BODYLEN', body.length)
console.log('BODY', body.slice(0, 1200))
console.log('REQS', reqs.slice(-20))
await writeFile(path.join(outDir, 'open-session-debug.json'), JSON.stringify({ urls: [page.url()], body, reqs }, null, 2))
await browser.close()
