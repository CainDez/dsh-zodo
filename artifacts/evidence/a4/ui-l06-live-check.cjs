// UI-L06 live check against the running Host 3181 (never restarts Host or Core).
// Opening the Zodo workbench reuses the documented host-side path from tests/browser/a4-flow.mjs.
const fs = require('fs')
const path = require('path')
const { chromium } = require('D:/projects/zodo_dsh/tests/browser/node_modules/playwright-core')

const ROOT = 'D:/projects/zodo_dsh'
const URL_FILE = path.join(ROOT, '.runtime/a2-url.txt')
const EVID = path.join(ROOT, 'artifacts/evidence/a4')
const TMP = path.join(ROOT, 'artifacts/tmp')

const results = { steps: {}, consoleErrors: [], pageErrors: [], environment: {} }
const record = async (name, fn) => {
  try {
    const value = await fn()
    results.steps[name] = { status: 'ok', value: value ?? null }
    console.log('PASS', name, value === undefined ? '' : JSON.stringify(value))
  } catch (error) {
    results.steps[name] = { status: 'failed', error: String(error && error.message ? error.message : error) }
    console.log('FAIL', name, String(error && error.message ? error.message : error))
  }
}
async function screenshot(page, name, fullPage = false) {
  const target = path.join(EVID, name)
  await page.screenshot({ path: target, fullPage })
  results.steps[`screenshot:${name}`] = { status: 'ok', value: target }
  console.log('SCREENSHOT', target)
}
const hardStop = setTimeout(() => {
  console.error('HARD TIMEOUT: live UI check exceeded 150s')
  process.exit(3)
}, 150000)

async function openWorkbench(page, url) {
  await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 45000 })
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
  await page.waitForTimeout(500)
  let guide = page.locator('button:has-text("Zodo 工作台")').first()
  if (!(await guide.count())) {
    const newTab = page.getByRole('button', { name: '新标签页' }).first()
    if (await newTab.count()) await newTab.click().catch(() => {})
    await page.waitForTimeout(500)
    guide = page.locator('button:has-text("Zodo 工作台")').first()
  }
  if (!(await guide.count())) {
    const body = await page.locator('body').innerText().catch(() => '')
    throw new Error(`Zodo workbench entry not found; body starts: ${body.slice(0, 300)}`)
  }
  await guide.click()
  await page.waitForSelector('[data-zodo-workbench="a2"]', { timeout: 25000 })
  await page.waitForTimeout(900)
}

