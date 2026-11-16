import assert from 'node:assert/strict'
import { chromium } from 'playwright-core'

const url = process.env.ZODO_URL
if (!url) throw new Error('ZODO_URL is required')
const browser = await chromium.launch({
  executablePath: process.env.ZODO_CHROME ?? 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
  headless: true,
})
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } })
  const errors = []
  page.on('pageerror', error => errors.push(String(error)))
  await page.goto(url, { waitUntil: 'domcontentloaded' })
  const entry = page.getByRole('button', { name: 'Zodo', exact: true })
  await entry.waitFor()
  assert.equal(await entry.count(), 1)
  assert.equal(await page.getByTestId('zodo-open').count(), 0)
  await entry.click()
  await page.locator('[data-zodo-workbench="a2"]').waitFor()
  await page.getByText('Core 已连接', { exact: true }).waitFor()
  assert.equal(await entry.getAttribute('aria-current'), 'page')
  // A fresh browser can show the Host's first-run overlay after the workbench connects.
  await page.keyboard.press('Escape')
  for (const name of ['继续', '开始使用', '稍后', '关闭', '跳过']) {
    const dismiss = page.getByRole('button', { name, exact: true }).filter({ visible: true }).first()
    if (await dismiss.count()) await dismiss.click({ timeout: 2000 }).catch(() => {})
  }
  await page.getByTestId('nav-inbox').click({ timeout: 3000 })
  await page.getByTestId('nav-today').click()
  const box = await page.locator('[data-zodo-workbench="a2"]').boundingBox()
  assert.ok(box && box.width > 700 && box.height > 500, 'workbench fills the main area')
  const toggle = page.locator('[data-sidebar-toggle]').filter({ visible: true }).first()
  await toggle.click()
  await entry.click()
  assert.equal(await entry.getAttribute('aria-current'), 'page')
  await toggle.click()
  await page.screenshot({ path: '.runtime/zodo-sidebar-updated.png' })
  assert.deepEqual(errors, [])
  console.log(JSON.stringify({ sidebarEntry: true, composerEntryRemoved: true, coreConnected: true, collapsedEntry: true, mainArea: box, errors }))
} finally {
  await browser.close()
}
