// Invoke via official-web-acceptance.py, which supplies the private launcher URL.
import assert from 'node:assert/strict'
import { once } from 'node:events'
import { createServer } from 'node:net'
import { resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

// Never emit browser errors: recent Chromium/Playwright logs can contain
// arbitrary paths, URLs, or display configuration. Emit only known labels.
function launchReason(error) {
  const err = String(error)
  if (/Socket path too long|sun_path|AF_UNIX.*too long/i.test(err)) return 'unix-socket-path'
  if (/No usable sandbox|SUID sandbox helper binary|Failed to move to new namespace/i.test(err)) return 'sandbox-unavailable'
  if (/Operation not permitted|Permission denied/i.test(err)) return 'os-permission-denied'
  if (/error while loading shared libraries/i.test(err)) return 'shared-library-unavailable'
  if (/TimeoutError/i.test(err)) return 'timeout'
  return 'unclassified'
}

function reportFailure(stage, error) {
  // No URL, browser exception, frame dump, screenshot, or session data in output.
  const message = String(error)
  const reason = stage === 'sandbox-launch' ? launchReason(error)
    : /strict mode violation/i.test(message) ? 'multiple-matches'
    : /intercepts pointer events/i.test(message) ? 'pointer-intercepted'
    : /TimeoutError/i.test(message) ? 'timeout' : 'unclassified'
  console.log('FAIL ' + stage + ' ' + reason)
  process.exitCode = 1
}

let phase = 'local-launch-token'
let browser, denyProxy
try {
  const chunks = []
  for await (const chunk of process.stdin) chunks.push(chunk)
  const launch = Buffer.concat(chunks).toString('utf8')
  assert.match(launch, /^http:\/\/127\.0\.0\.1:[0-9]+\/\?token=[A-Za-z0-9_-]+$/)
  const { origin, host } = new URL(launch)
  const socketOrigin = origin.replace(/^http:/, 'ws:')
  const { chromium } = await import(pathToFileURL(resolve(process.argv[3], 'index.mjs')))
  // Playwright routing does not intercept every redirect hop. Chromium's test-
  // instance proxy makes every other host fail closed, including redirects.
  phase = 'browser-egress-guard'
  denyProxy = createServer(socket => socket.destroy()).listen(0, '127.0.0.1')
  await once(denyProxy, 'listening')
  phase = 'sandbox-launch'
  browser = await chromium.launch({
    executablePath: process.argv[2], chromiumSandbox: true, timeout: 12000,
    args: [
      '--dns-prefetch-disable',
      '--proxy-server=http://127.0.0.1:' + denyProxy.address().port,
      '--proxy-bypass-list=<-loopback>;' + host,
    ],
  })
  console.log('PASS sandbox-launch')
  phase = 'sandboxed-browser-context'
  const context = await browser.newContext({
    serviceWorkers: 'block', locale: 'zh-CN', timezoneId: 'Asia/Shanghai', viewport: { width: 1440, height: 960 },
  })
  context.setDefaultTimeout(4500)
  phase = 'browser-egress-routes'
  await context.route('**/*', route => new URL(route.request().url()).origin === origin
    ? route.continue() : route.abort('blockedbyclient'))
  await context.routeWebSocket('**/*', socket => {
    if (new URL(socket.url()).origin === socketOrigin) socket.connectToServer()
    else socket.close()
  })

  phase = 'official-authenticated-navigation'
  const page = await context.newPage()
  const response = await page.goto(launch, { waitUntil: 'domcontentloaded', timeout: 15000 })
  assert.equal(response?.status(), 200)
  assert.equal(new URL(page.url()).origin, origin)

  phase = 'official-zodo-boot-graph'
  const ident = '@zodo/harness-plugin'
  const graph = await page.evaluate(() => globalThis.__DSH_BOOT__)
  assert.equal(graph.release.version, '0.2.2-rc.5')
  const entries = graph.entries.filter(item => item.id === ident)
  const batches = graph.batches.filter(batch => batch.entries.includes(ident))
  assert.equal(entries.length, 1)
  assert.equal(batches.length, 1)
  for (const row of [...entries, ...batches]) {
    const url = new URL(row.url, origin)
    assert.equal(url.origin, origin)
    assert.match(url.pathname, /^\/plugins\//)
  }
  console.log('PASS official-zodo-boot-graph')

  const workbench = page.locator('[data-zodo-workbench="a2"]')
  // Only these shipped, optional onboarding actions are permitted in this fresh
  // profile. Do not sign in, set up a model, or suppress the host overlay.
  const onboarding = [
    ['preview-notice', /^(预览版说明|Preview Notice)$/, /^(继续|Continue)$/],
    ['optional-account', /^(开始使用|Get started)$/, /^(关闭|Close)$/],
    ['optional-model', /^(添加一个 API Key 开始使用|Add an API key to get started)$/, /^(稍后配置|Configure later)$/],
  ].map(([id, name, action]) => ({ id, dialog: page.getByRole('dialog', { name }), action }))
  const [preview, account, model] = onboarding
  const completedOnboarding = new Set()
  const ensureHostInteractive = async (observeOnboarding = false, afterReload = false) => {
    phase = 'official-host-onboarding'
    if (observeOnboarding) {
      // Settings/session readiness can arrive after the sidebar first renders.
      const expected = afterReload ? preview.dialog.or(account.dialog).or(model.dialog).first() : preview.dialog
      await expected.waitFor({ timeout: 5500 }).catch(error => {
        if (error.name !== 'TimeoutError') throw error
      })
    }
    try {
      for (const { id, dialog, action } of onboarding) {
        if (!await dialog.isVisible()) continue
        phase = 'official-onboarding-' + id
        assert(!completedOnboarding.has(id))
        await dialog.getByRole('button', { name: action }).click()
        await dialog.waitFor({ state: 'hidden' })
        completedOnboarding.add(id)
        console.log('PASS ' + phase)
        if (id === 'preview-notice') {
          await account.dialog.or(model.dialog).first().waitFor({ timeout: 4500 }).catch(error => {
            if (error.name !== 'TimeoutError') throw error
          })
        }
      }
      phase = 'official-host-interactive'
      const ready = await page.waitForFunction(() => {
        const root = document.getElementById('root')
        const zodo = document.querySelector('[data-zodo-workbench="a2"]')
        return root && !root.inert && !zodo?.closest('[inert]')
      }, undefined, { timeout: 4500 })
      await ready.dispose()
    } catch (error) {
      const labels = await page.evaluate(() => {
        const drawer = document.querySelector('[data-sidebar-drawer]')
        const root = document.getElementById('root')
        const workbench = document.querySelector('[data-zodo-workbench="a2"]')
        return [
          innerWidth >= 768 ? 'desktop-viewport' : 'narrow-viewport',
          root?.inert ? 'root-inert' : 'root-not-inert',
          !drawer ? 'no-drawer' : drawer.hasAttribute('data-sidebar-collapsed') ? 'drawer-closed' : 'drawer-open',
          workbench?.closest('[inert]') ? 'workbench-inert' : 'workbench-not-inert',
        ]
      }).catch(() => ['host-markers-unavailable'])
      const visible = []
      for (const { id, dialog } of onboarding) {
        if (await dialog.isVisible().catch(() => false)) visible.push(id)
      }
      console.log('DIAG official-host ' + [...labels, ...(visible.length ? visible : ['no-known-dialog'])].join(' '))
      throw error
    }
  }
  await ensureHostInteractive(true)
  phase = 'official-sidebar-zodo'
  await page.getByRole('button', { name: 'Zodo', exact: true }).click({ timeout: 12000 })
  console.log('PASS official-sidebar-zodo')
  const heading = name => workbench.getByRole('heading', { name, level: 1, exact: true })
  const sidebar = workbench.getByRole('navigation', { name: 'Zodo 业务入口' })
  const topbar = workbench.locator('.zw-topbar')
  const openScheduleOptions = async () => {
    const options = workbench.getByTestId('schedule-options')
    if (!(await options.evaluate(node => node.parentElement.open))) await options.click()
  }
  phase = 'official-today-heading'
  await heading('今天').waitFor()
  phase = 'official-core-connected'
  await workbench.getByText('Core 已连接', { exact: true }).waitFor()
  console.log('PASS official-today-core-connected')

  const snapshot = () => page.evaluate(async () => {
    const response = await fetch('/zodo-bridge/v1/rpc', {
      method: 'POST', credentials: 'same-origin', signal: AbortSignal.timeout(4000),
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ kind: 'query', envelope: {
        schema: 'zodo.query/1', request_id: crypto.randomUUID(), workspace_id: 'ws_local',
        query: 'workspace.snapshot', payload: {},
      } }),
    })
    const body = await response.json()
    if (!response.ok || !body.ok) throw new Error('snapshot failed')
    const { workspace, tasks, capacity_blocks, allocations, now: _now, proposals: _proposals,
      approval_grants: _approvalGrants, ...otherFacts } = body.result.data
    return { revision: workspace.revision, timezone: workspace.timezone ?? workspace.settings?.timezone,
      tasks, capacity: capacity_blocks, allocations, otherFacts }
  })
  phase = 'clean-core-baseline'
  const before = await snapshot()
  assert.equal(before.tasks.length + before.capacity.length + before.allocations.length, 0)
  assert(Number.isInteger(before.revision))
  console.log('PASS clean-core-baseline')
  const taskDialog = { name: '新建任务', exact: true }
  const inWorkbench = workbench.getByRole('dialog', taskDialog)
  const anywhere = page.getByRole('dialog', taskDialog)
  const dialogInDom = page.locator('[role="dialog"][aria-label="新建任务"]')
  const readDialog = async () => {
    if (await inWorkbench.count()) return 'workbench'
    if (await anywhere.count()) return 'page-only'
    if (await dialogInDom.count()) return 'dom-only'
    return 'absent'
  }
  const checkKeyboard = async view => {
    const button = workbench.getByTestId('first-schedule-guide').getByTestId('first-schedule-action')
    let trigger, events
    try {
      phase = view + '-guide-button'
      await button.waitFor()
      assert.equal((await button.innerText()).trim(), '新建任务')
      assert(await button.isEnabled())
      trigger = await button.elementHandle()
      phase = view + '-blocked-by-inert-ancestor'
      assert(!(await trigger.evaluate(node => node.closest('[inert]') !== null)))
      phase = view + '-focus-before-enter'
      await trigger.focus()
      // Record trusted delivery only; do not intercept, synthesize or modify input.
      events = await trigger.evaluateHandle(node => {
        let key, target = false, click = false
        const capture = event => { if (event.isTrusted && event.key === 'Enter') key = event }
        const atTarget = event => { if (event.isTrusted && event.key === 'Enter') target = true }
        const activation = event => { if (event.isTrusted) click = true }
        window.addEventListener('keydown', capture, true)
        node.addEventListener('keydown', atTarget)
        node.addEventListener('click', activation)
        return {
          summary: () => !key ? 'enter-unobserved'
            : click ? 'trusted-click-observed'
              : key.defaultPrevented ? 'enter-default-prevented-no-click'
                : target ? 'enter-target-no-click' : 'enter-missed-target',
          dispose: () => {
            window.removeEventListener('keydown', capture, true)
            node.removeEventListener('keydown', atTarget)
            node.removeEventListener('click', activation)
          },
        }
      })
      assert(await trigger.evaluate(node => node.isConnected && document.activeElement === node))
      phase = view + '-enter'
      await page.keyboard.press('Enter')
      phase = view + '-keyboard-dialog-page'
      await anywhere.waitFor()
      phase = view + '-keyboard-dialog-workbench'
      await inWorkbench.waitFor()
      phase = view + '-escape-dialog'
      await page.keyboard.press('Escape')
      await inWorkbench.waitFor({ state: 'hidden' })
      phase = view + '-restore-focus'
      const focused = await page.waitForFunction(node => document.activeElement === node,
        trigger, { timeout: 3000 })
      await focused.dispose()
      console.log('PASS ' + view + '-keyboard-dialog')
    } catch {
      process.exitCode = 1
      const failed = phase
      const delivery = events ? await events.evaluate(value => value.summary()).catch(() => 'unavailable') : 'not-started'
      const dialog = await readDialog().catch(() => 'unknown')
      console.log('FAIL ' + failed + ' ' + delivery + ' dialog-' + dialog)
      // Pointer is a diagnostic/control only; it cannot clear the keyboard failure.
      // Do not click a CTA underneath an existing (possibly hidden) dialog.
      try {
        if (dialog === 'absent' && (failed.endsWith('-keyboard-dialog-page') ||
              failed.endsWith('-keyboard-dialog-workbench'))) {
          await button.click()
          await inWorkbench.waitFor()
          console.log('PASS ' + view + '-pointer-control-only')
        }
        if (await inWorkbench.count()) {
          await inWorkbench.getByRole('button', { name: '取消', exact: true }).click()
          await inWorkbench.waitFor({ state: 'hidden' })
        }
      } catch {
        const pointerDialog = await readDialog().catch(() => 'unknown')
        console.log('FAIL ' + view + '-pointer-diagnostic dialog-' + pointerDialog)
      }
    } finally {
      if (events) {
        await events.evaluate(value => value.dispose()).catch(() => {})
        await events.dispose()
      }
      await trigger?.dispose()
    }
  }
  // Always compare the real Core snapshot after the interaction attempt,
  // without replacing the original diagnosis if the snapshot also fails.
  let interactionFailed = false
  try {
    await ensureHostInteractive()
    await checkKeyboard('today')
    await ensureHostInteractive()
    phase = 'official-today-open-schedule'
    await workbench.getByTestId('today-open-schedule').click()
    phase = 'official-schedule-heading'
    await heading('日程').waitFor()
    await ensureHostInteractive()
    await checkKeyboard('schedule')
    phase = 'official-return-today-click'
    await workbench.getByTestId('nav-today').click()
    phase = 'official-return-today-heading'
    await heading('今天').waitFor()
    console.log('PASS official-return-today')
  } catch (error) {
    interactionFailed = true
    reportFailure(phase, error)
  } finally {
    phase = 'unchanged-core-snapshot'
    try {
      assert.deepEqual(await snapshot(), before)
      console.log('PASS unchanged-core-snapshot')
    } catch (error) {
      if (!interactionFailed) throw error
      reportFailure(phase, error)
    }
  }

  // Only the real UI may mutate the disposable workspace, and only after
  // the keyboard/cancellation check proved the baseline stayed unchanged.
  if (!process.exitCode) {
    const title = 'Official Web acceptance task'
    const estimate = 37
    const showTasks = async () => {
      phase = 'official-save-tasks-navigation'
      await sidebar.getByTestId('nav-work').click()
      await heading('任务').waitFor()
      await workbench.locator('main').getByTestId('nav-tasks').click()
      await heading('任务').waitFor()
    }
    const checkRow = async (id) => {
      const row = workbench.getByTestId('task-row-' + id)
      await row.getByText(title, { exact: true }).waitFor()
      await row.getByText(`${estimate} 分钟`, { exact: true }).waitFor()
    }
    phase = 'official-save-open-first-use'
    await workbench.getByTestId('first-schedule-guide').getByTestId('first-schedule-action').click()
    await inWorkbench.waitFor()
    phase = 'official-save-fill'
    await inWorkbench.getByLabel('标题', { exact: true }).fill(title)
    await inWorkbench.getByLabel('预计分钟', { exact: true }).fill(String(estimate))
    phase = 'official-save-submit'
    await inWorkbench.getByTestId('task-save').click()
    await inWorkbench.waitFor({ state: 'hidden' })

    phase = 'official-save-exact-core-record'
    const saved = await snapshot()
    assert.equal(saved.tasks.length, 1)
    const [task] = saved.tasks
    assert(typeof task.id === 'string' && task.id.length > 0)
    assert.equal(task.title, title)
    assert.equal(task.estimate_minutes, estimate)
    assert.deepEqual(saved, { ...before, revision: before.revision + 1, tasks: [task] })
    await showTasks()
    phase = 'official-save-visible-task'
    await checkRow(task.id)
    console.log('PASS official-ui-created-one-core-task')

    phase = 'official-save-browser-reload'
    const refreshed = await page.reload({ waitUntil: 'domcontentloaded', timeout: 15000 })
    assert.equal(refreshed?.status(), 200)
    assert.equal(new URL(page.url()).origin, origin)
    // Optional account prompts are session-scoped; the Preview Notice was
    // persisted by the real host and must not require acknowledgement again.
    completedOnboarding.delete('optional-account')
    completedOnboarding.delete('optional-model')
    await ensureHostInteractive(true, true)
    phase = 'official-save-reopen-zodo'
    await page.getByRole('button', { name: 'Zodo', exact: true }).click({ timeout: 12000 })
    await workbench.getByText('Core 已连接', { exact: true }).waitFor()
    await showTasks()
    phase = 'official-save-same-task-after-reload'
    await checkRow(task.id)
    assert.deepEqual(await snapshot(), saved)
    console.log('PASS official-ui-reload-same-task-no-duplicate')

    // Add one working-day capacity block, then schedule inside it with the
    // shipped strict preview and apply UI. No test code sends write RPCs.
    phase = 'official-booking-add-capacity'
    assert.equal(saved.timezone, 'Asia/Shanghai')
    await workbench.getByTestId('nav-schedule').click()
    await heading('日程').waitFor()
    const future = new Date(Date.now() + 86400000)
    const shanghai = { timeZone: saved.timezone }
    while (['Sat', 'Sun'].includes(future.toLocaleDateString('en-US', { ...shanghai, weekday: 'short' }))) {
      future.setTime(future.getTime() + 86400000)
    }
    const day = future.toLocaleDateString('en-CA', shanghai)
    assert.match(day, /^\d{4}-\d{2}-\d{2}$/)
    const capacityTitle = 'Official Web acceptance capacity'
    const instant = clock => Date.parse(`${day}T${clock}:00+08:00`)
    await openScheduleOptions()
    await workbench.getByRole('button', { name: '增加容量', exact: true }).click()
    const capacityEditor = workbench.getByRole('dialog', { name: '增加可用容量', exact: true })
    await capacityEditor.getByLabel('名称', { exact: true }).fill(capacityTitle)
    await capacityEditor.getByLabel('开始', { exact: true }).fill(`${day}T14:00`)
    await capacityEditor.getByLabel('结束', { exact: true }).fill(`${day}T16:00`)
    await capacityEditor.getByLabel('可用分钟', { exact: true }).fill('120')
    await capacityEditor.getByRole('button', { name: '保存', exact: true }).click()
    await capacityEditor.waitFor({ state: 'hidden' })
    phase = 'official-booking-exact-capacity'
    const baseline = await snapshot()
    const [capacity] = baseline.capacity
    assert.ok(capacity && typeof capacity.id === 'string' && capacity.id.length > 0)
    assert.deepEqual(baseline, { ...saved, revision: saved.revision + 1, capacity: [capacity] })
    assert.equal(capacity.title, capacityTitle)
    assert.equal(Date.parse(capacity.start), instant('14:00'))
    assert.equal(Date.parse(capacity.end), instant('16:00'))
    assert.equal(capacity.capacity_minutes, 120)

    const assertFacts = (state, extra = {}) => {
      assert.deepEqual(state, { ...baseline, revision: state.revision, ...extra })
    }
    const isRpc = request => request.method() === 'POST' && request.url() === origin + '/zodo-bridge/v1/rpc'
    const operation = request => {
      if (!isRpc(request)) return undefined
      const body = request.postDataJSON()
      return body?.kind === 'command' ? body.envelope?.operation : undefined
    }
    const commands = []
    const observe = request => {
      const name = operation(request)
      if (name !== undefined) commands.push(name)
    }
    const rpc = origin + '/zodo-bridge/v1/rpc'
    let releaseCommit
    let allowCommit = true
    const commitGate = new Promise(resolve => { releaseCommit = resolve })
    const holdCommit = async route => {
      // Fallback preserves the pre-existing context egress route. Fail closed
      // if the approval-only state could not be verified.
      if (operation(route.request()) !== 'changes.commit' || (allowCommit && await commitGate)) return route.fallback()
      return route.abort()
    }
    page.on('request', observe)
    await page.route(rpc, holdCommit)
    try {
      phase = 'official-booking-slot-input'
      await workbench.getByTestId('schedule-date').fill(day)
      await workbench.getByTestId(`schedule-slot-${day}`).click()
      const slot = workbench.getByRole('dialog', { name: '安排任务时段', exact: true })
      await slot.getByTestId('schedule-slot-task').selectOption(task.id)
      await slot.getByTestId('schedule-slot-capacity').selectOption(capacity.id)
      await slot.getByLabel('开始（工作空间时区）', { exact: true }).fill(`${day}T14:15`)
      await slot.getByLabel('结束（工作空间时区）', { exact: true }).fill(`${day}T14:45`)
      const resolved = await slot.getByTestId('schedule-slot-utc').innerText()
      assert.ok(resolved.includes(new Date(instant('14:15')).toISOString()))
      assert.ok(resolved.includes(new Date(instant('14:45')).toISOString()))
      phase = 'official-booking-preview-without-allocation'
      await slot.getByTestId('schedule-slot-preview').click()
      const review = workbench.getByRole('dialog', { name: '检查提案', exact: true })
      await review.getByTestId('plan-change-summary').getByText(/拟新增 1 项、修改 0 项、移除 0 项/).waitFor()
      assertFacts(await snapshot())
      assert.deepEqual(commands, ['proposal.create'])
      console.log('PASS official-booking-preview-no-allocation')

      phase = 'official-booking-approval-without-allocation'
      await Promise.all([
        page.waitForRequest(request => operation(request) === 'changes.commit', { timeout: 6000 }),
        review.getByTestId('plan-apply').click(),
      ])
      assert.deepEqual(commands, ['proposal.create', 'proposal.approve', 'changes.commit'])
      const approved = await snapshot()
      assertFacts(approved)
      console.log('PASS official-booking-approval-no-allocation')

      phase = 'official-booking-exact-commit'
      const committed = page.waitForResponse(response => operation(response.request()) === 'changes.commit', { timeout: 6000 })
      releaseCommit(true)
      const receipt = await committed
      assert.equal(receipt.status(), 200)
      assert.equal((await receipt.json()).ok, true)
      await review.waitFor({ state: 'hidden' })
      const after = await snapshot()
      const [allocation] = after.allocations
      assert.ok(allocation && typeof allocation.id === 'string' && allocation.id.length > 0)
      assert.equal(after.revision, approved.revision + 1)
      assertFacts(after, { allocations: [allocation] })
      assert.equal(allocation.task_id, task.id)
      assert.equal(allocation.capacity_block_id, capacity.id)
      assert.equal(Date.parse(allocation.start), instant('14:15'))
      assert.equal(Date.parse(allocation.end), instant('14:45'))
      assert.equal(allocation.minutes, 30)
      assert.deepEqual(commands, ['proposal.create', 'proposal.approve', 'changes.commit'])
      await workbench.getByTestId('time-allocation-' + allocation.id).waitFor()
      console.log('PASS official-booking-committed-one-exact-allocation')

      // Reuse the booked allocation. The same scoped request observer stays
      // active; the booking gate has already been released.
      commands.length = 0
      const row = workbench.getByTestId('time-allocation-' + allocation.id)
      const detail = workbench.getByRole('dialog', { name: '时间对象详情', exact: true })
      const move = workbench.getByRole('dialog', { name: '移动任务时段', exact: true })
      const openMove = async () => {
        await row.click()
        await detail.getByTestId('time-detail-move').click()
        await move.waitFor()
        assert.equal(await move.getByTestId('move-slot-capacity').inputValue(), capacity.id)
        await move.getByTestId('move-slot-start').fill(`${day}T15:00`)
        await move.getByText(`新结束（保持原时长）：${day} 15:30`, { exact: true }).waitFor()
      }

      phase = 'official-move-cancel-no-write'
      await openMove()
      await move.getByRole('button', { name: '取消', exact: true }).click()
      await move.waitFor({ state: 'hidden' })
      assert.deepEqual(await snapshot(), after)
      assert.deepEqual(commands, [])
      console.log('PASS official-move-cancel-no-write')

      phase = 'official-discard-create-disposable-preview'
      await openMove()
      const candidateResponse = page.waitForResponse(response => operation(response.request()) === 'proposal.create', { timeout: 6000 })
      await move.getByTestId('move-slot-preview').click()
      const candidateReceipt = await candidateResponse
      assert.equal(candidateReceipt.status(), 200)
      const candidate = (await candidateReceipt.json()).result?.data?.proposal
      assert.ok(candidate?.id && candidate.status === 'validated')
      await review.getByRole('button', { name: '放弃预览', exact: true }).click()
      await review.waitFor({ state: 'hidden' })
      assert.deepEqual(commands, ['proposal.create'])
      assert.deepEqual(await snapshot(), after)

      phase = 'official-discard-confirmation-cancel'
      await topbar.getByTestId('nav-proposals').click()
      await heading('提案').waitFor()
      const discard = workbench.getByTestId('proposal-discard-' + candidate.id)
      const confirmation = workbench.getByRole('dialog', { name: '确认放弃提案', exact: true })
      await discard.click()
      await confirmation.getByRole('button', { name: '取消', exact: true }).click()
      await confirmation.waitFor({ state: 'hidden' })
      await discard.waitFor()
      assert.deepEqual(commands, ['proposal.create'])
      assert.deepEqual(await snapshot(), after)

      phase = 'official-discard-confirm-one-no-schedule-change'
      await discard.click()
      const rejectResponse = page.waitForResponse(response => operation(response.request()) === 'proposal.reject', { timeout: 6000 })
      await confirmation.getByTestId('proposal-discard-confirm').click()
      const rejectReceipt = await rejectResponse
      assert.equal(rejectReceipt.status(), 200)
      const rejected = (await rejectReceipt.json()).result?.data?.proposal
      assert.equal(rejected?.id, candidate.id)
      assert.equal(rejected?.status, 'rejected')
      await confirmation.waitFor({ state: 'hidden' })
      await discard.waitFor({ state: 'hidden' })
      await workbench.getByTestId('proposal-history-' + candidate.id).getByText('rejected', { exact: true }).waitFor()
      assert.deepEqual(commands, ['proposal.create', 'proposal.reject'])
      assert.deepEqual(await snapshot(), after)
      await workbench.getByTestId('nav-schedule').click()
      await heading('日程').waitFor()
      await workbench.getByTestId('schedule-date').fill(day)
      commands.length = 0
      console.log('PASS official-discard-keeps-schedule-and-rejects-once')

      phase = 'official-move-preview-no-allocation-change'
      await openMove()
      await move.getByTestId('move-slot-preview').click()
      const oldSlot = await review.getByTestId('plan-move-before').innerText()
      const nextSlot = await review.getByTestId('plan-move-after').innerText()
      assert.match(oldSlot, /Official Web acceptance capacity.*14:15.*14:45/s)
      assert.match(nextSlot, /Official Web acceptance capacity.*15:00.*15:30/s)
      const movePreview = await snapshot()
      assert.deepEqual(movePreview, { ...after, revision: movePreview.revision })
      assert.deepEqual(commands, ['proposal.create'])
      console.log('PASS official-move-preview-no-allocation-change')

      phase = 'official-move-commit-same-allocation'
      const moveCommit = page.waitForResponse(response => operation(response.request()) === 'changes.commit', { timeout: 6000 })
      await review.getByTestId('plan-apply').click()
      const moveReceipt = await moveCommit
      assert.equal(moveReceipt.status(), 200)
      assert.equal((await moveReceipt.json()).ok, true)
      await review.waitFor({ state: 'hidden' })
      const movedState = await snapshot()
      const [moved] = movedState.allocations
      assert.ok(moved)
      assert.equal(Date.parse(moved.start), instant('15:00'))
      assert.equal(Date.parse(moved.end), instant('15:30'))
      assert.deepEqual(movedState, { ...after, revision: movePreview.revision + 1,
        allocations: [{ ...allocation, start: moved.start, end: moved.end, updated_at: moved.updated_at }] })
      assert.deepEqual(commands, ['proposal.create', 'proposal.approve', 'changes.commit'])
      await row.click()
      await detail.getByTestId('time-detail-move').waitFor()
      console.log('PASS official-move-committed-same-allocation')

      // The appointment starts after the current task slot, but overlaps the
      // requested destination. No commit may pass in this negative scenario.
      allowCommit = false
      commands.length = 0
      phase = 'official-move-conflict-create-fixed-appointment'
      await detail.getByRole('button', { name: '关闭', exact: true }).last().click()
      await detail.waitFor({ state: 'hidden' })
      await workbench.getByTestId(`schedule-add-appointment-${day}`).click()
      const appointment = workbench.getByRole('dialog', { name: '新建固定预约', exact: true })
      const appointmentTitle = 'Official Web acceptance fixed appointment'
      await appointment.getByLabel('标题', { exact: true }).fill(appointmentTitle)
      await appointment.getByLabel('开始', { exact: true }).fill(`${day}T15:35`)
      await appointment.getByLabel('结束', { exact: true }).fill(`${day}T16:00`)
      assert.equal(await appointment.getByRole('checkbox', { name: '锁定，不被自动移动' }).isChecked(), true)
      await appointment.getByRole('button', { name: '保存', exact: true }).click()
      await appointment.waitFor({ state: 'hidden' })
      const conflictBaseline = await snapshot()
      const [fixed] = conflictBaseline.otherFacts.commitments
      assert.ok(fixed && typeof fixed.id === 'string' && fixed.id.length > 0)
      assert.equal(fixed.title, appointmentTitle)
      assert.equal(fixed.locked, true)
      assert.equal(Date.parse(fixed.start), instant('15:35'))
      assert.equal(Date.parse(fixed.end), instant('16:00'))
      assert.deepEqual(conflictBaseline, { ...movedState, revision: movedState.revision + 1,
        otherFacts: { ...movedState.otherFacts, commitments: [fixed] } })
      assert.deepEqual(commands, ['commitment.create'])
      const fixedRow = workbench.getByTestId('time-commitment-' + fixed.id)
      await fixedRow.getByText(appointmentTitle, { exact: true }).waitFor()

      phase = 'official-move-fixed-conflict-rejected'
      commands.length = 0
      await row.click()
      await detail.getByTestId('time-detail-move').click()
      assert.equal(await move.getByTestId('move-slot-capacity').inputValue(), capacity.id)
      await move.getByTestId('move-slot-start').fill(`${day}T15:30`)
      await move.getByText(`新结束（保持原时长）：${day} 16:00`, { exact: true }).waitFor()
      await move.getByTestId('move-slot-preview').click()
      await move.getByRole('alert').filter({ hasText: /所选时段与已有安排或忙碌区间冲突；改选时段，不会移动已有安排/ }).waitFor()
      assert.equal(await review.count(), 0)
      assert.equal(await move.getByTestId('move-slot-start').inputValue(), `${day}T15:30`)
      await move.getByRole('button', { name: '取消', exact: true }).click()
      await move.waitFor({ state: 'hidden' })
      assert.deepEqual(commands, ['proposal.create'])
      const conflictResult = await snapshot()
      assert.deepEqual(conflictResult, { ...conflictBaseline, revision: conflictResult.revision })
      await row.waitFor()
      await fixedRow.waitFor()
      console.log('PASS official-move-fixed-conflict-no-fact-change')

      // A bigger second task cannot fit in the first day's remaining capacity.
      // Only visible controls create this disposable week fixture.
      phase = 'official-week-plan-fixture'
      await showTasks()
      await workbench.getByRole('button', { name: '新建任务', exact: true }).click()
      await inWorkbench.getByLabel('标题', { exact: true }).fill('Official Web week task')
      await inWorkbench.getByLabel('预计分钟', { exact: true }).fill('125')
      await inWorkbench.getByTestId('task-save').click()
      await inWorkbench.waitFor({ state: 'hidden' })
      await workbench.getByTestId('nav-schedule').click()
      await heading('日程').waitFor()
      const nextCapacityDate = new Date(Date.parse(day + 'T12:00:00Z') + 86400000)
      while ([0, 6].includes(nextCapacityDate.getUTCDay())) {
        nextCapacityDate.setTime(nextCapacityDate.getTime() + 86400000)
      }
      const nextDay = nextCapacityDate.toISOString().slice(0, 10)
      await openScheduleOptions()
      await workbench.getByRole('button', { name: '增加容量', exact: true }).click()
      await capacityEditor.getByLabel('名称', { exact: true }).fill('Official Web week capacity')
      await capacityEditor.getByLabel('开始', { exact: true }).fill(nextDay + 'T14:00')
      await capacityEditor.getByLabel('结束', { exact: true }).fill(nextDay + 'T16:00')
      await capacityEditor.getByLabel('可用分钟', { exact: true }).fill('120')
      await capacityEditor.getByRole('button', { name: '保存', exact: true }).click()
      await capacityEditor.waitFor({ state: 'hidden' })
      const week = await snapshot()
      const weekTask = week.tasks.find(item => item.title === 'Official Web week task')
      const weekCapacity = week.capacity.find(item => item.title === 'Official Web week capacity')
      assert.equal(weekTask?.estimate_minutes, 125)
      assert.equal(weekCapacity?.capacity_minutes, 120)
      assert.equal(Date.parse(weekCapacity.start), Date.parse(nextDay + 'T14:00:00+08:00'))
      assert.equal(Date.parse(weekCapacity.end), Date.parse(nextDay + 'T16:00:00+08:00'))
      assert.deepEqual(week, { ...conflictResult, revision: conflictResult.revision + 2,
        tasks: week.tasks, capacity: week.capacity })
      assert.equal(week.tasks.length, 2)
      assert.deepEqual(week.tasks.find(item => item.id === task.id), conflictResult.tasks[0])
      assert.equal(week.capacity.length, 2)
      assert.deepEqual(week.capacity.find(item => item.id === capacity.id), conflictResult.capacity[0])

      const plan = workbench.getByRole('dialog', { name: '检查计划建议', exact: true })
      const planCheck = async (button, intent, summary, startingFacts) => {
        commands.length = 0
        // Any write before explicit approval fails closed.
        allowCommit = false
        await workbench.getByRole('button', { name: button, exact: true }).click()
        assert.match(await plan.getByTestId('plan-intent').innerText(), intent)
        assert.match(await plan.getByTestId('plan-change-summary').innerText(), summary)
        assert.deepEqual(await snapshot(), startingFacts)
        assert.deepEqual(commands, ['plan.propose'])
        const applied = page.waitForResponse(response => operation(response.request()) === 'changes.commit', { timeout: 6000 })
        allowCommit = true
        await plan.getByTestId('plan-apply').click()
        const response = await applied
        assert.equal(response.status(), 200)
        assert.equal((await response.json()).ok, true)
        await plan.waitFor({ state: 'hidden' })
        assert.deepEqual(commands, ['plan.propose', 'proposal.approve', 'changes.commit'])
        const committed = await snapshot()
        assert.deepEqual(committed, { ...startingFacts, revision: startingFacts.revision + 1,
          allocations: committed.allocations })
        const ordered = committed.allocations.slice().sort((a, b) => Date.parse(a.start) - Date.parse(b.start))
        const windows = new Set()
        for (const [i, item] of ordered.entries()) {
          const block = week.capacity.find(entry => entry.id === item.capacity_block_id)
          assert.ok(block)
          const start = Date.parse(item.start)
          const end = Date.parse(item.end)
          assert.ok(start >= Date.parse(block.start) && end <= Date.parse(block.end))
          assert.equal(end - start, item.minutes * 60000)
          assert.ok(end <= Date.parse(fixed.start) || start >= Date.parse(fixed.end))
          if (i) assert.ok(start >= Date.parse(ordered[i - 1].end))
          windows.add(block.id)
        }
        assert.deepEqual(windows, new Set([capacity.id, weekCapacity.id]))
        for (const target of [task, weekTask]) {
          assert.equal(ordered.filter(item => item.task_id === target.id)
            .reduce((minutes, item) => minutes + item.minutes, 0), target.estimate_minutes)
        }
        return committed
      }

      phase = 'official-week-additive'
      const appended = await planCheck('生成计划', /仅补充安排/, /拟新增 [1-9]\d* 项、修改 0 项、移除 0 项/, week)
      assert.deepEqual(appended.allocations.find(item => item.id === moved.id), moved)
      console.log('PASS official-week-additive-preserves-existing-allocation')

      phase = 'official-week-explicit-replan'
      const replanned = await planCheck('重新规划', /允许重排/, /拟新增 [1-9]\d* 项、修改 0 项、移除 [1-9]\d* 项/, appended)
      assert.equal(replanned.allocations.some(item => item.id === moved.id), false)
      await workbench.getByTestId('schedule-date').fill(day)
      await fixedRow.waitFor()
      await workbench.getByTestId('time-allocation-' + replanned.allocations.find(item =>
        item.capacity_block_id === capacity.id).id).waitFor()
      console.log('PASS official-week-explicit-replan-preserves-fixed-appointment')
    } finally {
      releaseCommit(false)
      await page.unroute(rpc, holdCommit)
      page.off('request', observe)
    }

    // A real responsive host drawer, not the fixture's standalone workbench.
    phase = 'official-narrow-viewport'
    const narrowBaseline = await snapshot()
    commands.length = 0
    page.on('request', observe)
    try {
      await page.setViewportSize({ width: 390, height: 844 })
      const drawer = page.locator('[data-sidebar-drawer]')
      const drawerToggle = drawer.locator('button[data-sidebar-toggle]')
      const opener = page.locator('button[data-sidebar-opener]')
      const collapsed = page.locator('[data-sidebar-drawer][data-sidebar-collapsed]')
      await collapsed.waitFor()
      phase = 'official-narrow-host-drawer'
      await opener.click()
      const zodoEntry = drawer.getByRole('button', { name: 'Zodo', exact: true })
      await zodoEntry.waitFor()
      await drawerToggle.click()
      await collapsed.waitFor()
      await opener.click()
      await zodoEntry.click()
      await collapsed.waitFor()
      await ensureHostInteractive()
      await workbench.getByTestId('nav-schedule').click()
      await heading('日程').waitFor()
      console.log('PASS official-narrow-drawer-zodo-entry')

      phase = 'official-narrow-day-navigation'
      const testId = id => workbench.getByTestId(id)
      const waitDay = async expected => {
        const shown = await page.waitForFunction(value => document.querySelector(
          '[data-zodo-workbench="a2"] [data-testid="schedule-range"]')?.textContent === value,
        expected, { timeout: 4500 })
        await shown.dispose()
      }
      const adjacent = offset => new Date(Date.parse(day + 'T12:00:00Z') + offset * 86400000).toISOString().slice(0, 10)
      await testId('schedule-mode').selectOption('1')
      await testId('schedule-date').fill(day)
      await waitDay(day)
      assert.equal(await workbench.locator('.zw-day').count(), 1)
      await testId('schedule-previous').click()
      await waitDay(adjacent(-1))
      await testId('schedule-next').click()
      await waitDay(day)
      await testId('schedule-next').click()
      await waitDay(adjacent(1))
      const today = await page.evaluate(() => new Date().toLocaleDateString('en-CA'))
      await testId('schedule-today').click()
      await waitDay(today)
      await testId('schedule-date').fill(day)
      await waitDay(day)
      const visibleAllocation = narrowBaseline.allocations.find(item => item.capacity_block_id === capacity.id)
      assert.ok(visibleAllocation)
      await testId('time-allocation-' + visibleAllocation.id).waitFor()
      assert.equal(await workbench.locator('.zw-day').count(), 1)
      assert(await workbench.locator('.zw-single').evaluate(grid =>
        grid.clientWidth > 0 && grid.scrollWidth <= grid.clientWidth + 1))
      console.log('PASS official-narrow-single-day-navigation')

      phase = 'official-narrow-keyboard-cancel'
      const trigger = await testId(`schedule-slot-${day}`).elementHandle()
      try {
        await trigger.focus()
        assert(await trigger.evaluate(node => document.activeElement === node))
        await page.keyboard.press('Enter')
        const slot = workbench.getByRole('dialog', { name: '安排任务时段', exact: true })
        const modal = await slot.elementHandle()
        try {
          const active = await page.waitForFunction(node => node.contains(document.activeElement), modal, { timeout: 3000 })
          await active.dispose()
        } finally {
          await modal.dispose()
        }
        await page.keyboard.press('Escape')
        await slot.waitFor({ state: 'hidden' })
        const restored = await page.waitForFunction(node => document.activeElement === node, trigger, { timeout: 3000 })
        await restored.dispose()
      } finally {
        await trigger.dispose()
      }
      phase = 'official-narrow-unchanged-facts'
      assert.deepEqual(commands, [])
      assert.deepEqual(await snapshot(), narrowBaseline)
      console.log('PASS official-narrow-escape-focus-and-no-write')
    } finally {
      page.off('request', observe)
    }

    // Exercise the shipped manual importer, without configuring or contacting a
    // calendar provider. The agenda must not expose vendor IDs or event titles.
    phase = 'official-busy-missing-source'
    await page.setViewportSize({ width: 1440, height: 960 })
    await ensureHostInteractive()
    await workbench.getByTestId('schedule-mode').selectOption('7')
    await workbench.getByTestId('schedule-date').fill(day)
    const noSource = workbench.getByText(/没有外部日历来源，无法判断日历覆盖情况/)
    const busyRows = workbench.getByTestId('time-busy')
    await noSource.waitFor()
    assert.equal(await busyRows.count(), 0)
    const absent = await snapshot()
    assert.deepEqual(absent, narrowBaseline)
    assert.deepEqual(absent.otherFacts.calendar_sources, [])
    assert.deepEqual(absent.otherFacts.calendar_events, [])

    const sourceInput = {
      provider: 'manual', display_name: 'Official Web busy fixture', external_id: 'synthetic-hidden-source-id',
    }
    const tomorrow = new Date(Date.parse(day + 'T12:00:00Z') + 86400000).toISOString().slice(0, 10)
    const event = {
      external_id: 'synthetic-hidden-event-id', title: 'SYNTHETIC HIDDEN CALENDAR TITLE',
      start: day + 'T23:30:00+08:00', end: tomorrow + 'T00:30:00+08:00',
    }
    const importBusy = async (mode, events) => {
      const stage = 'official-busy-' + mode
      phase = stage + '-open-import-control'
      await openScheduleOptions()
      await workbench.getByRole('button', { name: '导入忙碌', exact: true }).click()
      const editor = workbench.getByRole('dialog', { name: '导入忙碌快照', exact: true })
      phase = stage + '-editor-visible'
      await editor.waitFor()
      phase = stage + '-fill-json'
      // A wrapped textarea's DOM text can include its initial JSON, so do
      // not use the entire wrapping label's computed text as an exact name.
      const field = editor.locator('label').filter({ has: page.getByText('JSON 快照', { exact: true }) })
      await field.locator('textarea').fill(JSON.stringify({ ...sourceInput, events }))
      phase = stage + '-submit'
      await editor.getByRole('button', { name: '导入', exact: true }).click()
      phase = stage + '-wait-for-close'
      try {
        await editor.waitFor({ state: 'hidden' })
      } catch (error) {
        if (await editor.getByRole('alert').isVisible().catch(() => false)) {
          phase = stage + '-editor-reported-error'
        }
        throw error
      }
    }
    commands.length = 0
    page.on('request', observe)
    try {
      await importBusy('cross-midnight', [event])
      phase = 'official-busy-cross-midnight-confirm-core'
      const imported = await snapshot()
      const [source] = imported.otherFacts.calendar_sources
      const [stored] = imported.otherFacts.calendar_events
      assert.ok(source?.id && stored?.id)
      assert.equal(source.provider, 'manual')
      assert.equal(source.writable, false)
      assert.equal(source.external_id, sourceInput.external_id)
      assert.equal(stored.source_id, source.id)
      assert.equal(stored.external_id, event.external_id)
      assert.equal(stored.title, event.title)
      assert.equal(stored.busy, true)
      assert.equal(Date.parse(stored.start), Date.parse(event.start))
      assert.equal(Date.parse(stored.end), Date.parse(event.end))
      assert.deepEqual(imported, { ...absent, revision: absent.revision + 1,
        otherFacts: { ...absent.otherFacts, calendar_sources: [source], calendar_events: [stored] } })
      const sourceRow = workbench.getByTestId('calendar-source-' + source.id)
      phase = 'official-busy-cross-midnight-fresh-source'
      await sourceRow.getByText(/1 个忙碌事件/).waitFor()
      await sourceRow.getByText('新鲜', { exact: true }).waitFor()

      phase = 'official-busy-cross-midnight-redacted-agenda'
      for (const [date, span, continuation] of [
        [day, '23:30–24:00', '延续至次日'], [tomorrow, '00:00–00:30', '从前日延续'],
      ]) {
        const row = workbench.getByTestId('schedule-day-' + date).getByTestId('time-busy')
        await row.getByText(span, { exact: true }).waitFor()
        await row.getByText(continuation, { exact: true }).waitFor()
        await row.getByText('外部日历忙碌', { exact: true }).waitFor()
        await row.getByText('只读忙碌快照', { exact: true }).waitFor()
        assert.equal(await row.getAttribute('role'), null)
        assert.equal(await row.getAttribute('tabindex'), null)
      }
      assert.equal(await busyRows.count(), 2)
      for (const privateValue of [sourceInput.external_id, event.external_id, event.title]) {
        assert.equal((await workbench.innerText()).includes(privateValue), false)
      }
      console.log('PASS official-busy-cross-midnight-redacted')

      await importBusy('empty', [])
      phase = 'official-busy-empty-source-not-missing'
      await sourceRow.getByText(/0 个忙碌事件/).waitFor()
      await sourceRow.getByText('新鲜', { exact: true }).waitFor()
      assert.equal(await noSource.count(), 0)
      assert.equal(await busyRows.count(), 0)
      const empty = await snapshot()
      const [refreshed] = empty.otherFacts.calendar_sources
      assert.ok(Date.parse(refreshed.last_sync_at) >= Date.parse(source.last_sync_at))
      assert.deepEqual(empty, { ...imported, revision: imported.revision + 1,
        otherFacts: { ...imported.otherFacts, calendar_sources: [{ ...source, last_sync_at: refreshed.last_sync_at }],
          calendar_events: [] } })
      assert.deepEqual(commands, ['calendar.import_busy', 'calendar.import_busy'])
      console.log('PASS official-busy-empty-distinct-from-missing-source')

      // Freeze only browser Date in this disposable page, after the real
      // imports. Leave browser timers running and never change the Core clock.
      const expiresAt = Date.parse(refreshed.last_sync_at) + refreshed.freshness_seconds * 1000
      assert.ok(Number.isFinite(expiresAt) && refreshed.freshness_seconds > 0)
      const renderAt = async now => {
        await page.clock.setFixedTime(new Date(now))
        assert.equal(await page.evaluate(() => Date.now()), now)
        await workbench.getByTestId('nav-today').click()
        await heading('今天').waitFor()
        await workbench.getByTestId('nav-schedule').click()
        await heading('日程').waitFor()
        await sourceRow.getByText(/0 个忙碌事件/).waitFor()
      }
      commands.length = 0
      phase = 'official-busy-ttl-exact-boundary'
      await renderAt(expiresAt)
      await sourceRow.getByText('新鲜', { exact: true }).waitFor()
      assert.equal(await sourceRow.getByText('过期', { exact: true }).count(), 0)

      // This is the last browser interaction; later lifecycle checks use
      // the normal wall clock in the separate host/Core processes.
      phase = 'official-busy-ttl-stale-without-write'
      await renderAt(expiresAt + 1)
      await sourceRow.getByText('过期', { exact: true }).waitFor()
      assert.equal(await sourceRow.getByText('新鲜', { exact: true }).count(), 0)
      assert.equal(await noSource.count(), 0)
      assert.equal(await busyRows.count(), 0)
      assert.deepEqual(commands, [])
      assert.deepEqual(await snapshot(), empty)
      console.log('PASS official-busy-ttl-exact-then-stale-no-write')
    } finally {
      page.off('request', observe)
    }
  }
} catch (error) {
  reportFailure(phase, error)
} finally {
  if (browser) await browser.close().catch(() => {
    console.log('FAIL sandboxed-browser-cleanup')
    process.exitCode = 1
  })
  if (denyProxy?.listening) denyProxy.close()
}
