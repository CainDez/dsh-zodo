import path from 'node:path'
import { mkdir, writeFile } from 'node:fs/promises'
import { chromium } from 'playwright-core'

const url = process.env.ZODO_A2_URL
const outDir = path.resolve('artifacts/evidence/a2')
await mkdir(outDir, { recursive: true })
const browser = await chromium.launch({
  executablePath: process.env.ZODO_CHROME ?? 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
  headless: true,
})
const page = await (await browser.newContext({ viewport: { width: 1440, height: 1050 } })).newPage()
const consoleMessages = []
const pageErrors = []
page.on('console', m => consoleMessages.push({ type: m.type(), text: m.text() }))
page.on('pageerror', e => pageErrors.push(String(e)))
const steps = []
async function step(name, action) {
  try {
    await action()
    await page.waitForTimeout(450)
    steps.push({ name, ok: true })
    await page.screenshot({ path: path.join(outDir, `inbox-${String(steps.length).padStart(2, '0')}-${name}.png`), fullPage: true })
  } catch (error) {
    steps.push({ name, ok: false, error: String(error) })
    await page.screenshot({ path: path.join(outDir, `inbox-fail-${name}.png`), fullPage: true })
    await writeFile(path.join(outDir, 'inbox-flow-error.json'), JSON.stringify({ steps, body: await page.locator('body').innerText().catch(() => ''), consoleMessages, pageErrors }, null, 2))
    throw error
  }
}
await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 60_000 })
await page.waitForTimeout(2000)
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
await page.waitForTimeout(800)

const captureText = `明天下午联系印刷店并确认封面 ${Date.now() % 10000}`
await step('capture', async () => {
  await page.getByTestId('top-capture').click()
  const dialog = page.getByRole('dialog', { name: '快速收集' })
  await dialog.locator('textarea').fill(captureText)
  await dialog.getByRole('button', { name: '保存原文' }).click()
  await page.getByTestId('zodo-toast').waitFor({ timeout: 10_000 })
})

await step('inbox-detail-clarify', async () => {
  await page.getByTestId('nav-inbox').click()
  await page.waitForTimeout(300)
  await page.locator('[data-testid^="inbox-row-"]').filter({ hasText: captureText }).first().click()
  const dialog = page.getByRole('dialog', { name: '收集详情' })
  await dialog.getByLabel('问题').fill('希望在几点前完成？')
  await dialog.getByLabel('回答（可稍后）').fill('下午六点前')
  await dialog.getByRole('button', { name: '保存澄清' }).click()
  await page.waitForTimeout(700)
  await dialog.locator('button[aria-label="关闭"]').click()
})

await step('inbox-convert', async () => {
  await page.locator('[data-testid^="inbox-row-"]').filter({ hasText: captureText }).first().click()
  const detail = page.getByRole('dialog', { name: '收集详情' })
  await detail.getByRole('button', { name: '转换…' }).click()
  const dialog = page.getByRole('dialog', { name: '处理收集内容' })
  await dialog.getByLabel('任务标题').fill('联系印刷店确认封面')
  await dialog.getByLabel('预计分钟').fill('15')
  await dialog.getByRole('button', { name: '提交转换' }).click()
  await page.getByTestId('zodo-toast').waitFor({ timeout: 10_000 })
})

await step('goal-achieve', async () => {
  await page.getByTestId('nav-goals').click()
  await page.getByRole('button', { name: '标记达成' }).first().click()
  const dialog = page.getByRole('dialog', { name: '标记目标达成' })
  await dialog.getByLabel('达成证据').fill('演示证据：三个案例已在线发布')
  await dialog.getByRole('button', { name: '确认达成' }).click()
  await page.getByTestId('zodo-toast').waitFor({ timeout: 10_000 })
})

await step('task-complete', async () => {
  await page.getByTestId('nav-tasks').click()
  await page.locator('[data-testid^="task-row-"]').filter({ hasText: '联系印刷店确认封面' }).first().click()
  await page.getByRole('button', { name: '标记完成' }).click()
  await page.getByTestId('zodo-toast').waitFor({ timeout: 10_000 })
})

const finalText = await page.locator('[data-zodo-workbench="a2"]').innerText()
const result = { steps, finalText: finalText.slice(0, 8000), consoleMessages, pageErrors }
await writeFile(path.join(outDir, 'inbox-flow.json'), JSON.stringify(result, null, 2))
console.log(JSON.stringify({ steps, pageErrors, consoleErrors: consoleMessages.filter(x => x.type === 'error') }, null, 2))
await browser.close()
