/**
 * A2 native-tool path: ask the real local Harness model to read Zodo tasks and submit a deterministic
 * planning proposal through the Host adapter. The agent never approves or commits.
 */
import path from 'node:path'
import { mkdir, writeFile } from 'node:fs/promises'
import { chromium } from 'playwright-core'

const url = process.env.ZODO_A2_URL
if (!url) {
  console.error('ZODO_A2_URL is required')
  process.exit(2)
}
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
await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 60_000 })
await page.waitForTimeout(2200)
for (const label of ['继续', '开始使用', '稍后', '关闭']) {
  const control = page.getByRole('button', { name: label, exact: true }).first()
  if (await control.count()) {
    await control.click({ timeout: 2000 }).catch(() => {})
    await page.waitForTimeout(300)
  }
}
const input = page.locator('textarea, [contenteditable="true"]').first()
if (!(await input.count())) throw new Error('composer input not found')
await input.click()
await page.keyboard.type(
  '请按顺序调用两个 Zodo 原生工具：先用 zodo_list_tasks 查看当前任务；再用返回的任务 id 调用 zodo_plan_propose，horizon_start 为 2026-10-25T00:00:00Z，horizon_end 为 2026-11-02T00:00:00Z。不要创建任务，不要批准或提交，完成后只回复 done。',
  { delay: 4 },
)
await page.keyboard.press('Enter')
const started = Date.now()
let body = ''
let stableIdle = 0
while (Date.now() - started < 300_000) {
  await page.waitForTimeout(2000)
  body = await page.locator('body').innerText().catch(() => '')
  if (!/深度求索中|进行中/.test(body)) {
    stableIdle += 1
    if (stableIdle >= 2) break
  } else {
    stableIdle = 0
  }
}
await page.waitForTimeout(800)
body = await page.locator('body').innerText().catch(() => '')
await page.screenshot({ path: path.join(outDir, 'native-tools-chat.png'), fullPage: true })
await writeFile(path.join(outDir, 'native-tools-chat.json'), JSON.stringify({
  body,
  consoleMessages,
  pageErrors,
}, null, 2))
console.log(JSON.stringify({
  bodyTail: body.slice(-4000),
  pageErrors,
  consoleErrors: consoleMessages.filter(x => x.type === 'error'),
}, null, 2))
await browser.close()
