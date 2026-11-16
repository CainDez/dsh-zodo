/**
 * A0 native-tool evidence: drive one real Host agent session to call `zodo_probe`, approve the Host
 * tool gate if the profile asks, and capture the rendered result plus the session-log tool events.
 * This exercises the actual model/provider configured on the isolated profile; it is not a stub.
 */
import path from 'node:path'
import { mkdir, writeFile, readdir, readFile } from 'node:fs/promises'
import { chromium } from 'playwright-core'

const url = process.env.ZODO_A0_URL
if (!url) {
  console.error('ZODO_A0_URL is required')
  process.exit(2)
}
const outDir = process.env.ZODO_A0_EVIDENCE_DIR ?? path.resolve('artifacts/evidence/a0')
await mkdir(outDir, { recursive: true })
const browser = await chromium.launch({
  executablePath: process.env.ZODO_CHROME ?? 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
  headless: true,
})
const context = await browser.newContext({ viewport: { width: 1440, height: 1100 } })
const page = await context.newPage()
const consoleMessages = []
const pageErrors = []
const responses = []
page.on('console', m => consoleMessages.push({ type: m.type(), text: m.text() }))
page.on('pageerror', e => pageErrors.push(String(e)))
page.on('response', response => {
  if (response.url().includes('/api/')) {
    responses.push({ status: response.status(), url: response.url() })
  }
})

await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 60_000 })
await page.waitForTimeout(2200)
for (const label of ['继续', '开始使用', '稍后', '关闭']) {
  const control = page.getByRole('button', { name: label, exact: true }).first()
  if (await control.count()) {
    await control.click({ timeout: 2000 }).catch(() => {})
    await page.waitForTimeout(400)
  }
}
const input = page.locator('textarea, [contenteditable="true"]').first()
if (!(await input.count())) throw new Error('composer input not found')
await input.click()
await page.keyboard.type('请调用 zodo_probe 工具，method 参数为 ping。调用完成后只回复 done。', { delay: 8 })
await page.waitForTimeout(300)
await page.screenshot({ path: path.join(outDir, 'native-before-send.png'), fullPage: true })
await page.keyboard.press('Enter')

let approvalClicked = false
let doneSeen = false
let toolCardText = ''
const started = Date.now()
while (Date.now() - started < 300_000) {
  await page.waitForTimeout(1200)
  if (!approvalClicked) {
    for (const label of ['允许一次', '允许', '批准', 'Approve once', 'Approve']) {
      const button = page.getByRole('button', { name: label, exact: true }).first()
      if (await button.count()) {
        await page.screenshot({ path: path.join(outDir, 'native-approval-before.png'), fullPage: true })
        await button.click().catch(() => {})
        approvalClicked = true
        await page.waitForTimeout(800)
        break
      }
    }
  }
  toolCardText = await page.locator('body').innerText().catch(() => '')
  if (/done/i.test(toolCardText) && /zodo_probe|pong|zodo-a0-bridge/i.test(toolCardText)) {
    doneSeen = true
    break
  }
  // A denied/closed approval can let the model answer without a tool; stop waiting once the input
  // is idle again and the body shows a final assistant response containing done.
}
await page.waitForTimeout(500)
await page.screenshot({ path: path.join(outDir, 'native-after.png'), fullPage: true })
const bodyText = await page.locator('body').innerText().catch(() => '')
const buttons = await page.locator('button').evaluateAll(nodes => nodes.map(node => ({
  text: node.innerText,
  aria: node.getAttribute('aria-label'),
  testid: node.getAttribute('data-testid'),
})))
await context.storageState({ path: path.join(outDir, 'browser-storage.json') })
await writeFile(path.join(outDir, 'native-tool.json'), JSON.stringify({
  approvalClicked,
  doneSeen,
  bodyText: bodyText.slice(0, 20_000),
  buttons,
  consoleMessages,
  pageErrors,
  responses,
}, null, 2))

// Collect newest session JSONL for raw durable evidence. Never copy attachment bodies.
const sessionsRoot = process.env.ZODO_DSH_HOME
  ? path.join(process.env.ZODO_DSH_HOME, 'sessions')
  : undefined
let sessionFiles = []
let toolLogs = []
if (sessionsRoot) {
  async function walk(dir) {
    for (const entry of await readdir(dir, { withFileTypes: true }).catch(() => [])) {
      const child = path.join(dir, entry.name)
      if (entry.isDirectory()) await walk(child)
      else if (entry.name.endsWith('.jsonl')) sessionFiles.push(child)
    }
  }
  await walk(sessionsRoot)
  sessionFiles.sort()
  for (const file of sessionFiles.slice(-5)) {
    const lines = (await readFile(file, 'utf8')).split('\n').filter(Boolean)
    for (const line of lines) {
      try {
        const record = JSON.parse(line)
        if (JSON.stringify(record).includes('zodo_probe') || JSON.stringify(record).includes('tool/')) {
          toolLogs.push({ file: path.basename(file), record })
        }
      } catch {
        // Ignore a partial tail line.
      }
    }
  }
}
await writeFile(path.join(outDir, 'native-session-tools.json'), JSON.stringify(toolLogs, null, 2))
console.log(JSON.stringify({
  approvalClicked,
  doneSeen,
  bodyHasZodo: /zodo_probe|zodo-a0-bridge/.test(bodyText),
  bodyTail: bodyText.slice(-2500),
  responses: responses.slice(-10),
  pageErrors,
  consoleErrors: consoleMessages.filter(x => x.type === 'error'),
  toolLogCount: toolLogs.length,
}, null, 2))
await browser.close()
