/**
 * A2 manual/no-model product surface: goals, projects, task dependency, execution feedback, focus,
 * review, reminder intent, recurrence, busy snapshot, search, audit and inbox conversion.
 */
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
page.on('console', message => consoleMessages.push({ type: message.type(), text: message.text() }))
page.on('pageerror', error => pageErrors.push(String(error)))
const steps = []
async function step(name, action, screenshot = true) {
  try {
    await action()
    await page.waitForTimeout(400)
    steps.push({ name, ok: true })
    if (screenshot) await page.screenshot({ path: path.join(outDir, `manual-${String(steps.length).padStart(2, '0')}-${name}.png`), fullPage: true })
  } catch (error) {
    steps.push({ name, ok: false, error: String(error) })
    await page.screenshot({ path: path.join(outDir, `manual-fail-${name}.png`), fullPage: true })
    await writeFile(path.join(outDir, 'manual-flow-error.json'), JSON.stringify({ steps, body: await page.locator('body').innerText().catch(() => ''), consoleMessages, pageErrors }, null, 2))
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

await step('goal-create', async () => {
  await page.getByTestId('nav-goals').click()
  await page.getByRole('button', { name: '新建目标' }).click()
  const dialog = page.getByRole('dialog', { name: '新建目标' })
  await dialog.getByLabel('目标').fill('完成作品集第二版')
  await dialog.getByLabel('成功标准').fill('精选 3 个项目，完成案例说明与在线展示')
  await dialog.getByLabel('期望完成').fill('2026-10-31')
  await dialog.getByRole('button', { name: '保存' }).click()
  await page.getByTestId('zodo-toast').waitFor({ timeout: 10_000 })
})

await step('project-create', async () => {
  await page.getByTestId('nav-projects').click()
  await page.getByRole('button', { name: '新建项目' }).click()
  const dialog = page.getByRole('dialog', { name: '新建项目' })
  await dialog.getByLabel('项目标题').fill('作品集第二版')
  await dialog.getByLabel('范围').fill('素材、案例说明、上线检查')
  await dialog.getByLabel('所属目标').selectOption({ label: '完成作品集第二版' })
  await dialog.getByRole('button', { name: '保存' }).click()
  await page.getByTestId('zodo-toast').waitFor({ timeout: 10_000 })
})

await step('task-create-second', async () => {
  await page.getByTestId('nav-tasks').click()
  await page.getByRole('button', { name: '新建任务' }).click()
  const dialog = page.getByRole('dialog', { name: '新建任务' })
  await dialog.getByLabel('标题').fill('校对案例说明')
  await dialog.getByLabel('预计分钟').fill('45')
  await dialog.getByLabel('所属项目').selectOption({ label: '作品集第二版' })
  await dialog.getByTestId('task-save').click()
  await page.waitForTimeout(500)
  await page.locator('[data-testid^="task-row-"]').filter({ hasText: '校对案例说明' }).first().click()
})

await step('dependency-add', async () => {
  await page.getByRole('button', { name: '添加依赖' }).click()
  const dialog = page.getByRole('dialog', { name: '添加依赖' })
  await dialog.getByLabel('前置任务').selectOption({ label: '筛选 3 个项目素材' })
  await dialog.getByRole('button', { name: '添加' }).click()
  await page.getByTestId('zodo-toast').waitFor({ timeout: 10_000 })
})

await step('progress-record', async () => {
  await page.getByRole('button', { name: '记录进度' }).click()
  const dialog = page.getByRole('dialog', { name: '记录本次进度' })
  await dialog.getByLabel('本次投入分钟').fill('10')
  await dialog.getByLabel('更新剩余分钟（可选）').fill('35')
  await dialog.getByLabel('备注（可选）').fill('校对前两页')
  await dialog.getByRole('button', { name: '记录' }).click()
  await page.getByTestId('zodo-toast').waitFor({ timeout: 10_000 })
})

await step('focus-start-finish', async () => {
  await page.getByRole('button', { name: '开始专注' }).click()
  await page.getByTestId('nav-focus').click()
  await page.getByTestId('focus-finish').click()
  await page.getByTestId('zodo-toast').waitFor({ timeout: 10_000 })
})

await step('reminder-create', async () => {
  await page.getByTestId('nav-tasks').click()
  await page.locator('[data-testid^="task-row-"]').filter({ hasText: '校对案例说明' }).first().click()
  await page.getByRole('button', { name: '设置提醒' }).click()
  const dialog = page.getByRole('dialog', { name: '设置提醒意图' })
  await dialog.getByRole('button', { name: '保存' }).click()
  await page.getByTestId('zodo-toast').waitFor({ timeout: 10_000 })
})

await step('recurrence-create', async () => {
  await page.getByRole('button', { name: '新建循环' }).click()
  const dialog = page.getByRole('dialog', { name: '新建循环工作' })
  await dialog.getByLabel('标题').fill('每周整理案例素材')
  await dialog.getByRole('button', { name: '生成实例' }).click()
  await page.getByTestId('zodo-toast').waitFor({ timeout: 10_000 })
})

await step('busy-import', async () => {
  await page.getByTestId('nav-schedule').click()
  await page.getByRole('button', { name: '导入忙碌' }).click()
  const dialog = page.getByRole('dialog', { name: '导入忙碌快照' })
  await dialog.getByRole('button', { name: '导入' }).click()
  await page.getByTestId('zodo-toast').waitFor({ timeout: 10_000 })
})

await step('search', async () => {
  await page.getByTestId('nav-search').click()
  await page.getByPlaceholder('搜索任务、目标、项目…').fill('案例')
  await page.getByRole('button', { name: '搜索', exact: true }).click()
  await page.waitForTimeout(500)
})

await step('review-build', async () => {
  await page.getByTestId('nav-review').click()
  await page.getByRole('button', { name: '生成今日回顾' }).click()
  await page.getByTestId('zodo-toast').waitFor({ timeout: 10_000 })
})

await step('audit', async () => {
  await page.getByTestId('nav-audit').click()
  await page.waitForTimeout(800)
  const text = await page.locator('[data-zodo-workbench="a2"]').innerText()
  if (!/捕获|capture\.collect|task\.create/.test(text)) throw new Error('audit entries did not render')
})

await step('privacy-settings-view', async () => {
  await page.getByTestId('nav-settings').click()
  await page.waitForTimeout(300)
  await page.getByTestId('nav-privacy').click()
  await page.waitForTimeout(300)
  const text = await page.locator('[data-zodo-workbench="a2"]').innerText()
  if (!/local_only|仅本地|device_only/.test(text)) throw new Error('privacy view missing policy')
})

const finalText = await page.locator('[data-zodo-workbench="a2"]').innerText()
const toast = await page.getByTestId('zodo-toast').innerText().catch(() => '')
const result = { steps, finalText: finalText.slice(0, 6000), toast, consoleMessages, pageErrors }
await writeFile(path.join(outDir, 'manual-flow.json'), JSON.stringify(result, null, 2))
console.log(JSON.stringify({ steps, pageErrors, consoleErrors: consoleMessages.filter(x => x.type === 'error'), finalHasReview: /专注|回顾/.test(finalText) }, null, 2))
await browser.close()
