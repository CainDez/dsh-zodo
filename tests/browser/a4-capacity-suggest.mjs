import { chromium } from 'playwright-core'

// Validation flow for PLN-15: read-only capacity candidates, explicit create.
const browser = await chromium.launch({
  executablePath: process.env.ZODO_CHROME ?? 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
  headless: true,
})
const page = await (await browser.newContext({ viewport: { width: 1440, height: 1050 } })).newPage()
const errors = []
page.on('pageerror', error => errors.push(String(error)))
page.on('console', message => { if (message.type() === 'error') errors.push(message.text()) })
await page.goto(process.env.ZODO_A2_URL, { waitUntil: 'domcontentloaded', timeout: 60000 })
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
await page.waitForTimeout(700)
await page.getByTestId('nav-schedule').click()
await page.waitForTimeout(400)
await page.getByTestId('capacity-suggest').click()
await page.waitForTimeout(900)
const before = await page.locator('[data-testid^="capacity-create-"]').count()
await page.screenshot({ path: 'artifacts/evidence/a4/pln15-candidates.png', fullPage: true })
if (before > 0) {
  await page.locator('[data-testid^="capacity-create-"]').first().click()
  await page.waitForTimeout(900)
}
const after = await page.locator('[data-testid^="capacity-create-"]').count()
const toast = await page.getByTestId('zodo-toast').innerText().catch(() => '')
const errorText = await page.locator('.zw-alert-error').allInnerTexts().catch(() => [])
await page.screenshot({ path: 'artifacts/evidence/a4/pln15-created.png', fullPage: true })
console.log(JSON.stringify({ before, after, toast, errorText, errors }, null, 2))
await browser.close()
