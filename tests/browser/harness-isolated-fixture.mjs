/** Isolated real Core + unmodified Host bridge + unmodified React client.
 * Host registration/model services are test doubles, NOT a full DSH Host.
 * No live profiles, credentials, routes or runtime directories are read.
 */
import { createServer } from 'node:http'
import { spawn } from 'node:child_process'
import { mkdtemp, readFile, writeFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { once } from 'node:events'

const here = path.dirname(fileURLToPath(import.meta.url))
const root = path.resolve(here, '../..')
const plugin = path.join(root, 'adapters/deepseek-harness/plugin')
const pause = ms => new Promise(resolve => setTimeout(resolve, ms))
const runtime = path.join(here, 'harness-fixture-runtime/node_modules')
const html = `<!doctype html><html lang="zh-CN"><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Zodo isolated real-bridge fixture</title><style>html,body,#root{margin:0;height:100%;font-family:system-ui}*{box-sizing:border-box}.fixture-note{background:#fef3c7;font:12px system-ui;padding:6px 12px;height:28px}#root{height:calc(100% - 28px)}</style><div class="fixture-note">隔离 Core · 真实 bridge / React client · 非完整 DSH Host（不覆盖登录/Host路由）</div><div id="root"></div><script src="/react.js"></script><script src="/react-dom.js"></script><script>window.__ModuleLoader__={load(module){const client=module.factory(name=>{if(name!=="react")throw Error(name);return React});client.apply({effect(fn){fn()},sidebarRightTabs:{register(){return ()=>{}}},slots:{inject(name,fn){return fn()},register(options,Component){if(options.name==="main")ReactDOM.createRoot(document.getElementById("root")).render(React.createElement(Component));return ()=>{}}}})}}</script><script src="/client.js"></script></html>`

export async function startFixture({ binary = path.join(root, 'target/release/zodo.exe') } = {}) {
  binary = path.resolve(binary)
  // Start our own child first: the real bridge only discovers this brand-new runtime.
  const dir = await mkdtemp(path.join(tmpdir(), 'zodo-harness-fixture-'))
  const envBefore = Object.fromEntries(['ZODO_DATA_DIR', 'ZODO_CORE_URL', 'ZODO_CORE_TOKEN', 'ZODO_CORE_BIN'].map(key => [key, process.env[key]]))
  process.env.ZODO_DATA_DIR = dir
  // Discovery is real, but a crashed fixture Core must never auto-respawn detached.
  process.env.ZODO_CORE_BIN = path.join(dir, 'fixture-autostart-disabled.exe')
  delete process.env.ZODO_CORE_URL
  delete process.env.ZODO_CORE_TOKEN
  await writeFile(path.join(dir, 'model.json'), JSON.stringify({
    version: 1, local: { id: 'fixture-local', label: 'Fixture disabled network route',
      base_url: 'http://127.0.0.1:9/v1', model: 'fixture-initial-model', api_key_env: 'ZODO_FIXTURE_NO_KEY',
      location: 'device', enabled: true }, cloud: null,
  }))
  const childEnv = Object.fromEntries(['SystemRoot', 'WINDIR', 'PATH'].flatMap(key =>
    process.env[key] ? [[key, process.env[key]]] : []))
  Object.assign(childEnv, { HOME: dir, USERPROFILE: dir, LOCALAPPDATA: dir, APPDATA: dir, TEMP: dir, TMP: dir })
  const core = spawn(binary, ['--data-dir', dir, 'serve', '--port', '0'], {
    cwd: dir, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'], env: childEnv,
  })
  let coreFailure
  core.on('error', error => { coreFailure = error })
  // Consume logs without publishing any credentials or runtime.json contents.
  core.stdout.resume(); core.stderr.resume()
  const routes = [], effects = [], tools = new Map()
  let origin, server, stopped = false
  const stop = async () => {
    if (stopped) return
    stopped = true
    for (const dispose of effects.reverse()) { try { dispose?.() } catch {} }
    if (server) { server.closeAllConnections(); await new Promise(resolve => server.close(resolve)) }
    if (core.exitCode === null && core.signalCode === null && !coreFailure) {
      const exited = once(core, 'exit')
      core.kill()
      await Promise.race([exited, pause(3000)])
      if (core.exitCode === null && core.signalCode === null) { core.kill('SIGKILL'); await exited }
    }
    for (const [key, value] of Object.entries(envBefore)) {
      if (value === undefined) delete process.env[key]; else process.env[key] = value
    }
    await rm(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 150 })
  }
  try {
    let ready = false
    for (let attempt = 0; attempt < 250; attempt++) {
      if (coreFailure) throw coreFailure
      if (core.exitCode !== null) throw new Error('Fixture Core exited before ready')
      try {
        const info = JSON.parse(await readFile(path.join(dir, 'runtime.json'), 'utf8'))
        const health = await fetch('http://' + info.host + ':' + info.port + '/v1/health')
        if (health.ok && (await health.json()).instance_id === info.instance_id) { ready = true; break }
      } catch {}
      await pause(100)
    }
    if (!ready) throw new Error('Fresh fixture Core readiness timeout')
    const assets = new Map([
      ['/react.js', await readFile(path.join(runtime, 'react/umd/react.production.min.js'))],
      ['/react-dom.js', await readFile(path.join(runtime, 'react-dom/umd/react-dom.production.min.js'))],
    ])
    const ctx = {
      logger: { info() {} },
      tools: { register(def) { tools.set(def.name, def); return () => tools.delete(def.name) } },
      webServer: {
        protocol: 'http:', getOrigin: () => origin,
        register(route) { routes.push(route); return () => routes.splice(routes.indexOf(route), 1) },
      },
      sessionProjections: { async register(def) { return { stateOf: () => def.init() } } },
      sessions: { list: () => [], get: () => undefined }, agents: { currentInitiator: () => undefined },
      on() {}, effect(fn) { effects.push(fn()) },
      llm: {
        listProviders: () => [{ id: 'fixture-local', name: 'Fixture local (no dispatch)' }],
        listConfigurableProviders: () => [{ provider: 'fixture-local', settingsNs: 'fixture-local' }],
        listModels: async () => [{ id: 'fixture-model', name: 'Fixture deterministic model' }],
        resolveModelInfo: async () => ({ baseURL: 'http://127.0.0.1:9/v1' }),
      },
      settings: { describe: async () => [{ ns: 'fixture-local', value: { baseURL: 'http://127.0.0.1:9/v1', api: 'openai-completions' } }] },
      agentDefaultModel: { currentSelection: () => ({ provider: 'fixture-local', model: 'fixture-model' }) },
    }
    await (await import(pathToFileURL(path.join(plugin, 'index.js')))).apply(ctx)
    server = createServer(async (req, res) => {
      try {
        const pathname = new URL(req.url, 'http://fixture.invalid').pathname
        if (pathname === '/__fixture/shutdown' && req.method === 'POST' && (!req.headers.origin || req.headers.origin === origin)) {
          res.end('stopping owned fixture'); setImmediate(() => { void stop() }); return
        }
        const route = routes.find(row => row.kind === 'exact' ? row.path === pathname : pathname.startsWith(row.path))
        if (route) { await route.handler(req, res); return }
        if (pathname === '/') { res.setHeader('content-type', 'text/html; charset=utf-8'); res.end(html); return }
        if (pathname === '/client.js') { res.setHeader('content-type', 'text/javascript; charset=utf-8'); res.end(await readFile(path.join(plugin, 'client.js'))); return }
        if (assets.has(pathname)) { res.setHeader('content-type', 'text/javascript'); res.end(assets.get(pathname)); return }
        res.statusCode = 404; res.end('not found')
      } catch { res.statusCode = 500; res.end('Fixture request failed (detail withheld)') }
    })
    server.listen(0, '127.0.0.1')
    await once(server, 'listening')
    origin = 'http://127.0.0.1:' + server.address().port
    return { url: origin, stop, tools, dataDirectory: dir, corePid: core.pid }
  } catch (error) { await stop(); throw error }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (!process.argv.includes('--serve')) throw new Error('Use --serve for manual inspection; use harness-isolated-regression.mjs for automatic cleanup')
  const fixture = await startFixture({ binary: process.env.ZODO_FIXTURE_CORE_BIN })
  console.log(JSON.stringify({ fixtureUrl: fixture.url, isolated: true, fullHost: false }))
  let quitting = false
  const quit = async () => { if (quitting) return; quitting = true; await fixture.stop(); process.exit(0) }
  process.once('SIGINT', quit); process.once('SIGTERM', quit)
  // Hard lifetime bounds accidental abandonment of manual test servers.
  setTimeout(quit, 30 * 60_000).unref()
}
