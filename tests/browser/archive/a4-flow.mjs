import path from 'node:path'
import { mkdir, writeFile } from 'node:fs/promises'
import { chromium } from 'playwright-core'

const url = process.env.ZODO_A2_URL
if (!url) throw new Error('ZODO_A2_URL is required')
const outDir = path.resolve('artifacts/evidence/a4')
await mkdir(outDir, { recursive: true })
const browser = await chromium.launch({
  executablePath: process.env.ZODO_CHROME ?? 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
  headless: true,
})
const context = await browser.newContext({ viewport: { width: 1440, height: 1050 }, acceptDownloads: true })
const page = await context.newPage()
const consoleMessages = []
const pageErrors = []
page.on('console', m => consoleMessages.push({ type: m.type(), text: m.text() }))
page.on('pageerror', e => pageErrors.push(String(e)))
const steps = []
async function step(name, action) {
  try {
    await action()
    await page.waitForTimeout(500)
    steps.push({ name, ok: true })
    await page.screenshot({ path: path.join(outDir, `a4-${String(steps.length).padStart(2, '0')}-${name}.png`), fullPage: true })
  } catch (error) {
    steps.push({ name, ok: false, error: String(error) })
    await page.screenshot({ path: path.join(outDir, `a4-fail-${name}.png`), fullPage: true })
    await writeFile(path.join(outDir, 'a4-flow-error.json'), JSON.stringify({ steps, body: await page.locator('body').innerText().catch(() => ''), consoleMessages, pageErrors }, null, 2))
    throw error
  }
}
await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 60_000 })
await page.waitForTimeout(2200)
for (const label of ['继续', '开始使用', '稍后', '关闭']) {
  const control = page.getByRole('button', { name: label, exact: true }).first()
  if (await control.count()) {
    await control.click({ timeout: 2000 }).catch(() => {})
    await page.waitForTimeout(250)
  }
}
const openSidebar = page.getByRole('button', { name: '打开右侧边栏' }).first()
if (await openSidebar.count()) await openSidebar.click().catch(() => {})
await page.waitForTimeout(400)
let guide = page.locator('button:has-text("Zodo 工作台")').first()
if (!(await guide.count())) {
  const newTab = page.getByRole('button', { name: '新标签页' }).first()
  if (await newTab.count()) await newTab.click().catch(() => {})
  await page.waitForTimeout(400)
  guide = page.locator('button:has-text("Zodo 工作台")').first()
}
await guide.click()
await page.waitForSelector('[data-zodo-workbench="a2"]', { timeout: 20_000 })
await page.waitForTimeout(900)

const habitTitle = `晨间写作 ${Date.now() % 1000}`
await step('habit-create', async () => {
  await page.getByTestId('nav-habits').click()
  await page.getByRole('button', { name: '新建习惯' }).click()
  const dialog = page.getByRole('dialog', { name: '新建习惯' })
  await dialog.getByLabel('习惯').fill(habitTitle)
  await dialog.getByLabel('每周目标次数').fill('5')
  await dialog.getByRole('button', { name: '保存' }).click()
  await page.getByTestId('zodo-toast').waitFor({ timeout: 10_000 })
})

await step('habit-log', async () => {
  const card = page.locator('.zw-card').filter({ hasText: habitTitle }).first()
  await card.getByRole('button', { name: '记录一次' }).click()
  const dialog = page.getByRole('dialog', { name: '记录习惯' })
  await dialog.getByLabel('本次分钟').fill('20')
  await dialog.getByRole('button', { name: '记录' }).click()
  await page.getByTestId('zodo-toast').waitFor({ timeout: 10_000 })
  const text = await page.locator('[data-zodo-workbench="a2"]').innerText()
  if (!/1\/5/.test(text)) throw new Error('habit progress did not update')
})

await step('saved-view-create', async () => {
  await page.getByTestId('nav-views').click()
  await page.getByRole('button', { name: '新建视图' }).click()
  const dialog = page.getByRole('dialog', { name: '新建保存视图' })
  await dialog.getByLabel('名称').fill('A4 习惯任务')
  await dialog.getByLabel('标签').fill('ai')
  await dialog.getByRole('button', { name: '保存' }).click()
  await page.getByTestId('zodo-toast').waitFor({ timeout: 10_000 })
  await page.getByRole('button', { name: '应用' }).first().click()
  await page.waitForTimeout(500)
  const text = await page.locator('[data-zodo-workbench="a2"]').innerText()
  if (!/筛选/.test(text)) throw new Error('saved view did not apply to tasks')
})

await step('milestone', async () => {
  await page.getByTestId('nav-projects').click()
  await page.getByRole('button', { name: '添加里程碑' }).first().click()
  const dialog = page.getByRole('dialog', { name: '添加里程碑' })
  await dialog.getByLabel('标题').fill('A4 里程碑验收')
  await dialog.getByLabel('截止日期').fill('2026-11-10')
  await dialog.getByRole('button', { name: '保存' }).click()
  await page.getByTestId('zodo-toast').waitFor({ timeout: 10_000 })
  const milestone = page.locator('.zw-milestone').filter({ hasText: 'A4 里程碑验收' }).first()
  await milestone.click()
  const edit = page.getByRole('dialog', { name: '编辑里程碑' })
  await edit.getByLabel('已完成').check()
  await edit.getByRole('button', { name: '保存' }).click()
  await page.getByTestId('zodo-toast').waitFor({ timeout: 10_000 })
})

await step('data-backup', async () => {
  await page.getByTestId('nav-data').click()
  await page.getByTestId('data-backup').click()
  await page.getByTestId('zodo-toast').waitFor({ timeout: 20_000 })
  const text = await page.locator('[data-zodo-workbench="a2"]').innerText()
  if (!/备份/.test(text)) throw new Error('data view missing backup')
})

await step('data-export', async () => {
  const downloadPromise = page.waitForEvent('download', { timeout: 20_000 })
  await page.getByTestId('data-export').click()
  const download = await downloadPromise
  const target = path.join(outDir, 'zodo-export.json')
  await download.saveAs(target)
})

await step('models-save', async () => {
  await page.getByTestId('nav-models').click()
  await page.waitForTimeout(900)
  await page.getByTestId('model-save').click()
  await page.getByTestId('zodo-toast').waitFor({ timeout: 10_000 })
})

const finalText = await page.locator('[data-zodo-workbench="a2"]').innerText()
const result = { steps, finalText: finalText.slice(0, 8000), consoleMessages, pageErrors }
await writeFile(path.join(outDir, 'a4-flow.json'), JSON.stringify(result, null, 2))
console.log(JSON.stringify({ steps, pageErrors, consoleErrors: consoleMessages.filter(x => x.type === 'error') }, null, 2))
await browser.close()
