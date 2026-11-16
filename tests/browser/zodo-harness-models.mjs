/**
 * Zodo Models page: read-only Harness model catalog + fill Core model draft.
 *
 * Verifies the isolated Host (127.0.0.1:3181) exposes the DSH catalog configured in the
 * zodo-a0 profile (pi-ai provider `ollama`: displayName "Ollama local", baseURL
 * http://127.0.0.1:11434/v1, model aravhawk/qwen3.5-opus-4.6:latest) through the Zodo
 * workbench Models page, and that "填入 Core 模型" fills the Core model-connection draft
 * (local provider because the endpoint host is 127.0.0.1) without copying any secret.
 *
 * Run from the repo root so the screenshot lands in .runtime/:
 *   $env:ZODO_URL = Get-Content .runtime\a2-url.txt -Raw
 *   node tests/browser/zodo-harness-models.mjs
 */
import assert from 'node:assert/strict'
import { chromium } from 'playwright-core'

const url = process.env.ZODO_URL
if (!url) throw new Error('ZODO_URL is required')
const OLLAMA_BASE_URL = 'http://127.0.0.1:11434/v1'
const OLLAMA_MODEL = 'aravhawk/qwen3.5-opus-4.6:latest'
const OLLAMA_GROUP = 'Ollama local'
const SCREENSHOT = '.runtime/zodo-harness-models.png'

const browser = await chromium.launch({
  executablePath: process.env.ZODO_CHROME ?? 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
  headless: true,
})
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } })
  const errors = []
  const consoleErrors = []
  const catalogResponses = []
  page.on('pageerror', error => errors.push(String(error)))
  page.on('console', message => { if (message.type() === 'error') consoleErrors.push(message.text()) })
  page.on('response', async response => {
    if (!response.url().includes('/zodo-bridge/v1/harness/models')) return
    let body
    try { body = await response.json() } catch { body = undefined }
    catalogResponses.push({ status: response.status(), body })
  })

  await page.goto(url, { waitUntil: 'domcontentloaded' })
  const entry = page.getByRole('button', { name: 'Zodo', exact: true })
  await entry.waitFor()
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

  // Models page: secondary nav entry in the workbench.
  await page.getByTestId('nav-models').click({ timeout: 5000 })
  const section = page.getByTestId('harness-models')
  await section.waitFor({ timeout: 20_000 })
  assert.equal(await section.isVisible(), true, 'harness-models section is visible')
  const sectionText = await section.innerText()
  assert.match(sectionText, /Harness 模型/, 'section title names the Harness model catalog')
  assert.match(sectionText, /Harness 当前模型：ollama \/ aravhawk\/qwen3\.5-opus-4\.6:latest/,
    'section shows the Host default model route')

  // Read-only catalog: group + endpoint rendered exactly as configured in the Host profile.
  await section.getByText(OLLAMA_GROUP, { exact: false }).first().waitFor({ timeout: 10_000 })
  assert.ok(await section.getByText(OLLAMA_BASE_URL, { exact: false }).count(), 'catalog shows the Ollama endpoint')
  assert.ok(await section.getByText(OLLAMA_MODEL, { exact: false }).count(), 'catalog shows the Ollama model id')

  // Refresh control belongs to the section and stays usable.
  const refresh = page.getByTestId('harness-models-refresh')
  assert.equal(await refresh.count(), 1, 'harness-models-refresh exists')
  assert.equal(await refresh.isEnabled(), true, 'harness-models-refresh is enabled')
  assert.equal(await page.getByTestId('harness-models-retry').count(), 0, 'catalog loaded without a read error')
  assert.ok(await page.getByTestId('harness-group-ollama').count(), 'catalog renders the ollama provider group')

  // Pick the "填入 Core 模型" row inside the Ollama group so a multi-provider catalog still
  // fills the expected row (the profile currently has exactly one model).
  const useButtons = section.locator('[data-testid^="harness-model-use-"]')
  const useCount = await useButtons.count()
  assert.ok(useCount > 0, 'at least one harness-model-use-* button exists')
  // This host has exactly one compatible route, so a disabled fill button cannot be exercised here;
  // assert none of the advertised rows is disabled instead.
  assert.equal(await useButtons.evaluateAll(buttons => buttons.filter(button => button.disabled).length), 0,
    'every advertised fill button is enabled (no disabled row in this single-route host)')
  const targetIndex = await useButtons.evaluateAll(buttons => {
    for (let index = 0; index < buttons.length; index += 1) {
      let node = buttons[index]
      while (node && node.getAttribute('data-testid') !== 'harness-models') {
        if ((node.textContent ?? '').includes('Ollama local')) return index
        node = node.parentElement
      }
    }
    return 0
  })
  const target = useButtons.nth(targetIndex)
  assert.equal(await target.isEnabled(), true, 'model fill button is enabled')
  await target.click()

  // Fill action writes the Core model-connection draft only; no credential travels to the browser.
  const baseUrlInput = page.getByLabel('本地 OpenAI 兼容端点', { exact: true })
  const modelInput = page.getByLabel('本地模型 ID', { exact: true })
  await baseUrlInput.waitFor({ timeout: 10_000 })
  const readDraft = async () => ({ baseUrl: await baseUrlInput.inputValue(), model: await modelInput.inputValue() })
  const deadline = Date.now() + 10_000
  let draft = await readDraft()
  while (Date.now() < deadline && !(draft.baseUrl === OLLAMA_BASE_URL && draft.model.includes(OLLAMA_MODEL))) {
    await page.waitForTimeout(150)
    draft = await readDraft()
  }
  assert.equal(draft.baseUrl, OLLAMA_BASE_URL, 'local endpoint filled from the Harness catalog')
  assert.ok(draft.model.includes(OLLAMA_MODEL), 'local model id filled from the Harness catalog')
  assert.equal(await page.getByTestId('model-save').isEnabled(), true, 'existing save button remains usable')

  // Scroll the Core draft into view so the screenshot proves the filled fields, not just the source.
  await baseUrlInput.scrollIntoViewIfNeeded()
  await page.waitForTimeout(150)
  await page.screenshot({ path: SCREENSHOT })
  assert.deepEqual(errors, [], 'no page errors')

  // The route returns the catalog directly ({ ok, schema, groups, failures, ... }); tolerate the
  // older { result } envelope so the summary keeps working if the bridge shape is tightened.
  const catalogBody = body => (body && typeof body === 'object' && body.result && typeof body.result === 'object'
    ? body.result
    : body)
  const catalog = catalogResponses.map(item => {
    const body = catalogBody(item.body)
    return {
      status: item.status,
      ok: Boolean(body && body.ok),
      schema: body?.schema ?? null,
      groups: Array.isArray(body?.groups) ? body.groups.length : null,
      models: Array.isArray(body?.groups)
        ? body.groups.reduce((total, group) => total + (Array.isArray(group.models) ? group.models.length : 0), 0)
        : null,
      failures: Array.isArray(body?.failures) ? body.failures.length : null,
    }
  })
  console.log(JSON.stringify({
    harnessModelsSection: true,
    catalogEndpoint: OLLAMA_BASE_URL,
    group: OLLAMA_GROUP,
    useButtons: useCount,
    filledDraft: draft,
    saveEnabled: true,
    catalogResponses: catalog,
    consoleErrors,
    screenshot: SCREENSHOT,
    pageErrors: errors,
  }))
} finally {
  await browser.close()
}
