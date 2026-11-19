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
    serviceWorkers: 'block', locale: 'zh-CN', viewport: { width: 1440, height: 960 },
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
  const ensureHostInteractive = async (observePreview = false) => {
    phase = 'official-host-onboarding'
    if (observePreview) {
      // Settings/session readiness can arrive after the sidebar first renders.
      await preview.dialog.waitFor({ timeout: 5500 }).catch(error => {
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
  const heading = name => workbench.getByRole('heading', { name, exact: true })
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
    const { workspace, tasks, capacity_blocks, allocations } = body.result.data
    return { revision: workspace.revision, tasks: tasks.length,
      capacity: capacity_blocks.length, allocations: allocations.length }
  })
  phase = 'clean-core-baseline'
  const before = await snapshot()
  assert.equal(before.tasks + before.capacity + before.allocations, 0)
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
    phase = 'official-schedule-click'
    await workbench.getByTestId('nav-schedule').click()
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
} catch (error) {
  reportFailure(phase, error)
} finally {
  if (browser) await browser.close().catch(() => {
    console.log('FAIL sandboxed-browser-cleanup')
    process.exitCode = 1
  })
  if (denyProxy?.listening) denyProxy.close()
}
