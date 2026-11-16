/** Run from any cwd: node tests/browser/harness-isolated-regression.mjs.
 * Always creates a fresh owned Core; never accepts a live URL or Host profile.
 */
import assert from 'node:assert/strict'
import path from 'node:path'
import { mkdir, writeFile, stat, readFile } from 'node:fs/promises'
import { createHash } from 'node:crypto'
import { fileURLToPath } from 'node:url'
import { chromium } from 'playwright-core'
import { startFixture } from './harness-isolated-fixture.mjs'

const here = path.dirname(fileURLToPath(import.meta.url))
const output = path.join(here, 'harness-fixture-runtime/results', new Date().toISOString().replace(/[:.]/g, '-'))
await mkdir(output, { recursive: true })
const report = { fixture: 'real Core + real index.js bridge + real client.js + React 18', fullHost: false,
  exclusions: ['DSH authentication/login', 'DSH router and sidebar composition', 'live model/cloud dispatch', 'native desktop shell'],
  cases: [], navigation: [], pageErrors: [], unexpectedNetwork: [] }
let browser, fixture, page
const check = async (name, run) => {
  const start = Date.now()
  try { await run(); report.cases.push({ name, passed: true, ms: Date.now() - start }) }
  catch (error) {
    report.cases.push({ name, passed: false, error: String(error), ms: Date.now() - start })
    if (page) {
      await page.screenshot({ path: path.join(output, 'failure-' + report.cases.length + '.png'), fullPage: true }).catch(() => {})
      await page.unrouteAll({ behavior: 'wait' }).catch(() => {})
      await page.reload({ waitUntil: 'domcontentloaded' }).catch(() => {})
    }
  }
}
const connected = () => page.getByText('Core 已连接', { exact: true }).waitFor()
const query = async (name, payload = {}) => page.evaluate(async ({ name, payload }) => {
  const response = await fetch('/zodo-bridge/v1/rpc', { method: 'POST', credentials: 'same-origin',
    headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ kind: 'query', envelope: {
      schema: 'zodo.query/1', request_id: crypto.randomUUID(), workspace_id: 'ws_local', query: name, payload,
    } }) })
  const body = await response.json()
  if (!response.ok || !body.ok) throw new Error(body.error?.message ?? 'query failed')
  return body.result.data
}, { name, payload })
const snapshot = () => query('workspace.snapshot')
const nav = async id => { await page.getByTestId('nav-' + id).click(); await page.locator('main h1').first().waitFor() }
const screenshot = name => page.screenshot({ path: path.join(output, name + '.png'), fullPage: true })

