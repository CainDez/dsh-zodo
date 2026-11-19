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
  const context = await browser.newContext({ serviceWorkers: 'block', locale: 'zh-CN' })
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

  phase = 'official-sidebar-zodo'
  await page.getByRole('button', { name: 'Zodo', exact: true }).click({ timeout: 12000 })
  console.log('PASS official-sidebar-zodo')
  phase = 'official-today-core-connected'
  await page.getByRole('heading', { name: '今天', exact: true }).waitFor()
  await page.getByText('Core 已连接', { exact: true }).waitFor()

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
  for (const [view, heading] of [['today', '今天'], ['schedule', '日程']]) {
    phase = view + '-navigate'
    await page.getByTestId('nav-' + view).click()
    await page.getByRole('heading', { name: heading, exact: true }).waitFor()
    phase = view + '-open-task'
    const trigger = await page.getByTestId('first-schedule-guide')
      .getByTestId('first-schedule-action').elementHandle()
    try {
      await trigger.focus()
      await page.keyboard.press('Enter')
      const dialog = page.getByRole('dialog', { name: '新建任务' })
      await dialog.waitFor()
      phase = view + '-escape-focus'
      await page.keyboard.press('Escape')
      await dialog.waitFor({ state: 'hidden' })
      const focused = await page.waitForFunction(button => document.activeElement === button,
        trigger, { timeout: 3000 })
      await focused.dispose()
    } finally {
      await trigger.dispose()
    }
    console.log('PASS ' + view + '-keyboard-dialog')
  }
  phase = 'unchanged-core-snapshot'
  assert.deepEqual(await snapshot(), before)
  console.log('PASS unchanged-core-snapshot')
} catch (error) {
  // No URL, browser exception, frame dump, screenshot, or session data in output.
  console.log('FAIL ' + phase + (phase === 'sandbox-launch' ? ' ' + launchReason(error) : ''))
  process.exitCode = 1
} finally {
  if (browser) await browser.close().catch(() => {
    console.log('FAIL sandboxed-browser-cleanup')
    process.exitCode = 1
  })
  if (denyProxy?.listening) denyProxy.close()
}
