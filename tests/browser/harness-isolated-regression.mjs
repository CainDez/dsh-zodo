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
import { fixtureFailure, fixtureSummary } from './harness-isolated-diagnostics.mjs'

const here = path.dirname(fileURLToPath(import.meta.url))
const output = path.join(here, 'harness-fixture-runtime/results', new Date().toISOString().replace(/[:.]/g, '-'))
await mkdir(output, { recursive: true })
const report = { fixture: 'real Core + real index.js bridge + real client.js + React 18', fullHost: false,
  exclusions: ['DSH authentication/login', 'DSH router and sidebar composition', 'live model dispatch', 'native desktop shell'],
  cases: [], navigation: [], pageErrors: [], unexpectedNetwork: [] }
let browser, fixture, page
let stage = 'artifacts'
report.runtime = { node: process.version, platform: process.platform, arch: process.arch, timezone: 'Asia/Shanghai' }
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
// Fixture-only command channel for setup and negative Core submissions; same bridge envelope the UI uses.
const command = async (operation, payload = {}) => page.evaluate(async ({ operation, payload }) => {
  const response = await fetch('/zodo-bridge/v1/rpc', { method: 'POST', credentials: 'same-origin',
    headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ kind: 'command', envelope: {
      schema: 'zodo.command/1', request_id: crypto.randomUUID(), idempotency_key: crypto.randomUUID(),
      workspace_id: 'ws_local', operation, operation_version: 1, payload,
    } }) })
  const body = await response.json()
  if (!response.ok || !body.ok) throw new Error(body.error?.message ?? 'command failed')
  return body.result
}, { operation, payload })
const getModel = () => page.evaluate(async () => (await (await fetch('/zodo-bridge/v1/model')).json()).result)
const postModel = config => page.evaluate(async config => {
  const response = await fetch('/zodo-bridge/v1/model', { method: 'POST', credentials: 'same-origin',
    headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(config) })
  const body = await response.json()
  if (!response.ok || !body.ok) throw new Error(body.error?.message ?? 'model update failed')
  return body.result
}, config)
// A second isolated settings surface saves direct v2 config, never a copied endpoint/role wrapper.
const saveModelFromOutside = async mutate => {
  const config = structuredClone((await getModel()).config)
  mutate(config)
  return postModel(config)
}
const setAdvanced = async (testId, open) => {
  const details = page.getByTestId(testId)
  if (((await details.getAttribute('open')) !== null) !== open) await details.locator('summary').click()
}
const fieldInputValueEventually = (label, value) => page.waitForFunction(({ label, value }) =>
  [...document.querySelectorAll('label')].some(element =>
    element.textContent.includes(label) && element.querySelector('input')?.value === value), { label, value })