try {
  const root = path.resolve(here, '../..')
  const hash = async file => createHash('sha256').update(await readFile(file)).digest('hex')
  report.artifacts = {
    coreSha256: await hash(process.env.ZODO_FIXTURE_CORE_BIN ?? path.join(root, 'target/release/zodo.exe')),
    bridgeSha256: await hash(path.join(root, 'adapters/deepseek-harness/plugin/index.js')),
    clientSha256: await hash(path.join(root, 'adapters/deepseek-harness/plugin/client.js')),
  }
  fixture = await startFixture({ binary: process.env.ZODO_FIXTURE_CORE_BIN })
  browser = await chromium.launch({ executablePath: process.env.ZODO_CHROME ?? 'C:/Program Files/Google/Chrome/Application/chrome.exe', headless: true })
  const context = await browser.newContext({ viewport: { width: 1440, height: 1000 }, timezoneId: 'Asia/Shanghai' })
  // A new browser context contains no real Host cookies/sign-ins or persistent localStorage.
  await context.route('**/*', route => {
    if (new URL(route.request().url()).origin !== fixture.url) {
      report.unexpectedNetwork.push(new URL(route.request().url()).origin)
      return route.abort()
    }
    return route.continue()
  })
  page = await context.newPage()
  page.setDefaultTimeout(8000)
  page.on('pageerror', error => report.pageErrors.push(String(error)))

  await check('BFF unauthenticated and cross-origin gates', async () => {
    let response = await fetch(fixture.url + '/zodo-bridge/v1/rpc', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{}' })
    assert.equal(response.status, 401)
    response = await fetch(fixture.url + '/zodo-bridge/v1/session', { method: 'POST', headers: { Origin: 'https://untrusted.invalid' }, body: '{}' })
    assert.equal(response.status, 403)
    response = await fetch(fixture.url + '/zodo-bridge/v1/session', { method: 'POST', headers: { Origin: fixture.url }, body: '{}' })
    const body = await response.json()
    assert.equal(response.status, 200); assert.equal(body.ok, true)
    assert.equal(body.session_token, undefined, 'browser-origin response never exposes bearer token')
    assert.match(response.headers.get('set-cookie'), /HttpOnly; SameSite=Strict/)
    assert.equal(body.core.tokens, undefined)
  })

  await check('Real client boot and empty Core', async () => {
    await page.goto(fixture.url, { waitUntil: 'domcontentloaded' }); await connected()
    assert.equal((await snapshot()).tasks.length, 0)
    await screenshot('desktop-today-empty')
  })

  await check('21 navigation surfaces mount without JS errors', async () => {
    const ids = ['today','inbox','goals','schedule','review','tasks','projects','habits','views','automation','focus','proposals','runs','search','models','data','sync','settings','privacy','agents','audit']
    assert.equal(await page.locator('[data-testid^="nav-"]').count(), ids.length)
    for (const id of ids) {
      await nav(id)
      assert.match(await page.getByTestId('nav-' + id).getAttribute('class'), /zw-nav-active/)
      const heading = await page.locator('main h1').first().innerText()
      assert.ok(heading.trim(), id + ' heading')
      report.navigation.push({ id, heading })
    }
    assert.deepEqual(report.pageErrors, [])
  })

  await check('Capture persists raw text through real bridge', async () => {
    await page.getByTestId('top-capture').click()
    await page.getByTestId('capture-input').fill('Fixture isolated capture: contact printing shop')
    await page.getByTestId('capture-save').click()
    await page.getByRole('dialog').waitFor({ state: 'hidden' })
    await nav('inbox')
    await page.getByText('Fixture isolated capture: contact printing shop', { exact: true }).first().waitFor()
    assert.equal((await snapshot()).inbox.length, 1)
  })

  await check('Inbox manual clarification and task/goal/project conversion', async () => {
    await nav('inbox')
    const state = await snapshot()
    const entry = state.inbox.find(row => row.text === 'Fixture isolated capture: contact printing shop')
    assert.ok(entry)
    await page.getByTestId('inbox-row-' + entry.id).click()
    const detail = page.getByRole('dialog', { name: '收集详情' })
    await detail.getByLabel('问题', { exact: true }).fill('Fixture clarification')
    await detail.getByLabel('回答（可稍后）').fill('Keep source text')
    const saved = page.waitForResponse(response => response.url().endsWith('/rpc') && response.request().postData()?.includes('capture.clarify'))
    await detail.getByRole('button', { name: '保存澄清', exact: true }).click()
    assert.equal((await saved).status(), 200)
    assert.ok((await snapshot()).inbox.find(row => row.id === entry.id).clarifications.some(row => row.question === 'Fixture clarification'))
    await detail.getByRole('button', { name: '关闭', exact: true }).last().click()
    await detail.waitFor({ state: 'hidden' })
    await page.getByTestId('inbox-row-' + entry.id).click()
    await detail.getByRole('button', { name: '转换…', exact: true }).click()
    const convert = page.getByRole('dialog', { name: '处理收集内容' })
    await convert.getByLabel('任务标题').fill('Fixture converted task')
    await convert.getByLabel('同时创建目标').check()
    await convert.getByLabel('目标标题').fill('Fixture converted goal')
    await convert.getByLabel('同时创建项目').check()
    await convert.getByLabel('项目标题').fill('Fixture converted project')
    await convert.getByRole('button', { name: '提交转换', exact: true }).click()
    await convert.waitFor({ state: 'hidden' })
    const after = await snapshot()
    assert.equal(after.inbox.find(row => row.id === entry.id).text, entry.text)
    assert.ok(after.goals.some(row => row.title === 'Fixture converted goal'))
    assert.ok(after.projects.some(row => row.title === 'Fixture converted project'))
    assert.ok(after.tasks.some(row => row.title === 'Fixture converted task'))
    await nav('goals'); await page.getByText('Fixture converted goal', { exact: true }).waitFor()
    await screenshot('converted-goal-project')
  })

  let taskId
  await check('Task create validation, keyboard Escape and focus restore', async () => {
    await nav('tasks')
    const open = page.getByRole('button', { name: '新建任务', exact: true })
    await open.click()
    const dialog = page.getByRole('dialog', { name: '新建任务' })
    await dialog.getByTestId('task-save').click()
    await dialog.getByText('请填写标题', { exact: true }).waitFor()
    await dialog.getByLabel('标题', { exact: true }).fill('Fixture task initial')
    await dialog.getByLabel('预计分钟').fill('30')
    await dialog.getByTestId('task-save').click()
    await dialog.waitFor({ state: 'hidden' })
    const task = (await snapshot()).tasks.find(row => row.title === 'Fixture task initial')
    assert.ok(task); taskId = task.id
    await open.click(); await page.keyboard.press('Escape')
    await dialog.waitFor({ state: 'hidden' })
    assert.equal(await open.evaluate(element => element === document.activeElement), true)
  })

  await check('Task edit, completion/reopen and archive/restore', async () => {
    assert.ok(taskId, 'create task prerequisite')
    await nav('tasks'); await page.getByTestId('task-row-' + taskId).click()
    const detail = page.getByRole('complementary', { name: '任务详情' })
    await detail.getByRole('button', { name: '编辑', exact: true }).click()
    const dialog = page.getByRole('dialog', { name: '编辑任务' })
    await dialog.getByLabel('标题', { exact: true }).fill('Fixture task edited')
    await dialog.getByTestId('task-save').click(); await dialog.waitFor({ state: 'hidden' })
    assert.equal((await snapshot()).tasks.find(row => row.id === taskId).title, 'Fixture task edited')
    await detail.getByRole('button', { name: '标记完成', exact: true }).click()
    await detail.getByRole('button', { name: '重新打开', exact: true }).waitFor()
    assert.equal((await snapshot()).tasks.find(row => row.id === taskId).status, 'done')
    await detail.getByRole('button', { name: '重新打开', exact: true }).click()
    await detail.getByRole('button', { name: '标记完成', exact: true }).waitFor()
    await detail.getByRole('button', { name: '归档', exact: true }).click()
    await detail.getByRole('button', { name: '恢复', exact: true }).waitFor()
    assert.equal((await snapshot()).tasks.find(row => row.id === taskId).archived, true)
    await detail.getByRole('button', { name: '恢复', exact: true }).click()
    await detail.getByRole('button', { name: '归档', exact: true }).waitFor()
  })

  await check('Capacity + plan preview + explicit approval/commit', async () => {
    assert.ok(taskId, 'create task prerequisite')
    await nav('schedule')
    await page.getByRole('button', { name: '增加容量', exact: true }).click()
    const dialog = page.getByRole('dialog', { name: '增加可用容量' })
    const future = new Date(Date.now() + 86400000)
    while (['Sat', 'Sun'].includes(future.toLocaleDateString('en-US', { weekday: 'short', timeZone: 'Asia/Shanghai' }))) future.setDate(future.getDate() + 1)
    const tomorrow = future.toLocaleDateString('en-CA', { timeZone: 'Asia/Shanghai' })
    await dialog.getByLabel('名称').fill('Fixture future capacity')
    await dialog.getByLabel('开始', { exact: true }).fill(tomorrow + 'T14:00')
    await dialog.getByLabel('结束', { exact: true }).fill(tomorrow + 'T16:00')
    await dialog.getByLabel('可用分钟').fill('120')
    await dialog.getByRole('button', { name: '保存', exact: true }).click()
    await dialog.waitFor({ state: 'hidden' })
    const before = await snapshot()
    await page.getByRole('button', { name: '生成计划', exact: true }).first().click()
    const plan = page.getByRole('dialog', { name: '检查计划建议' })
    await plan.waitFor()
    assert.equal((await snapshot()).allocations.length, before.allocations.length, 'preview cannot mutate allocations')
    assert.equal(await plan.getByTestId('plan-apply').isEnabled(), true)
    await screenshot('plan-preview')
    await plan.getByTestId('plan-apply').click(); await plan.waitFor({ state: 'hidden' })
    const after = await snapshot()
    assert.ok(after.proposals.some(row => row.status === 'applied'))
    assert.ok(after.allocations.length > before.allocations.length, 'committed plan allocates task')
    assert.ok(after.workspace.revision > before.workspace.revision, 'commit advances authoritative revision')
  })

  await check('Search resolves committed task', async () => {
    await nav('search')
    await page.getByPlaceholder('搜索任务、目标、项目…').fill('Fixture task edited')
    await page.getByRole('button', { name: '搜索', exact: true }).click()
    await page.locator('.zw-search-result').getByText('Fixture task edited', { exact: true }).waitFor()
  })

  await check('Goal and linked project create through UI editors', async () => {
    await nav('goals')
    await page.getByRole('button', { name: '新建目标', exact: true }).click()
    const goalDialog = page.getByRole('dialog', { name: '新建目标' })
    await goalDialog.getByLabel('目标', { exact: true }).fill('Fixture editor goal')
    await goalDialog.getByLabel('成功标准').fill('All fixture checks pass')
    await goalDialog.getByRole('button', { name: '保存', exact: true }).click()
    await goalDialog.waitFor({ state: 'hidden' })
    const goal = (await snapshot()).goals.find(row => row.title === 'Fixture editor goal')
    assert.ok(goal)
    const card = page.locator('article').filter({ has: page.getByRole('heading', { name: 'Fixture editor goal', exact: true }) })
    await card.getByRole('button', { name: '添加项目', exact: true }).click()
    const projectDialog = page.getByRole('dialog', { name: '新建项目' })
    await projectDialog.getByLabel('项目标题').fill('Fixture editor project')
    await projectDialog.getByLabel('范围', { exact: true }).fill('Isolated UI verification')
    assert.equal(await projectDialog.getByLabel('所属目标').inputValue(), goal.id)
    await projectDialog.getByRole('button', { name: '保存', exact: true }).click()
    await projectDialog.waitFor({ state: 'hidden' })
    const project = (await snapshot()).projects.find(row => row.title === 'Fixture editor project')
    assert.equal(project.goal_id, goal.id)
    await screenshot('goal-project-editors')
  })

  await check('Habit create and log preserve activity facts', async () => {
    await nav('habits')
    await page.getByRole('button', { name: '新建习惯', exact: true }).click()
    const dialog = page.getByRole('dialog', { name: '新建习惯' })
    await dialog.getByLabel('习惯', { exact: true }).fill('Fixture daily review')
    await dialog.getByLabel('每周目标次数').fill('3')
    await dialog.getByRole('button', { name: '保存', exact: true }).click()
    await dialog.waitFor({ state: 'hidden' })
    const habit = (await snapshot()).habits.find(row => row.title === 'Fixture daily review')
    assert.ok(habit)
    const card = page.locator('article').filter({ has: page.getByRole('heading', { name: habit.title, exact: true }) })
    await card.getByRole('button', { name: '记录一次', exact: true }).click()
    const log = page.getByRole('dialog', { name: '记录习惯' })
    await log.getByLabel('本次分钟').fill('15')
    await log.getByLabel('备注（可选）').fill('Fixture habit log')
    await log.getByRole('button', { name: '记录', exact: true }).click()
    await log.waitFor({ state: 'hidden' })
    const activity = (await snapshot()).activities.find(row => row.habit_id === habit.id)
    assert.equal(activity.minutes, 15); assert.equal(activity.kind, 'habit')
    await screenshot('habit-logged')
  })

  await check('Saved view create and apply reevaluates task filter', async () => {
    await nav('views')
    await page.getByRole('button', { name: '新建视图', exact: true }).click()
    const dialog = page.getByRole('dialog', { name: '新建保存视图' })
    await dialog.getByLabel('名称', { exact: true }).fill('Fixture edited tasks view')
    await dialog.getByLabel('文本包含').fill('Fixture task edited')
    await dialog.getByRole('button', { name: '保存', exact: true }).click()
    await dialog.waitFor({ state: 'hidden' })
    assert.ok((await snapshot()).saved_queries.some(row => row.name === 'Fixture edited tasks view'))
    const card = page.locator('article').filter({ has: page.getByRole('heading', { name: 'Fixture edited tasks view', exact: true }) })
    await card.getByRole('button', { name: '应用', exact: true }).click()
    await page.getByTestId('task-row-' + taskId).waitFor()
    assert.equal(await page.locator('[data-testid^="task-row-"]').count(), 1)
    await screenshot('saved-view-applied')
  })

  await check('Automation manual run exhausts one-run budget exactly once', async () => {
    await nav('automation')
    await page.getByRole('button', { name: '新建规则', exact: true }).click()
    const dialog = page.getByRole('dialog', { name: '新建自动化规则' })
    await dialog.getByLabel('规则名称').fill('Fixture single budget')
    await dialog.getByLabel('动作：创建任务标题').fill('Fixture automated task')
    await dialog.getByLabel('最多运行次数').fill('1')
    assert.equal(await dialog.getByLabel('按时间自动触发（否则仅手动运行）').isChecked(), false)
    await dialog.getByRole('button', { name: '保存', exact: true }).click()
    await dialog.waitFor({ state: 'hidden' })
    const card = page.locator('article').filter({ has: page.getByRole('heading', { name: 'Fixture single budget', exact: true }) })
    const response = page.waitForResponse(row => row.url().endsWith('/rpc') && row.request().postData()?.includes('automation.run'))
    await card.getByRole('button', { name: '手动运行', exact: true }).click()
    assert.equal((await response).status(), 200)
    const state = await snapshot()
    const rule = state.automation_rules.find(row => row.name === 'Fixture single budget')
    assert.equal(rule.runs, 1); assert.equal(rule.budget, 1)
    assert.equal(state.tasks.filter(row => row.title === 'Fixture automated task').length, 1)
    await page.waitForFunction(() => [...document.querySelectorAll('button')].some(button => button.textContent === '手动运行' && button.disabled))
    assert.equal(await card.getByRole('button', { name: '手动运行', exact: true }).isDisabled(), true)
    await screenshot('automation-budget-exhausted')
  })

  await check('Focus start pause resume finish does not complete task', async () => {
    await nav('focus')
    await page.getByRole('combobox', { name: /^任务/ }).selectOption(taskId)
    await page.getByTestId('focus-pause').waitFor()
    const running = (await snapshot()).focus_sessions.find(row => row.state === 'running')
    assert.equal(running.task_id, taskId)
    await page.getByTestId('focus-pause').click(); await page.getByTestId('focus-resume').waitFor()
    assert.equal((await snapshot()).focus_sessions.find(row => row.id === running.id).state, 'paused')
    await page.getByTestId('focus-resume').click(); await page.getByTestId('focus-pause').waitFor()
    await page.getByTestId('focus-finish').click(); await page.getByTestId('focus-finish').waitFor({ state: 'hidden' })
    const after = await snapshot()
    assert.equal(after.focus_sessions.find(row => row.id === running.id).state, 'completed')
    assert.notEqual(after.tasks.find(row => row.id === taskId).status, 'done')
    assert.ok(after.activities.some(row => row.task_id === taskId && row.kind === 'focus'))
    await screenshot('focus-completed')
  })

  await check('Data JSON/CSV downloads and verified SQLite backup', async () => {
    await nav('data')
    const downloadText = async id => {
      const pending = page.waitForEvent('download')
      await page.getByTestId(id).click()
      const download = await pending
      const stream = await download.createReadStream()
      let text = ''; for await (const chunk of stream) text += chunk.toString('utf8')
      assert.equal(await download.failure(), null)
      return text
    }
    const json = await downloadText('data-export')
    assert.ok(JSON.parse(json)); assert.ok(json.includes('Fixture task edited'))
    assert.equal(/"tokens?"\s*:/i.test(json), false, 'export excludes token keys')
    const csv = await downloadText('data-export-csv')
    assert.ok(csv.includes('Fixture task edited'))
    const pending = page.waitForResponse(row => row.url().endsWith('/data/backup') && row.request().method() === 'POST')
    await page.getByTestId('data-backup').click()
    const response = await pending
    assert.equal(response.status(), 200)
    const metadata = (await response.json()).result
    assert.ok(metadata.size_bytes > 0)
    assert.match(metadata.sha256, /^[a-f0-9]{64}$/)
    await page.getByText('SQLite 备份已创建。', { exact: true }).waitFor()
    await screenshot('data-backup')
  })

  await check('Harness catalog compatible fill is draft-only', async () => {
    await nav('models')
    const section = page.getByTestId('harness-models')
    await section.getByText('Fixture deterministic model', { exact: false }).first().waitFor()
    const fills = page.locator('[data-testid^="harness-model-use-"]')
    assert.equal(await fills.first().isEnabled(), true)
    const beforeConfig = await page.evaluate(async () => (await (await fetch('/zodo-bridge/v1/model')).json()).result.config)
    assert.equal(beforeConfig.local.model, 'fixture-initial-model')
    await fills.first().click()
    assert.equal(await page.getByLabel('本地 OpenAI 兼容端点', { exact: true }).inputValue(), 'http://127.0.0.1:9/v1')
    assert.equal(await page.getByLabel('本地模型 ID', { exact: true }).inputValue(), 'fixture-model')
    const afterConfig = await page.evaluate(async () => (await (await fetch('/zodo-bridge/v1/model')).json()).result.config)
    assert.deepEqual(afterConfig, beforeConfig, 'filling draft must not save model configuration')
    await screenshot('models')
  })

  await check('Catalog failure has usable retry and recovery', async () => {
    await page.route('**/zodo-bridge/v1/harness/models', route => route.fulfill({ status: 503, contentType: 'application/json', body: JSON.stringify({ ok: false, error: { code: 'FIXTURE_CATALOG_DOWN', message: 'Fixture catalog temporarily unavailable' } }) }))
    await page.getByTestId('harness-models-refresh').click()
    await page.getByTestId('harness-models-retry').waitFor()
    await screenshot('catalog-error')
    await page.unroute('**/zodo-bridge/v1/harness/models')
    await page.getByTestId('harness-models-retry').click()
    await page.getByTestId('harness-models-retry').waitFor({ state: 'hidden' })
    await page.getByTestId('harness-models-refresh').waitFor()
  })

  await check('21 navigation actions at 390px, 800px and 600px container', async () => {
    const sizes = [{ width: 390, height: 844 }, { width: 800, height: 1000 }, { width: 1440, height: 1000, container: 600 }]
    for (const size of sizes) {
      await page.setViewportSize({ width: size.width, height: size.height })
      await page.locator('#root').evaluate((element, width) => { element.style.width = width ? width + 'px' : '' }, size.container ?? null)
      for (const { id } of report.navigation) {
        await nav(id)
        assert.match(await page.getByTestId('nav-' + id).getAttribute('class'), /zw-nav-active/)
        assert.ok((await page.locator('main h1').first().innerText()).trim())
        const metrics = await page.locator('nav[aria-label="Zodo 业务入口"]').evaluate(element => ({ navWidth: element.clientWidth, navScroll: element.scrollWidth, viewport: innerWidth, document: document.documentElement.scrollWidth }))
        assert.ok(metrics.navScroll <= metrics.navWidth + 1, id + ' nav overflow: ' + JSON.stringify(metrics))
        assert.ok(metrics.document <= metrics.viewport + 1, id + ' document overflow: ' + JSON.stringify(metrics))
      }
      report.responsiveNavigation ??= []
      report.responsiveNavigation.push({ viewport: size.width, container: size.container ?? size.width, clicked: report.navigation.length, documentOverflow: false, navigationOverflow: false })
      await screenshot('navigation-' + size.width + (size.container ? '-container600' : ''))
    }
    await page.locator('#root').evaluate(element => { element.style.width = '' })
    await page.setViewportSize({ width: 390, height: 844 })
    await nav('today')
    await page.getByTestId('top-capture').click()
    const dialog = page.getByRole('dialog', { name: '快速收集' })
    await dialog.waitFor()
    const box = await dialog.boundingBox()
    assert.ok(box && box.x >= -1 && box.x + box.width <= 391)
    await screenshot('narrow-capture')
    await page.keyboard.press('Escape'); await dialog.waitFor({ state: 'hidden' })
    await nav('settings')
    const dimensions = await page.evaluate(() => ({ width: innerWidth, scroll: document.documentElement.scrollWidth }))
    assert.ok(dimensions.scroll <= dimensions.width + 1, JSON.stringify(dimensions))
    await screenshot('narrow-settings')
    await page.setViewportSize({ width: 1440, height: 1000 })
  })

  await check('Offline capture retains local draft and recovers without duplicate', async () => {
    const text = 'Fixture offline draft unique'
    await page.getByTestId('top-capture').click(); await page.getByTestId('capture-input').fill(text)
    await page.route('**/zodo-bridge/v1/rpc', route => route.abort('internetdisconnected'))
    await page.getByTestId('capture-save').click()
    await page.getByText(/草稿只保存在本机，尚未提交到 Core/).waitFor()
    await page.getByTestId('capture-draft-state').waitFor()
    assert.equal(await page.getByTestId('capture-input').inputValue(), text)
    await screenshot('offline-draft')
    await page.unroute('**/zodo-bridge/v1/rpc')
    assert.equal((await snapshot()).inbox.filter(row => row.text === text).length, 0)
    await page.reload({ waitUntil: 'domcontentloaded' }); await connected()
    await page.getByTestId('top-capture').click()
    assert.equal(await page.getByTestId('capture-input').inputValue(), text, 'draft survives reload')
    await page.getByTestId('capture-save').click()
    await page.getByRole('dialog', { name: '快速收集' }).waitFor({ state: 'hidden' })
    assert.equal((await snapshot()).inbox.filter(row => row.text === text).length, 1)
  })

  await check('No client JavaScript errors or external network', async () => {
    assert.deepEqual(report.pageErrors, [])
    assert.deepEqual(report.unexpectedNetwork, [])
  })
} finally {
  await browser?.close()
  if (fixture) {
    await fixture.stop()
    report.cleanup = { coreStopped: true, serverStopped: true, dataRemoved: !(await stat(fixture.dataDirectory).then(() => true, () => false)) }
    assert.equal(report.cleanup.dataRemoved, true)
  }
  report.passed = report.cases.filter(row => row.passed).length
  report.failed = report.cases.filter(row => !row.passed).length
  await writeFile(path.join(output, 'report.json'), JSON.stringify(report, null, 2) + '\n')
  console.log(JSON.stringify({ passed: report.passed, failed: report.failed, navigation: report.navigation.length, output, cleanup: report.cleanup, failures: report.cases.filter(row => !row.passed) }))
}
if (report.failed) process.exitCode = 1
