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
const context = await browser.newContext({ viewport: { width: 1440, height: 1050 } })
const page = await context.newPage()
const errors = []
page.on('pageerror', error => errors.push(String(error)))
page.on('console', message => { if (message.type() === 'error') errors.push(message.text()) })

async function openWorkbench() {
  await page.waitForTimeout(1800)
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
  await page.waitForTimeout(600)
}

await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 60000 })
await openWorkbench()
await page.getByRole('button', { name: '收集', exact: true }).click()
await page.waitForTimeout(300)
const text = `离线草稿测试 ${Date.now()}`
await page.getByTestId('capture-input').fill(text)
await page.route('**/zodo-bridge/v1/rpc', async route => {
  const body = route.request().postData() ?? ''
  if (body.includes('capture.collect')) return route.abort()
  return route.continue()
})
await page.getByTestId('capture-save').click()
await page.waitForTimeout(800)
const offlineMessage = await page.locator('.zw-alert-error').innerText().catch(() => '')
const draftAfterFailure = await page.evaluate(() => window.localStorage.getItem('zodo.capture.draft.v1'))
await page.screenshot({ path: path.join(out, 'dat05-offline-draft.png'), fullPage: true })

await page.reload({ waitUntil: 'domcontentloaded' })
await openWorkbench()
await page.getByRole('button', { name: '收集', exact: true }).click()
await page.waitForTimeout(400)
const restored = await page.getByTestId('capture-input').inputValue()
await page.unroute('**/zodo-bridge/v1/rpc')
await page.getByTestId('capture-save').click()
await page.waitForTimeout(900)
const draftAfterSuccess = await page.evaluate(() => window.localStorage.getItem('zodo.capture.draft.v1'))
const toast = await page.getByTestId('zodo-toast').innerText().catch(() => '')
await page.screenshot({ path: path.join(out, 'dat05-reconnected.png'), fullPage: true })
const result = {
  offlineMessage,
  draftAfterFailure,
  restored,
  text,
  draftAfterSuccess,
  toast,
  committed: draftAfterSuccess === null && !offlineMessage.includes('已保存'),
  errors,
}
await writeFile(path.join(out, 'ui-dat05-offline-draft.json'), JSON.stringify(result, null, 2))
console.log(JSON.stringify({ ...result, text: text.slice(0, 40), draftAfterFailure: Boolean(draftAfterFailure), restoredMatches: restored === text }, null, 2))
await browser.close()
