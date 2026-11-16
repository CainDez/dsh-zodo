
import { chromium } from 'playwright-core'

const url = process.env.URL ?? 'http://127.0.0.1:59679'
const browser = await chromium.launch({ executablePath: 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe', headless: true })
const context = await browser.newContext({ viewport: { width: 1440, height: 1000 } })
const page = await context.newPage()
const logs = []
const traffic = []
page.on('console', m => logs.push({ type: m.type(), text: m.text().slice(0, 500) }))
page.on('pageerror', e => logs.push({ type: 'pageerror', text: String(e).slice(0, 800) }))
page.on('request', r => { if (r.url().includes('/zodo-bridge/')) traffic.push({ phase: 'req', method: r.method(), url: r.url(), origin: r.headers()['origin'] ?? null }) })
page.on('response', async r => { if (!r.url().includes('/zodo-bridge/')) return; let body; try { body = (await r.text()).slice(0, 300) } catch {} ; traffic.push({ phase: 'res', status: r.status(), url: r.url(), body }) })
await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 60000 })
await page.waitForTimeout(4000)
const info = await page.evaluate(() => ({ href: location.href, title: document.title, text: document.body.innerText.slice(0, 1500) }))
console.log(JSON.stringify({ info }, null, 2))
await page.screenshot({ path: 'D:/projects/zodo_dsh/.runtime/repro-1-start.png' })

const candidates = await page.evaluate(() => Array.from(document.querySelectorAll('button, [data-testid], [role=button]')).map(el => ({ tag: el.tagName, tid: el.getAttribute('data-testid'), text: (el.textContent ?? '').trim().slice(0, 40), title: el.getAttribute('title'), aria: el.getAttribute('aria-label') })).filter(x => /zodo/i.test(JSON.stringify(x))).slice(0, 30))
console.log('ZODO CANDIDATES', JSON.stringify(candidates, null, 2))

let clicked = false
try {
  const el = await page.$('[data-testid="zodo-open"]')
  if (el) { await el.click(); clicked = true }
} catch (e) { logs.push({ type: 'click-error', text: String(e) }) }
console.log('clicked composer dock:', clicked)
await page.waitForTimeout(5000)
await page.screenshot({ path: 'D:/projects/zodo_dsh/.runtime/repro-2-after-click.png' })
const info2 = await page.evaluate(() => ({ href: location.href, text: document.body.innerText.slice(0, 2500) }))
console.log(JSON.stringify({ info2 }, null, 2))
console.log('TRAFFIC', JSON.stringify(traffic, null, 2))
console.log('LOGS', JSON.stringify(logs.slice(-40), null, 2))
await browser.close()