const dshSelection = (provider, model) => JSON.stringify({ provider, model })
const saveModelInUI = async () => {
  const pending = page.waitForResponse(response => response.url().endsWith('/zodo-bridge/v1/model') && response.request().method() === 'POST')
  await page.getByTestId('model-save').click()
  const response = await pending
  assert.equal(response.status(), 200)
  const body = await response.json()
  assert.equal(body.ok, true)
  return body.result
}
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
  stage = 'fixture-start'
  fixture = await startFixture({ binary: process.env.ZODO_FIXTURE_CORE_BIN })
  stage = 'browser-launch'
  browser = await chromium.launch({ executablePath: process.env.ZODO_CHROME ?? 'C:/Program Files/Google/Chrome/Application/chrome.exe', headless: true })
  report.runtime.browser = browser.version()
  stage = 'browser-context'
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

  stage = 'cases'
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

  await check('Agenda date navigation and row details are local-only under Asia/Shanghai', async () => {
    // Reuse the synthetic capacity and allocation created through the real bridge
    // above. This checks rendered UI wiring, not a second projection implementation.
    const before = await snapshot()
    const capacity = before.capacity_blocks.find(row => row.title === 'Fixture future capacity')
    const allocation = before.allocations.find(row => row.task_id === taskId)
    assert.ok(capacity && allocation, 'approved fixture plan prerequisite')
    await nav('schedule')
    const dates = await page.evaluate(() => {
      const format = date => `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`
      const now = new Date()
      now.setHours(12, 0, 0, 0)
      return [-7, 0, 7].map(offset => {
        const first = new Date(now); first.setDate(first.getDate() + offset)
        const last = new Date(first); last.setDate(last.getDate() + 6)
        return `${format(first)} 至 ${format(last)}`
      })
    })
    const waitRange = expected => page.waitForFunction(value => document.querySelector('[data-testid="schedule-range"]')?.textContent === value, expected)
    const writes = []
    const recordWrite = request => {
      if (!request.url().endsWith('/zodo-bridge/v1/rpc') || request.method() !== 'POST') return
      const body = request.postDataJSON()
      if (body?.kind === 'command') writes.push(body.envelope?.operation ?? 'unknown command')
    }
    page.on('request', recordWrite)
    try {
      await page.getByTestId('schedule-today').click(); await waitRange(dates[1])
      await page.getByTestId('time-capacity-' + capacity.id).waitFor()
      await page.getByTestId('schedule-next').click(); await waitRange(dates[2])
      assert.equal(await page.getByTestId('time-capacity-' + capacity.id).count(), 0)
      await page.getByTestId('schedule-previous').click(); await waitRange(dates[1])
      await page.getByTestId('time-capacity-' + capacity.id).waitFor()
      await page.getByTestId('schedule-previous').click(); await waitRange(dates[0])
      await page.getByTestId('schedule-today').click(); await waitRange(dates[1])
      const capacityCard = page.getByTestId('time-capacity-' + capacity.id)
      assert.match(await capacityCard.innerText(), /14:00.*16:00/)
      await capacityCard.focus(); await capacityCard.press('Space')
      const detail = page.getByRole('dialog', { name: '时间对象详情' })
      await detail.waitFor()
      assert.match(await detail.innerText(), /Fixture future capacity/)
      assert.match(await detail.innerText(), /容量块/)
      await detail.getByRole('button', { name: '关闭', exact: true }).last().click()
      await detail.waitFor({ state: 'hidden' })
      await page.getByTestId('time-allocation-' + allocation.id).click()
      await detail.waitFor()
      assert.match(await detail.innerText(), /Fixture task edited/)
      assert.match(await detail.innerText(), /任务分配/)
      await detail.getByTestId('time-detail-task-time').waitFor()
      await detail.getByRole('button', { name: '关闭', exact: true }).last().click()
      await detail.waitFor({ state: 'hidden' })
      await screenshot('agenda-navigation-and-details')
      assert.deepEqual(writes, [], 'navigation and drilldown never send a write RPC')
      const after = await snapshot()
      assert.equal(after.workspace.revision, before.workspace.revision)
      assert.deepEqual(after.capacity_blocks, before.capacity_blocks)
      assert.deepEqual(after.allocations, before.allocations)
    } finally { page.off('request', recordWrite) }
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

  await check('Harness subscription catalog selection is draft-only without endpoint or key copying', async () => {
    await nav('models')
    const before = await getModel()
    assert.deepEqual(before.config, { version: 2, source: { kind: 'dsh', provider: null, model: null } })
    const catalog = await page.evaluate(async () => await (await fetch('/zodo-bridge/v1/harness/models')).json())
    assert.equal(catalog.schema, 'zodo.harness-model-catalog/1')
    assert.equal('default_route' in catalog, false, 'catalog never inherits the chat model')
    const group = catalog.groups.find(row => row.provider === 'fixture-subscription')
    assert.equal(group.supported, true)
    assert.equal(group.models.find(row => row.id === 'fixture-subscribed-model').supported, true)
    assert.doesNotMatch(JSON.stringify(group), /base_url|baseURL|api_key_env|endpoint/)
    const select = page.getByTestId('model-dsh-select')
    const value = dshSelection('fixture-subscription', 'fixture-subscribed-model')
    await select.locator('option').filter({ hasText: 'Fixture subscription model' }).waitFor({ state: 'attached' })
    assert.equal(await select.locator('option').filter({ hasText: 'Fixture subscription model' }).isEnabled(), true)
    await select.selectOption(value)
    assert.deepEqual((await getModel()).config, before.config, 'selecting a source/model does not save')
    assert.equal(await page.getByLabel('API 地址', { exact: true }).count(), 0)
    assert.equal(await page.getByLabel('密钥环境变量名（可选）', { exact: true }).count(), 0)
    await screenshot('models-subscription-draft')
    await page.getByTestId('model-discard').click()
    await page.waitForFunction(() => document.querySelector('[data-testid="model-dsh-select"]').value === '')
  })

  await check('Catalog failure has usable retry and recovery', async () => {
    await nav('models')
    await page.route('**/zodo-bridge/v1/harness/models', route => route.fulfill({ status: 503, contentType: 'application/json', body: JSON.stringify({ ok: false, error: { code: 'FIXTURE_CATALOG_DOWN', message: 'Fixture catalog temporarily unavailable' } }) }))
    try {
      await page.getByTestId('model-refresh').click()
      await page.getByTestId('model-catalog-error').waitFor()
      await page.getByText('Fixture catalog temporarily unavailable', { exact: false }).first().waitFor()
      assert.equal(await page.getByTestId('model-refresh').isEnabled(), true)
      assert.deepEqual((await getModel()).config.source, { kind: 'dsh', provider: null, model: null })
      await screenshot('catalog-error')
    } finally { await page.unroute('**/zodo-bridge/v1/harness/models') }
    await page.getByTestId('model-refresh').click()
    await page.getByTestId('model-catalog-error').waitFor({ state: 'hidden' })
    await page.getByTestId('model-dsh-select').locator('option').filter({ hasText: 'Fixture subscription model' }).waitFor({ state: 'attached' })
  })

  await check('Independent settings defaults: DSH unselected, privacy deny-all, preflight only', async () => {
    await nav('models')
    assert.equal(await page.locator('main h1').first().innerText(), '模型')
    assert.equal(await page.getByTestId('model-source-dsh').isChecked(), true)
    assert.equal(await page.getByTestId('model-source-custom').isChecked(), false)
    assert.equal(await page.getByTestId('model-dsh-select').inputValue(), '')
    assert.equal(await page.locator('main input[type="radio"]').count(), 2, 'only DSH/custom sources')
    assert.equal(await page.getByTestId('privacy-save').count(), 0)
    const model = await getModel()
    assert.equal(model.schema, 'zodo.model/1')
    assert.equal(model.effective.status, 'preflight')
    assert.equal(model.effective.connection_tested, false)
    assert.equal(model.effective.selected.source, 'dsh')
    assert.equal(model.effective.selected.provider, null)
    assert.equal(model.effective.selected.model, null)
    assert.equal(model.effective.selected.available, false)
    assert.ok(model.effective.selected.reason)
    for (const operation of ['decompose', 'clarify']) {
      assert.equal(model.effective.operations[operation].available, false)
      assert.ok(model.effective.operations[operation].reason)
    }
    await page.getByTestId('model-effective-status').waitFor()
    await nav('privacy')
    assert.equal(await page.locator('main h1').first().innerText(), '隐私')
    for (const id of ['goals', 'projects', 'captures']) assert.equal(await page.getByTestId('privacy-' + id).isChecked(), false)
    assert.equal(await page.getByTestId('privacy-advanced').getAttribute('open'), null)
    assert.equal(await page.getByTestId('model-save').count(), 0, 'privacy is not another ModelsView entry')
    assert.equal(await page.getByTestId('model-source-dsh').count(), 0)
    assert.deepEqual((await snapshot()).workspace.policy, {
      model_access: { goals: false, projects: false, captures: false }, diagnostic_capture: false,
    })
    assert.equal(fixture.llmCalls.defaultSelections, 0)
    await screenshot('privacy-default')
  })

  await check('DSH subscription and custom model saves change no privacy or content state', async () => {
    await nav('models')
    const baseline = await snapshot()
    await page.getByTestId('model-dsh-select').selectOption(dshSelection('fixture-subscription', 'fixture-subscribed-model'))
    const subscribed = await saveModelInUI()
    assert.equal(subscribed.schema, 'zodo.model/1')
    assert.deepEqual(subscribed.config, { version: 2, source: { kind: 'dsh', provider: 'fixture-subscription', model: 'fixture-subscribed-model' } })
    await page.getByTestId('model-source-custom').click()
    await page.getByLabel('API 地址', { exact: true }).fill('http://127.0.0.1:9/v1')
    await page.getByLabel('模型 ID', { exact: true }).fill('fixture-custom-model')
    await setAdvanced('model-advanced', true)
    await page.getByLabel('密钥环境变量名（可选）', { exact: true }).fill('')
    assert.deepEqual((await getModel()).config, subscribed.config, 'custom fields remain a draft before save')
    const custom = await saveModelInUI()
    assert.deepEqual(custom.config, { version: 2, source: { kind: 'custom', base_url: 'http://127.0.0.1:9/v1', model: 'fixture-custom-model', api_key_env: null } })
    const after = await snapshot()
    assert.deepEqual(after.workspace.policy, baseline.workspace.policy)
    assert.equal(after.workspace.policy_revision, baseline.workspace.policy_revision)
    assert.deepEqual(after.notes, baseline.notes, 'model settings cannot reclassify notes')
    assert.equal(fixture.llmCalls.streams.length, 0, 'saving a model never dispatches it')
    await screenshot('models-custom-saved')
  })

  await check('Privacy independent booleans: failed save leaves state, retry saves without model changes', async () => {
    const modelBefore = (await getModel()).config
    await nav('privacy')
    const before = (await snapshot()).workspace
    await page.getByTestId('privacy-goals').check()
    await page.getByTestId('privacy-captures').check()
    assert.equal(await page.getByTestId('privacy-projects').isChecked(), false)
    await setAdvanced('privacy-advanced', true)
    await page.getByTestId('privacy-diagnostics').check()
    assert.deepEqual((await snapshot()).workspace.policy, before.policy, 'privacy edits are draft-only')
    await page.getByTestId('zodo-toast').waitFor({ state: 'hidden', timeout: 6000 })
    await page.route('**/zodo-bridge/v1/rpc', route => {
      if ((route.request().postData() ?? '').includes('workspace.update')) return route.fulfill({ status: 502, contentType: 'application/json',
        body: JSON.stringify({ ok: false, error: { code: 'FIXTURE_PRIVACY_SAVE_FAILED', message: 'fixture injected privacy save failure' } }) })
      return route.continue()
    })
    try {
      await page.getByTestId('privacy-save').click()
      await page.getByText('fixture injected privacy save failure', { exact: false }).first().waitFor()
      assert.equal(await page.getByTestId('zodo-toast').count(), 0, 'failed save is not success')
      assert.deepEqual((await snapshot()).workspace.policy, before.policy)
    } finally { await page.unroute('**/zodo-bridge/v1/rpc') }
    const saved = page.waitForResponse(response => response.url().endsWith('/zodo-bridge/v1/rpc') && (response.request().postData() ?? '').includes('workspace.update'))
    await page.getByTestId('privacy-save').click()
    assert.equal((await saved).status(), 200)
    await page.getByTestId('privacy-status').getByText('隐私设置已保存；模型选择未改变。', { exact: true }).waitFor()
    assert.deepEqual((await snapshot()).workspace.policy, {
      model_access: { goals: true, projects: false, captures: true }, diagnostic_capture: true,
    })
    // Change two context permissions without a global mode or touching the third one.
    await page.getByTestId('privacy-projects').check()
    await page.getByTestId('privacy-captures').uncheck()
    const secondSave = page.waitForResponse(response => response.url().endsWith('/zodo-bridge/v1/rpc') && (response.request().postData() ?? '').includes('workspace.update'))
    await page.getByTestId('privacy-save').click()
    assert.equal((await secondSave).status(), 200)
    await page.getByTestId('privacy-status').getByText('隐私设置已保存；模型选择未改变。', { exact: true }).waitFor()
    assert.deepEqual((await snapshot()).workspace.policy, {
      model_access: { goals: true, projects: true, captures: false }, diagnostic_capture: true,
    })
    assert.deepEqual((await getModel()).config, modelBefore, 'privacy cannot replace the chosen model')
    assert.equal(fixture.llmCalls.streams.length, 0)
    await screenshot('privacy-independent-saved')
  })

  await check('Source and privacy persist separately across navigation and reload', async () => {
    const modelBefore = (await getModel()).config
    const policyBefore = (await snapshot()).workspace.policy
    await nav('models')
    await fieldInputValueEventually('模型 ID', 'fixture-custom-model')
    assert.equal(await page.getByTestId('model-source-custom').isChecked(), true)
    assert.equal(await page.getByLabel('模型 ID', { exact: true }).inputValue(), 'fixture-custom-model')
    await nav('privacy')
    assert.equal(await page.getByTestId('privacy-goals').isChecked(), true)
    assert.equal(await page.getByTestId('privacy-captures').isChecked(), false)
    await page.reload({ waitUntil: 'domcontentloaded' }); await connected()
    await nav('models')
    await fieldInputValueEventually('模型 ID', 'fixture-custom-model')
    assert.equal(await page.getByTestId('model-source-custom').isChecked(), true)
    await nav('privacy')
    assert.equal(await page.getByTestId('privacy-projects').isChecked(), true)
    assert.deepEqual((await getModel()).config, modelBefore)
    assert.deepEqual((await snapshot()).workspace.policy, policyBefore)
  })

  await check('Legacy config and global privacy schemas are rejected without writes', async () => {
    const modelBefore = (await getModel()).config
    const before = (await snapshot()).workspace
    const invalidConfigs = [
      { version: 1, local: null, cloud: null },
      { ...modelBefore, roles: { decompose: 'auto' } },
      { ...modelBefore, local: null, cloud: null },
      { editable: modelBefore },
      { version: 2, source: { ...modelBefore.source, api_key: 'fixture-not-a-real-key' } },
    ]
    for (const config of invalidConfigs) await assert.rejects(() => postModel(config), /./)
    const invalidPolicies = [
      { model_route: 'local_only', allowed_cloud_providers: [], diagnostic_capture: false },
      { model_route: 'cloud_assisted', allowed_cloud_providers: ['fixture-subscription'], diagnostic_capture: false },
      { model_route: 'cloud_allowed', private_notes: 'device_only', diagnostic_capture: false },
      { ...before.policy, private_notes: 'device_only' },
      { ...before.policy, allowed_cloud_providers: [] },
      { ...before.policy, model_access: { ...before.policy.model_access, notes: true } },
    ]
    for (const policy of invalidPolicies) await assert.rejects(() => command('workspace.update', { settings: null, policy }), /./)
    const after = (await snapshot()).workspace
    assert.deepEqual((await getModel()).config, modelBefore)
    assert.deepEqual(after.policy, before.policy)
    assert.equal(after.policy_revision, before.policy_revision)
    await screenshot('settings-legacy-rejected')
  })

  await check('Cross-window source save: clean refresh syncs direct config before editing', async () => {
    await nav('models')
    const outside = await saveModelFromOutside(config => {
      config.source.model = 'fixture-model-B'
      config.source.api_key_env = 'ZODO_FIXTURE_NO_KEY'
    })
    assert.equal(outside.config.source.model, 'fixture-model-B')
    await page.getByTestId('model-refresh').click()
    await fieldInputValueEventually('模型 ID', 'fixture-model-B')
    await page.getByLabel('模型 ID', { exact: true }).fill('fixture-model-C')
    const saved = await saveModelInUI()
    assert.equal(saved.config.source.model, 'fixture-model-C')
    assert.equal(saved.config.source.api_key_env, 'ZODO_FIXTURE_NO_KEY', 'refresh prevents rollback of an unrelated outside edit')
  })

  await check('Stale source draft refuses POST, keeps draft, and discard reloads authoritative config', async () => {
    await nav('models')
    await fieldInputValueEventually('模型 ID', 'fixture-model-C')
    await page.getByLabel('模型 ID', { exact: true }).fill('fixture-model-D-draft')
    await saveModelFromOutside(config => { config.source.model = 'fixture-model-external-D' })
    const seen = { get: 0, post: 0 }
    const onModelRequest = request => {
      if (!request.url().endsWith('/zodo-bridge/v1/model')) return
      if (request.method() === 'POST') seen.post++; else seen.get++
    }
    page.on('request', onModelRequest)
    try {
      await page.getByTestId('model-save').click()
      await page.getByTestId('model-stale').waitFor()
    } finally { page.off('request', onModelRequest) }
    assert.equal(seen.post, 0)
    assert.ok(seen.get >= 1, 'save re-checks the server baseline')
    assert.equal(await page.getByLabel('模型 ID', { exact: true }).inputValue(), 'fixture-model-D-draft')
    assert.equal(await page.getByTestId('model-save').isDisabled(), true)
    assert.equal((await getModel()).config.source.model, 'fixture-model-external-D')
    await page.getByTestId('model-discard').click()
    await fieldInputValueEventually('模型 ID', 'fixture-model-external-D')
    await page.getByTestId('model-stale').waitFor({ state: 'hidden' })
    assert.equal(await page.getByTestId('model-save').isDisabled(), true)
    await screenshot('models-stale-discarded')
  })

  await check('Independent advanced sections toggle by keyboard and fit narrow viewport', async () => {
    await nav('models')
    await setAdvanced('model-advanced', false)
    const summary = page.getByTestId('model-advanced').locator('summary')
    await summary.focus(); await page.keyboard.press('Enter')
    await page.waitForFunction(() => document.querySelector('[data-testid="model-advanced"]').open === true)
    assert.equal(await page.getByLabel('密钥环境变量名（可选）', { exact: true }).inputValue(), 'ZODO_FIXTURE_NO_KEY')
    assert.equal(await page.locator('main input[type="password"]').count(), 0, 'there is no manual secret input')
    await page.setViewportSize({ width: 390, height: 844 })
    for (const id of ['models', 'privacy']) {
      await nav(id)
      await setAdvanced(id === 'models' ? 'model-advanced' : 'privacy-advanced', true)
      const dimensions = await page.evaluate(() => ({ width: innerWidth, scroll: document.documentElement.scrollWidth }))
      assert.ok(dimensions.scroll <= dimensions.width + 1, id + ' advanced overflow: ' + JSON.stringify(dimensions))
      await screenshot(id + '-advanced-narrow')
    }
    await page.setViewportSize({ width: 1440, height: 1000 })
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
    assert.equal(fixture.llmCalls.streams.length, 0, 'settings/navigation never dispatch mock or paid model calls')
    assert.equal(fixture.llmCalls.defaultSelections, 0, 'the current chat selection is never read')
  })
} catch (error) {
  report.fatal = fixtureFailure(stage, error)
  throw error
} finally {
  // Attempt both cleanup steps even if one fails, and preserve startup evidence.
  try { await browser?.close() } catch (error) {
    report.cleanupFailure = fixtureFailure('browser-cleanup', error)
  }
  if (fixture) {
    try {
      await fixture.stop()
      report.cleanup = { coreStopped: true, serverStopped: true, dataRemoved: !(await stat(fixture.dataDirectory).then(() => true, () => false)) }
      assert.equal(report.cleanup.dataRemoved, true)
    } catch (error) {
      report.cleanupFailure = fixtureFailure('fixture-cleanup', error)
    }
  }
  report.fatal ??= report.cleanupFailure
  Object.assign(report, fixtureSummary(report.cases, report.fatal))
  await writeFile(path.join(output, 'report.json'), JSON.stringify(report, null, 2) + '\n')
  console.log(JSON.stringify({ status: report.status, executed: report.executed, fatal: report.fatal, passed: report.passed, failed: report.failed, navigation: report.navigation.length, output, cleanup: report.cleanup, failures: report.cases.filter(row => !row.passed) }))
}
if (report.status !== 'passed') process.exitCode = 1
