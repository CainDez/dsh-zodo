import path from 'node:path'
import { mkdir, writeFile } from 'node:fs/promises'
import { chromium } from 'playwright-core'

const url = process.env.ZODO_A2_URL
if (!url) throw new Error('ZODO_A2_URL is required')
const outDir = path.resolve('artifacts/evidence/a3')
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
const result = { pageErrors, consoleMessages }
try {
  await page.getByTestId('nav-models').click()
  await page.waitForTimeout(1200)
  const modelsText = await page.locator('[data-zodo-workbench="a2"]').innerText()
  result.modelsText = modelsText
  await page.screenshot({ path: path.join(outDir, 'a3-models.png'), fullPage: true })

  await page.getByTestId('nav-agents').click()
  await page.waitForTimeout(500)
  await page.getByTestId('revoke-agent').click()
  await page.waitForTimeout(800)
  result.agentRevokeText = await page.locator('[data-zodo-workbench="a2"]').innerText()
  await page.screenshot({ path: path.join(outDir, 'a3-agents-revoked.png'), fullPage: true })
  await page.getByRole('button', { name: '恢复 Agent' }).click()
  await page.waitForTimeout(700)
  result.agentRestoreText = await page.locator('[data-zodo-workbench="a2"]').innerText()
  await page.screenshot({ path: path.join(outDir, 'a3-agents-restored.png'), fullPage: true })

  await page.getByTestId('nav-runs').click()
  await page.waitForTimeout(700)
  const runsText = await page.locator('[data-zodo-workbench="a2"]').innerText()
  result.runsText = runsText
  await page.screenshot({ path: path.join(outDir, 'a3-runs.png'), fullPage: true })
} catch (error) {
  result.error = String(error)
  await page.screenshot({ path: path.join(outDir, 'a3-surfaces-failure.png'), fullPage: true })
  throw error
} finally {
  await writeFile(path.join(outDir, 'a3-surfaces.json'), JSON.stringify(result, null, 2))
}
console.log(JSON.stringify({
  modelsHasLocal: /本地 Ollama/.test(result.modelsText ?? ''),
  modelsHasCredential: /凭据|credential/.test(result.modelsText ?? ''),
  revokedMessage: /撤销|恢复|当前撤销通道/.test(result.agentRevokeText ?? ''),
  runsCount: (result.runsText?.match(/goal\.decompose/g) ?? []).length,
  errors: pageErrors,
  consoleErrors: consoleMessages.filter(x => x.type === 'error'),
}, null, 2))
await browser.close()
