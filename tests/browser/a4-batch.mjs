import path from 'node:path'
import { mkdir, writeFile } from 'node:fs/promises'
import { chromium } from 'playwright-core'

const url = process.env.ZODO_A2_URL
const outDir = path.resolve('artifacts/evidence/a4')
await mkdir(outDir, { recursive: true })
const browser = await chromium.launch({
  executablePath: process.env.ZODO_CHROME ?? 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
  headless: true,
})
const page = await (await browser.newContext({ viewport: { width: 1440, height: 1050 } })).newPage()
const dialogs = []
page.on('dialog', async dialog => { dialogs.push(dialog.message()); await dialog.accept() })
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
await page.getByTestId('nav-tasks').click()
await page.waitForTimeout(600)
const boxes = page.locator('input.zw-select')
const count = await boxes.count()
if (count < 2) throw new Error('not enough tasks for batch test')
await boxes.nth(0).check()
await boxes.nth(1).check()
await page.screenshot({ path: path.join(outDir, 'batch-selected.png'), fullPage: true })
await page.getByRole('button', { name: '批量完成' }).click()
await page.waitForTimeout(1500)
const toast = await page.getByTestId('zodo-toast').innerText().catch(() => '')
await page.screenshot({ path: path.join(outDir, 'batch-applied.png'), fullPage: true })
const result = { dialogs, toast, selectedBefore: 2, taskRows: await page.locator('[data-testid^="task-row-"]').count() }
await writeFile(path.join(outDir, 'a4-batch.json'), JSON.stringify(result, null, 2))
console.log(JSON.stringify(result, null, 2))
await browser.close()