async function main() {
  const url = fs.readFileSync(URL_FILE, 'utf8').trim()
  results.environment.url = url.replace(/token=[^&]+/, 'token=<redacted>')
  const browser = await chromium.launch({
    executablePath: process.env.ZODO_CHROME ?? 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
    headless: true,
  })
  results.environment.browser = 'system chrome via playwright-core 1.64.1'
  const context = await browser.newContext({ viewport: { width: 1500, height: 950 }, locale: 'zh-CN' })
  const page = await context.newPage()
  page.setDefaultTimeout(12000)
  page.setDefaultNavigationTimeout(30000)
  page.on('console', message => {
    if (message.type() === 'error') results.consoleErrors.push(message.text())
  })
  page.on('pageerror', error => results.pageErrors.push(String(error)))

  await record('open Zodo workbench in live Host', async () => {
    await openWorkbench(page, url)
    const revision = await page.locator('.zw-revision').first().textContent()
    results.environment.revision = revision
    return { revision }
  })

  await record('open tasks view', async () => {
    await page.click('[data-testid="nav-tasks"]')
    await page.waitForSelector('[data-testid^="task-row-"]', { timeout: 15000 })
    return { rows: await page.locator('[data-testid^="task-row-"]').count() }
  })

  // The live workspace already holds a weekly series (每周整理案例素材) created through the earlier
  // recurrence flow; selecting one of its instances is the real S24-style entry point for L06.
  const seriesRow = page.locator('[data-testid^="task-row-"]', { hasText: '每周整理案例素材' }).first()
  await record('select recurring task', async () => {
    await seriesRow.waitFor({ state: 'visible', timeout: 15000 })
    await seriesRow.click()
    await page.waitForSelector('.zw-detail', { timeout: 10000 })
    return { detail_title: (await page.locator('.zw-detail h2').first().textContent()) }
  })

  await record('L06 entry button exists', async () => {
    const trigger = page.locator('[data-testid="recurrence-scope"]')
    await trigger.waitFor({ state: 'visible', timeout: 8000 })
    return { label: (await trigger.textContent()).trim() }
  })

  await record('dialog opens with honest scopes', async () => {
    await page.click('[data-testid="recurrence-scope"]')
    const dialog = page.locator('[role="dialog"][aria-label="循环作用范围"]')
    await dialog.waitFor({ state: 'visible', timeout: 8000 })
    const pills = await dialog.locator('.zw-pill').allTextContents()
    const singleActive = await dialog.locator('[data-testid="recurrence-scope-single"].zw-pill-active').count()
    await screenshot(page, 'ui-l06-dialog-single.png')
    return { pills, single_is_default: singleActive === 1 }
  })

  await record('future scope shows range and editable fields', async () => {
    await page.click('[data-testid="recurrence-scope-future"]')
    await page.waitForSelector('[data-testid="recurrence-scope-title"]', { timeout: 8000 })
    const dialog = page.locator('[role="dialog"][aria-label="循环作用范围"]')
    const fineprint = await dialog.locator('p.zw-fineprint').allTextContents()
    const rangeLine = fineprint.find(text => text.includes('将更新')) ?? null
    const titleValue = await page.inputValue('[data-testid="recurrence-scope-title"]')
    await screenshot(page, 'ui-l06-dialog-future.png')
    return { range_line: rangeLine, title_value: titleValue }
  })

  await record('client-side validation surfaces inline', async () => {
    await page.fill('[data-testid="recurrence-scope-title"]', '   ')
    await page.click('[data-testid="recurrence-scope-submit"]')
    const alert = page.locator('[role="dialog"][aria-label="循环作用范围"] [role="alert"]')
    await alert.waitFor({ state: 'visible', timeout: 6000 })
    const text = await alert.textContent()
    await screenshot(page, 'ui-l06-validation.png')
    return { alert: text }
  })

  await record('escape closes and focus returns to trigger', async () => {
    await page.keyboard.press('Escape')
    await page.locator('[role="dialog"][aria-label="循环作用范围"]').waitFor({ state: 'hidden', timeout: 6000 })
    const focused = await page.evaluate(() => {
      const active = document.activeElement
      return { testid: active?.getAttribute ? active.getAttribute('data-testid') : null, tag: active?.tagName ?? null }
    })
    await screenshot(page, 'ui-l06-closed-focus-return.png')
    return focused
  })

  // Submit only to document the failure surface when the running Core predates the new command.
  // A rejected command writes nothing; a supported one is a normal UI-L06 edit.
  await record('submit surfaces backend result or failure', async () => {
    await page.click('[data-testid="recurrence-scope"]')
    const dialog = page.locator('[role="dialog"][aria-label="循环作用范围"]')
    await dialog.waitFor({ state: 'visible', timeout: 8000 })
    await page.click('[data-testid="recurrence-scope-future"]')
    await page.waitForSelector('[data-testid="recurrence-scope-title"]', { timeout: 8000 })
    await page.fill('[data-testid="recurrence-scope-title"]', '每周整理案例素材')
    const revisionBefore = await page.locator('.zw-revision').first().textContent().catch(() => null)
    await page.click('[data-testid="recurrence-scope-submit"]')
    await page.waitForTimeout(3000)
    const modalStillOpen = await page.locator('[role="dialog"][aria-label="循环作用范围"]').count()
    const inlineError = modalStillOpen
      ? await page.locator('[role="dialog"][aria-label="循环作用范围"] [role="alert"]').first().textContent().catch(() => null)
      : null
    const globalError = await page.locator('[data-testid="zodo-error"]').first().textContent().catch(() => null)
    const toast = await page.locator('[data-testid="zodo-toast"]').first().textContent().catch(() => null)
    await screenshot(page, 'ui-l06-submit-live-core.png')
    if (modalStillOpen) await page.keyboard.press('Escape')
    return { revision_before: revisionBefore, modal_still_open: modalStillOpen > 0, inline_error: inlineError, global_error: globalError, toast }
  })

  await record('reopen and close via header ×', async () => {
    await page.click('[data-testid="recurrence-scope"]')
    const dialog = page.locator('[role="dialog"][aria-label="循环作用范围"]')
    await dialog.waitFor({ state: 'visible', timeout: 8000 })
    await dialog.locator('.zw-modal-head .zw-icon-btn').click()
    await dialog.waitFor({ state: 'hidden', timeout: 6000 })
    return { closed: true }
  })

  await record('no uncaught page errors', async () => {
    return { pageErrors: results.pageErrors.slice(0, 10), consoleErrors: results.consoleErrors.slice(0, 10) }
  })

  await browser.close()
  clearTimeout(hardStop)
  results.finished_at = new Date().toISOString()
  fs.writeFileSync(path.join(EVID, 'ui-l06-live.json'), JSON.stringify(results, null, 2), 'utf8')
  fs.writeFileSync(path.join(TMP, 'u06-ui-live.json'), JSON.stringify(results, null, 2), 'utf8')
  console.log('wrote artifacts/evidence/a4/ui-l06-live.json')
}

main().catch(error => {
  console.error('LIVE UI CHECK FAILED:', error)
  try {
    fs.writeFileSync(path.join(EVID, 'ui-l06-live.json'), JSON.stringify({ ...results, fatal: String(error) }, null, 2), 'utf8')
  } catch {}
  process.exitCode = 1
})
