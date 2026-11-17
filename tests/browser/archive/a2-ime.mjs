import path from 'node:path'
import { mkdir, writeFile } from 'node:fs/promises'
import { chromium } from 'playwright-core'

const out = path.resolve('artifacts/evidence/a4')
await mkdir(out, { recursive: true })
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
await page.getByRole('button', { name: '收集', exact: true }).click()
await page.waitForTimeout(300)
const input = page.getByTestId('capture-input')
await input.fill('中文输入法组词测试')
const composing = await page.evaluate(() => {
  const area = document.querySelector('[data-testid="capture-input"]')
  if (!area) return false
  const event = new KeyboardEvent('keydown', {
    key: 'Enter', code: 'Enter', ctrlKey: true, bubbles: true, cancelable: true, isComposing: true,
  })
  area.dispatchEvent(event)
  return true
})
await page.waitForTimeout(600)
const dialogAfterComposing = await page.getByRole('dialog').count()
await page.screenshot({ path: path.join(out, 'h25-ime-composing.png'), fullPage: true })
await page.evaluate(() => {
  const area = document.querySelector('[data-testid="capture-input"]')
  if (!area) return
  area.dispatchEvent(new KeyboardEvent('keydown', {
    key: 'Enter', code: 'Enter', ctrlKey: true, bubbles: true, cancelable: true, isComposing: false,
  }))
})
await page.waitForTimeout(900)
const dialogAfterCommit = await page.getByRole('dialog').count()
const toast = await page.getByTestId('zodo-toast').innerText().catch(() => '')
await page.screenshot({ path: path.join(out, 'h25-ime-committed.png'), fullPage: true })
const result = { composingDispatched: composing, dialogAfterComposing, dialogAfterCommit, toast, errors }
await writeFile(path.join(out, 'h25-ime.json'), JSON.stringify(result, null, 2))
console.log(JSON.stringify(result, null, 2))
await browser.close()
