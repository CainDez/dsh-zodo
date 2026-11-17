import path from 'node:path'
import { mkdir, writeFile } from 'node:fs/promises'
import { chromium } from 'playwright-core'

const url = process.env.ZODO_A2_URL
const out = path.resolve('artifacts/evidence/a4')
await mkdir(out, { recursive: true })
const browser = await chromium.launch({
  executablePath: process.env.ZODO_CHROME ?? 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
  headless: true,
})
const page = await (await browser.newContext({ viewport: { width: 1440, height: 1050 } })).newPage()
const errors = []
page.on('pageerror', e => errors.push(String(e)))
page.on('console', m => { if (m.type() === 'error') errors.push(m.text()) })
await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 60000 })
await page.waitForTimeout(2200)
const open = page.getByRole('button', { name: '打开右侧边栏' }).first()
if (await open.count()) await open.click().catch(() => {})
await page.waitForTimeout(400)
let guide = page.locator('button:has-text("Zodo 工作台")').first()
if (!(await guide.count())) {
  const tab = page.getByRole('button', { name: '新标签页' }).first()
  if (await tab.count()) await tab.click().catch(() => {})
  await page.waitForTimeout(400)
  guide = page.locator('button:has-text("Zodo 工作台")').first()
}
await guide.click()
await page.waitForSelector('[data-zodo-workbench="a2"]', { timeout: 20000 })
await page.waitForTimeout(800)
await page.getByTestId('nav-review').click()
await page.waitForTimeout(400)
await page.getByTestId('estimate-propose').click()
await page.waitForTimeout(1200)
const body = await page.locator('[data-zodo-workbench="a2"]').innerText()
await page.screenshot({ path: path.join(out, 'estimate-proposal.png'), fullPage: true })
const applied = page.getByRole('button', { name: '检查并批准' }).first()
let toast = ''
if (await applied.count()) {
  await applied.click()
  const dialog = page.getByRole('dialog')
  await dialog.waitFor({ timeout: 10000 })
  await dialog.getByTestId('plan-apply').click()
  await page.waitForTimeout(1200)
  toast = await page.getByTestId('zodo-toast').innerText().catch(() => '')
}
await writeFile(path.join(out, 'a4-estimate-adjust.json'), JSON.stringify({ body: body.slice(0, 4000), toast, errors }, null, 2))
console.log(JSON.stringify({ hasProposal: /估算调整/.test(body), applied: Boolean(toast), toast, errors }, null, 2))
await browser.close()
