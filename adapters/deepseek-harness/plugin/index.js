/**
 * Zodo Harness Host adapter.
 *
 * This is the only place that holds Core credentials. The browser gets a separate same-origin UI
 * cookie and calls the Host BFF; the agent gets native tools whose results are deliberately trimmed.
 * Zodo's authoritative facts, authorization, proposals and receipts remain in the independent Rust
 * runtime. No domain rule is implemented here.
 */
import { spawn } from 'node:child_process'
import { execFileSync } from 'node:child_process'
import { randomBytes } from 'node:crypto'
import { createHostGateway, readModelCatalog } from './host-gateway.js'
import { closeSync, existsSync, openSync } from 'node:fs'
import { mkdir, readFile } from 'node:fs/promises'
import { homedir } from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { assertHostExportAllowed, sessionHasRestrictedExport, privacyProjection, PRIVACY_GUARD_VERSION, privacyError } from './privacy.js'

export const name = 'zodo-harness'
export const inject = ['tools', 'webServer', 'llm', 'sessions', 'sessionProjections', 'agents']

const PACKAGE_DIR = path.dirname(fileURLToPath(import.meta.url))
const CORE_PROTOCOL = 'zodo/1'
const BRIDGE_VERSION = 'zodo.harness-bridge/0.2'
const PACKAGE_META = JSON.parse(await readFile(path.join(PACKAGE_DIR, 'package.json'), 'utf8'))
const ADAPTER_VERSION = PACKAGE_META.version
const HOST_REQUIREMENT = PACKAGE_META.engines.dsh
const COOKIE = 'zodo_ui'
const SESSION_TTL_MS = 60 * 60 * 1000
const MAX_BODY_BYTES = 2 * 1024 * 1024

function dataDirectory() {
  if (process.env.ZODO_DATA_DIR) return path.resolve(process.env.ZODO_DATA_DIR)
  const base = process.env.LOCALAPPDATA ?? path.join(homedir(), 'AppData', 'Local')
  return path.join(base, 'zodo')
}

function coreBinary() {
  if (process.env.ZODO_CORE_BIN) return process.env.ZODO_CORE_BIN
  const arch = process.arch === 'x64' ? 'x64' : process.arch
  const platform = process.platform
  const suffix = platform === 'win32' ? '-core.exe' : '-core'
  return path.join(PACKAGE_DIR, 'bin', `${platform}-${arch}`, `zodo${suffix}`)
}

function delay(ms) {
  return new Promise(resolve => setTimeout(resolve, ms))
}

async function readJson(pathname) {
  try {
    return JSON.parse(await readFile(pathname, 'utf8'))
  } catch {
    return undefined
  }
}

async function fetchJson(url, options = {}, timeoutMs = 15_000) {
  const timeout = AbortSignal.timeout(timeoutMs)
  const signal = options.signal ? AbortSignal.any([options.signal, timeout]) : timeout
  const response = await fetch(url, { ...options, signal })
  const text = await response.text()
  let body
  try {
    body = text ? JSON.parse(text) : null
  } catch {
    throw privacyError('CORE_INVALID_RESPONSE', 'Zodo Core returned an invalid JSON response; body withheld.')
  }
  return { response, body }
}



function createCoreManager({ log }) {
  const stateDir = dataDirectory()
  const infoPath = path.join(stateDir, 'runtime.json')
  const remoteUrl = process.env.ZODO_CORE_URL ? process.env.ZODO_CORE_URL.trim() : ''
  const remoteToken = process.env.ZODO_CORE_TOKEN ? process.env.ZODO_CORE_TOKEN.trim() : ''
  let cached
  let starting
  let logFd

  const health = async info => {
    const { response, body } = await fetchJson(`http://${info.host}:${info.port}/v1/health`, {}, 2_000)
    if (!response.ok || body?.protocol !== CORE_PROTOCOL) {
      throw new Error('runtime health/protocol mismatch')
    }
    if (body.instance_id !== info.instance_id) {
      throw new Error('runtime instance changed under runtime.json')
    }
    return body
  }

  const waitForReady = async (timeoutMs = 25_000) => {
    const start = Date.now()
    let lastError
    while (Date.now() - start < timeoutMs) {
      const info = await readJson(infoPath)
      if (info && info.protocol === CORE_PROTOCOL) {
        try {
          const facts = await health(info)
          return { info, facts }
        } catch (error) {
          lastError = error
        }
      }
      await delay(150)
    }
    throw new Error(`Zodo Core did not become ready: ${lastError?.message ?? 'no runtime.json'}`)
  }

  const start = async () => {
    await mkdir(stateDir, { recursive: true })
    const binary = coreBinary()
    if (!existsSync(binary)) {
      const error = new Error(
        `Zodo Core binary not found at ${binary}. Build zodo-runtime release and copy it into the plugin, or set ZODO_CORE_BIN.`,
      )
      error.code = 'CORE_BINARY_MISSING'
      throw error
    }
    // Refuse an incompatible artifact before it can bind a port or touch a data directory.
    try {
      const version = execFileSync(binary, ['--version'], { encoding: 'utf8', timeout: 5_000 }).trim()
      if (!/^zodo \d+\.\d+\.\d+/.test(version)) {
        const error = new Error(
          `Incompatible Zodo Core artifact at ${binary}; expected a zodo runtime version, got: ${version.slice(0, 200)}`,
        )
        error.code = 'CORE_INCOMPATIBLE'
        throw error
      }
    } catch (failure) {
      if (failure?.code === 'CORE_INCOMPATIBLE') throw failure
      const detail = `${failure?.stdout ?? ''}${failure?.stderr ?? ''}${failure?.message ?? ''}`.trim().slice(0, 300)
      const error = new Error(
        `Incompatible Zodo Core artifact at ${binary}; version preflight failed: ${detail || 'unknown error'}`,
      )
      error.code = 'CORE_INCOMPATIBLE'
      throw error
    }
    logFd = openSync(path.join(stateDir, 'core-host.log'), 'a')
    const child = spawn(binary, ['--data-dir', stateDir, 'serve', '--port', '0'], {
      cwd: stateDir,
      detached: true,
      windowsHide: true,
      stdio: ['ignore', logFd, logFd],
    })
    child.unref()
    child.once('error', error => log(`Core spawn failed: ${error.message}`))
    log(`started Core pid=${child.pid ?? 'unknown'} data=${stateDir}`)
    return waitForReady()
  }

  const ensure = async () => {
    if (cached) {
      try {
        await health(cached.info)
        return cached
      } catch {
        cached = undefined
      }
    }
    if (starting) return starting
    starting = (async () => {
      if (remoteUrl) {
        if (!remoteToken) {
          const error = new Error('ZODO_CORE_URL is set but ZODO_CORE_TOKEN is missing')
          error.code = 'CORE_REMOTE_TOKEN_MISSING'
          throw error
        }
        const parsed = new URL(remoteUrl)
        const info = {
          protocol: CORE_PROTOCOL,
          runtime_version: 'remote',
          instance_id: 'pending',
          pid: 0,
          host: parsed.hostname,
          port: Number(parsed.port || (parsed.protocol === 'https:' ? 443 : 80)),
          data_dir: '(remote)',
          workspace_id: 'ws_local',
          started_at: new Date().toISOString(),
          tokens: { ui: remoteToken, agent: remoteToken },
        }
        const healthUrl = `http://${info.host}:${info.port}/v1/health`
        const probe = await fetchJson(healthUrl, {}, 5_000)
        if (!probe.response.ok || probe.body?.protocol !== CORE_PROTOCOL) {
          throw new Error('remote Zodo Core health/protocol mismatch')
        }
        const facts = probe.body
        info.instance_id = facts.instance_id
        info.workspace_id = facts.workspace_id
        info.runtime_version = facts.runtime_version ?? 'remote'
        cached = { info, facts }
        return cached
      }
      const existing = await readJson(infoPath)
      if (existing && existing.protocol === CORE_PROTOCOL) {
        try {
          const facts = await health(existing)
          cached = { info: existing, facts }
          return cached
        } catch {
          // The runtime may be restarting; spawn and let the single-writer lock arbitrate.
        }
      }
      const started = await start()
      cached = started
      return started
    })()
    try {
      return await starting
    } finally {
      starting = undefined
      if (logFd !== undefined) {
        try {
          closeSync(logFd)
        } catch {
          // Core inherited the handle.
        }
        logFd = undefined
      }
    }
  }

  return { ensure, stateDir, infoPath }
}

function createUiSessions() {
  const sessions = new Map()
  return {
    mint(origin) {
      const token = randomBytes(32).toString('hex')
      const now = Date.now()
      sessions.set(token, { createdAt: now, expiresAt: now + SESSION_TTL_MS, origin })
      if (sessions.size > 64) {
        for (const [key, value] of sessions) {
          if (value.expiresAt <= now) sessions.delete(key)
        }
        while (sessions.size > 64) sessions.delete(sessions.keys().next().value)
      }
      return token
    },
    verify(token, origin) {
      const found = token ? sessions.get(token) : undefined
      // Same-origin GET requests do not include an Origin header. The HttpOnly SameSite=Strict
      // cookie is the proof for this same-site UI face; POSTs still carry and must match Origin.
      if (!found || found.expiresAt <= Date.now() || (origin !== undefined && found.origin !== origin)) {
        if (token) sessions.delete(token)
        return undefined
      }
      return found
    },
    clear() {
      sessions.clear()
    },
  }
}

function json(res, status, value, extraHeaders = {}) {
  const body = Buffer.from(JSON.stringify(value), 'utf8')
  res.writeHead(status, {
    'Content-Type': 'application/json; charset=utf-8',
    'Content-Length': body.length,
    'Cache-Control': 'no-store',
    'X-Content-Type-Options': 'nosniff',
    'Referrer-Policy': 'no-referrer',
    ...extraHeaders,
  })
  res.end(body)
}

function readBody(req) {
  return new Promise((resolve, reject) => {
    let body = ''
    let size = 0
    req.on('data', chunk => {
      size += chunk.length
      if (size > MAX_BODY_BYTES) {
        reject(Object.assign(new Error('body too large'), { code: 'BODY_TOO_LARGE' }))
        req.destroy()
        return
      }
      body += chunk.toString('utf8')
    })
    req.on('end', () => resolve(body))
    req.on('error', reject)
  })
}

function parseCookies(header) {
  const map = new Map()
  for (const part of String(header ?? '').split(';')) {
    const at = part.indexOf('=')
    if (at > 0) map.set(part.slice(0, at).trim(), part.slice(at + 1).trim())
  }
  return map
}

function sameOrigin(req, origin) {
  return req.headers.origin === origin
}

// The Desktop face forwards bridge traffic through its shell, which strips request Origin,
// Cookie and response Set-Cookie (the shell owns the cookie jar). Browsers always attach an
// Origin header to POST requests, so an absent Origin marks a non-browser, shell-mediated
// caller; a present Origin must still be same-origin or allowlisted. Because Set-Cookie cannot
// reach the Desktop page, such callers receive the UI session token in the response body and
// replay it through the X-Zodo-UI-Session header.
function uiSessionFrom(req) {
  const header = req.headers['x-zodo-ui-session']
  return (typeof header === 'string' && header.length > 0) ? header : parseCookies(req.headers.cookie).get(COOKIE)
}

async function coreCall(info, token, kind, envelope, signal) {
  const url = `http://${info.host}:${info.port}${kind === 'command' ? '/v1/commands' : '/v1/queries'}`
  const { response, body } = await fetchJson(
    url,
    {
      method: 'POST',
      headers: {
        Authorization: `Bearer ${token}`,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify(envelope),
      signal,
    },
    30_000,
  )
  return { status: response.status, body }
}

async function coreHttp(info, token, method, pathname, body, signal) {
  const options = {
    method,
    headers: {
      Authorization: `Bearer ${token}`,
      Accept: 'application/json',
    },
    signal,
  }
  if (body !== undefined) {
    options.headers['Content-Type'] = 'application/json'
    options.body = JSON.stringify(body)
  }
  const { response, body: payload } = await fetchJson(
    `http://${info.host}:${info.port}${pathname}`,
    options,
    35_000,
  )
  return { status: response.status, body: payload }
}

async function coreCommand(core, operation, payload, idempotencyKey, options = {}) {
  const { info } = await core.ensure()
  const token = options.agent ? info.tokens.agent : info.tokens.ui
  if (!token) throw new Error('Core runtime does not expose the required channel token')
  const envelope = {
    schema: 'zodo.command/1',
    request_id: `tool_${randomBytes(8).toString('hex')}`,
    idempotency_key: idempotencyKey,
    workspace_id: info.workspace_id,
    operation,
    operation_version: 1,
    payload,
  }
  const result = await coreCall(info, token, 'command', envelope, options.signal)
  if (options.agent) assertHostExportAllowed(result.body)
  if (result.status < 200 || result.status >= 300) {
    const error = result.body?.error ?? result.body ?? { message: `Core HTTP ${result.status}` }
    const failure = new Error(options.agent ? 'Zodo Core command failed; inspect details in the private Zodo UI.' : (error.message ?? 'Core command failed'))
    failure.code = options.agent ? 'CORE_COMMAND_FAILED' : (error.code ?? `CORE_HTTP_${result.status}`)
    throw failure
  }
  return result.body
}

function toolResult(value) {
  return assertHostExportAllowed(value)
}

function safeTask(task) {
  return {
    id: task.id ?? null,
    title: task.title ?? null,
    status: task.status ?? null,
    priority: task.priority ?? null,
    estimate_minutes: task.estimate_minutes ?? null,
    target_due: task.target_due ?? null,
  }
}

function createTools(core) {
  return [
    {
      name: 'zodo_capture',
      description:
        'Capture raw text into the user\'s Zodo inbox. This preserves the original text; it does not create or complete a task.',
      parameters: {
        type: 'object',
        properties: {
          text: { type: 'string', description: 'The raw text to preserve.' },
        },
        required: ['text'],
        additionalProperties: false,
      },
      output: {
        schema: {
          type: 'object',
          properties: {
            ok: { type: 'boolean' },
            inbox_id: { type: 'string' },
            status: { type: 'string' },
          },
          required: ['ok', 'inbox_id', 'status'],
          additionalProperties: false,
        },
        render: (_args, value) => [{ type: 'text', text: JSON.stringify(value) }],
      },
      async execute(args, exec) {
        const result = await coreCommand(
          core,
          'capture.collect',
          { text: String(args.text ?? '') },
          `tool-capture:${exec.callId}`,
          { agent: true, signal: exec.signal },
        )
        return toolResult({
          ok: true,
          inbox_id: result.data.inbox_entry.id,
          status: result.data.inbox_entry.status,
        })
      },
    },
    {
      name: 'zodo_list_tasks',
      description:
        'List a small, safe projection of Zodo tasks. Use this to understand the user\'s current work before proposing a change.',
      parameters: {
        type: 'object',
        properties: {
          text: { type: 'string', description: 'Optional text filter.' },
          limit: { type: 'integer', description: 'Maximum rows, capped at 50.' },
        },
        additionalProperties: false,
      },
      output: {
        schema: {
          type: 'object',
          properties: {
            ok: { type: 'boolean' },
            revision: { type: 'integer' },
            tasks: { type: 'array', items: { type: 'object', additionalProperties: true } },
          },
          required: ['ok', 'revision', 'tasks'],
          additionalProperties: false,
        },
        render: (_args, value) => [{ type: 'text', text: JSON.stringify(value) }],
      },
      async execute(args, exec) {
        const { info } = await core.ensure()
        const envelope = {
          schema: 'zodo.query/1',
          request_id: `tool_${randomBytes(8).toString('hex')}`,
          workspace_id: info.workspace_id,
          query: 'tasks.list',
          payload: {
            filter: {
              text: args.text,
              limit: Math.min(Number(args.limit) || 20, 50),
              include_archived: false,
              only_leaf: true,
            },
          },
        }
        const result = await coreCall(info, info.tokens.agent, 'query', envelope, exec.signal)
        assertHostExportAllowed(result.body)
        if (result.status < 200 || result.status >= 300) {
          throw privacyError('CORE_QUERY_FAILED', 'Zodo Core query failed; inspect details in the private Zodo UI.')
        }
        return toolResult({
          ok: true,
          revision: result.body.revision,
          tasks: (result.body.data.tasks ?? []).map(row => safeTask(row.task ?? row)),
        })
      },
    },
    {
      name: 'zodo_propose',
      description:
        'Submit a structured candidate change to Zodo. The Core validates the operations, creates a proposal and returns its preview. This does not approve or commit it; the user must review and approve.',
      parameters: {
        type: 'object',
        properties: {
          intent: { type: 'string', description: 'One sentence describing the intended outcome.' },
          operations: {
            type: 'array',
            description: 'Structured Core change operations.',
            items: { type: 'object', additionalProperties: true },
          },
        },
        required: ['intent', 'operations'],
        additionalProperties: false,
      },
      output: {
        schema: {
          type: 'object',
          properties: {
            ok: { type: 'boolean' },
            proposal_id: { type: 'string' },
            status: { type: 'string' },
            summary: { type: 'string' },
          },
          required: ['ok', 'proposal_id', 'status', 'summary'],
          additionalProperties: false,
        },
        render: (_args, value) => [{ type: 'text', text: JSON.stringify(value) }],
      },
      async execute(args, exec) {
        const result = await coreCommand(
          core,
          'proposal.create',
          {
            intent: String(args.intent ?? ''),
            operations: args.operations ?? [],
          },
          `tool-propose:${exec.callId}`,
          { agent: true, signal: exec.signal },
        )
        return toolResult({
          ok: true,
          proposal_id: result.data.proposal.id,
          status: result.data.proposal.status,
          summary: `${result.data.proposal.intent} (${result.data.proposal.status})`,
        })
      },
    },
    {
      name: 'zodo_plan_propose',
      description:
        'Ask the Core to compute a deterministic schedule proposal for selected tasks. The result is a proposal, not a committed plan.',
      parameters: {
        type: 'object',
        properties: {
          task_ids: { type: 'array', items: { type: 'string' } },
          horizon_start: { type: 'string', description: 'RFC3339 instant.' },
          horizon_end: { type: 'string', description: 'RFC3339 instant.' },
        },
        required: ['task_ids', 'horizon_start', 'horizon_end'],
        additionalProperties: false,
      },
      output: {
        schema: {
          type: 'object',
          properties: {
            ok: { type: 'boolean' },
            proposal_id: { type: 'string' },
            status: { type: 'string' },
            assigned_minutes: { type: 'integer' },
            unplaced_minutes: { type: 'integer' },
          },
          required: ['ok', 'proposal_id', 'status', 'assigned_minutes', 'unplaced_minutes'],
          additionalProperties: false,
        },
        render: (_args, value) => [{ type: 'text', text: JSON.stringify(value) }],
      },
      async execute(args, exec) {
        const result = await coreCommand(
          core,
          'plan.propose',
          {
            task_ids: args.task_ids ?? [],
            horizon_start: args.horizon_start,
            horizon_end: args.horizon_end,
            move_unlocked: true,
          },
          `tool-plan:${exec.callId}`,
          { agent: true, signal: exec.signal },
        )
        const assignments = result.data.plan?.assignments ?? []
        const unplaced = result.data.plan?.unplaced ?? []
        return toolResult({
          ok: true,
          proposal_id: result.data.proposal.id,
          status: result.data.proposal.status,
          assigned_minutes: assignments.reduce((sum, row) => sum + (row.minutes ?? 0), 0),
          unplaced_minutes: unplaced.reduce((sum, row) => sum + (row.minutes ?? 0), 0),
        })
      },
    },
    {
      name: 'zodo_replan',
      description:
        'Recompute a local plan for selected tasks while keeping locked, started and past facts fixed. Returns a proposal; a human must approve and commit.',
      parameters: {
        type: 'object',
        properties: {
          task_ids: { type: 'array', items: { type: 'string' } },
          horizon_start: { type: 'string', description: 'RFC3339 instant.' },
          horizon_end: { type: 'string', description: 'RFC3339 instant.' },
        },
        required: ['task_ids', 'horizon_start', 'horizon_end'],
        additionalProperties: false,
      },
      output: {
        schema: {
          type: 'object',
          properties: {
            ok: { type: 'boolean' },
            proposal_id: { type: 'string' },
            status: { type: 'string' },
            assigned_minutes: { type: 'integer' },
            unplaced_minutes: { type: 'integer' },
          },
          required: ['ok', 'proposal_id', 'status', 'assigned_minutes', 'unplaced_minutes'],
          additionalProperties: false,
        },
        render: (_args, value) => [{ type: 'text', text: JSON.stringify(value) }],
      },
      async execute(args, exec) {
        const result = await coreCommand(
          core,
          'plan.propose',
          {
            task_ids: args.task_ids ?? [],
            horizon_start: args.horizon_start,
            horizon_end: args.horizon_end,
            move_unlocked: true,
            intent: 'local_replan',
          },
          `tool-replan:${exec.callId}`,
          { agent: true, signal: exec.signal },
        )
        const assignments = result.data.plan?.assignments ?? []
        const unplaced = result.data.plan?.unplaced ?? []
        return toolResult({
          ok: true,
          proposal_id: result.data.proposal.id,
          status: result.data.proposal.status,
          assigned_minutes: assignments.reduce((sum, row) => sum + (row.minutes ?? 0), 0),
          unplaced_minutes: unplaced.reduce((sum, row) => sum + (row.minutes ?? 0), 0),
        })
      },
    },
    {
      name: 'zodo_get_proposal',
      description: 'Read a proposal and its structured diagnostics by id.',
      parameters: {
        type: 'object',
        properties: { proposal_id: { type: 'string' } },
        required: ['proposal_id'],
        additionalProperties: false,
      },
      output: {
        schema: {
          type: 'object',
          properties: {
            ok: { type: 'boolean' },
            proposal_id: { type: 'string' },
            status: { type: 'string' },
            intent: { type: 'string' },
            diagnostics: { type: 'array', items: { type: 'object', additionalProperties: true } },
          },
          required: ['ok', 'proposal_id', 'status', 'intent', 'diagnostics'],
          additionalProperties: false,
        },
        render: (_args, value) => [{ type: 'text', text: JSON.stringify(value) }],
      },
      async execute(args, exec) {
        const { info } = await core.ensure()
        const envelope = {
          schema: 'zodo.query/1',
          request_id: `tool_${randomBytes(8).toString('hex')}`,
          workspace_id: info.workspace_id,
          query: 'proposal.get',
          payload: { id: String(args.proposal_id ?? '') },
        }
        const result = await coreCall(info, info.tokens.agent, 'query', envelope, exec.signal)
        assertHostExportAllowed(result.body)
        if (result.status < 200 || result.status >= 300) {
          throw privacyError('CORE_QUERY_FAILED', 'Zodo Core query failed; inspect details in the private Zodo UI.')
        }
        const proposal = result.body.data.proposal
        return toolResult({
          ok: true,
          proposal_id: proposal.id,
          status: proposal.status,
          intent: proposal.intent,
          diagnostics: proposal.diagnostics ?? [],
        })
      },
    },
  ]
}

// Only provider/model catalog facts cross the browser bridge. The Host owns every credential
// and protocol (including subscription/OAuth); selecting a route invokes llm, never copies settings.
async function readHarnessModelCatalog(ctx) {
  const catalog = await readModelCatalog(ctx.llm)
  return { ok: true, schema: 'zodo.harness-model-catalog/1', fetched_at: new Date().toISOString(),
    host: { adapter: ADAPTER_VERSION, host_requirement: HOST_REQUIREMENT, privacy_guard: PRIVACY_GUARD_VERSION },
    ...catalog, notes: ['所选 DSH 模型通过 Host llm 调用；凭据和订阅登录留在 Host。', '目录和配置状态仅为预检，不代表推理已成功。'] }
}

export async function apply(ctx) {
  const privacy = await ctx.sessionProjections.register(privacyProjection)
  const log = message => {
    try {
      ctx.logger.info(message)
    } catch {
      // Logging must not affect availability.
    }
  }
  const manager = createCoreManager({ log })
  const hostGateway = createHostGateway({ llm: ctx.llm })
  const core = { ...manager, async ensure() {
    const result = await manager.ensure()
    try { await hostGateway.ensureBound(result.info) } catch { log('Zodo Host model binding unavailable; no model fallback was selected.') }
    return result
  } }
  const sessions = createUiSessions()
  const disposers = []

  let generation = 0
  const install = () => {
  for (const definition of createTools(core)) {
    disposers.push(ctx.tools.register(definition))
  }

  const origin = () => ctx.webServer.getOrigin()
  const allowedOrigins = () => {
    const value = origin()
    return new Set([value, value.replace('127.0.0.1', 'localhost')])
  }

  disposers.push(ctx.webServer.register({
    kind: 'exact',
    path: '/zodo-bridge/v1/health',
    handler: async (req, res) => {
      if (req.method !== 'GET') {
        json(res, 405, { ok: false, error: { code: 'METHOD_NOT_ALLOWED', message: 'GET only' } })
        return
      }
      try {
        const { info, facts } = await core.ensure()
        json(res, 200, {
          ok: true,
          schema: BRIDGE_VERSION,
          bridge: {
            host_requirement: HOST_REQUIREMENT,
            adapter: ADAPTER_VERSION,
            privacy_guard: PRIVACY_GUARD_VERSION,
            core_owned_by_host: false,
            ui_token_exposed: false,
          },
          core: {
            protocol: facts.protocol,
            instance_id: facts.instance_id,
            revision: facts.revision,
            workspace_id: facts.workspace_id,
            data_dir: info.data_dir,
          },
        })
      } catch (error) {
        json(res, 503, {
          ok: false,
          error: { code: error.code ?? 'CORE_UNAVAILABLE', message: error.message, retryable: true },
        })
      }
    },
  }))

  disposers.push(ctx.webServer.register({
    kind: 'exact',
    path: '/zodo-bridge/v1/session',
    handler: async (req, res) => {
      if (req.method !== 'POST') {
        json(res, 405, { ok: false, error: { code: 'METHOD_NOT_ALLOWED', message: 'POST only' } })
        return
      }
      const sessionOrigin = req.headers.origin
      if (sessionOrigin && !sameOrigin(req, origin()) && !allowedOrigins().has(sessionOrigin)) {
        json(res, 403, { ok: false, error: { code: 'ORIGIN_DENIED', message: 'same-origin UI only' } })
        return
      }
      try {
        const { facts } = await core.ensure()
        const token = sessions.mint(sessionOrigin)
        const secure = ctx.webServer.protocol === 'https:' ? '; Secure' : ''
        json(res, 200, {
          ok: true,
          schema: BRIDGE_VERSION,
          ui_face: 'interactive',
          bridge_protocol: CORE_PROTOCOL,
          core: {
            protocol: facts.protocol,
            instance_id: facts.instance_id,
            revision: facts.revision,
            workspace_id: facts.workspace_id,
          },
          ...(sessionOrigin === undefined ? { session_token: token } : {}),
          expires_in: Math.floor(SESSION_TTL_MS / 1000),
        }, {
          'Set-Cookie': `${COOKIE}=${token}; HttpOnly; SameSite=Strict; Path=/zodo-bridge; Max-Age=${Math.floor(SESSION_TTL_MS / 1000)}${secure}`,
        })
      } catch (error) {
        json(res, 503, {
          ok: false,
          error: { code: error.code ?? 'CORE_UNAVAILABLE', message: error.message, retryable: true },
        })
      }
    },
  }))

  disposers.push(ctx.webServer.register({
    kind: 'exact',
    path: '/zodo-bridge/v1/rpc',
    handler: async (req, res) => {
      try {
        if (req.method !== 'POST') {
          json(res, 405, { ok: false, error: { code: 'METHOD_NOT_ALLOWED', message: 'POST only' } })
          return
        }
        const browserOrigin = req.headers.origin
        if (browserOrigin && !sameOrigin(req, origin()) && !allowedOrigins().has(browserOrigin)) {
          json(res, 403, { ok: false, error: { code: 'ORIGIN_DENIED', message: 'same-origin UI only' } })
          return
        }
        const token = uiSessionFrom(req)
        if (!sessions.verify(token, req.headers.origin)) {
          json(res, 401, { ok: false, error: { code: 'UI_SESSION_REQUIRED', message: 'mint a UI session first' } })
          return
        }
        if (!String(req.headers['content-type'] ?? '').toLowerCase().includes('application/json')) {
          json(res, 415, { ok: false, error: { code: 'CONTENT_TYPE_REQUIRED', message: 'application/json required' } })
          return
        }
        const raw = await readBody(req)
        let request
        try {
          request = JSON.parse(raw)
        } catch {
          json(res, 400, { ok: false, error: { code: 'BAD_JSON', message: 'invalid JSON' } })
          return
        }
        if (!request || !['command', 'query'].includes(request.kind) || !request.envelope) {
          json(res, 400, { ok: false, error: { code: 'BAD_REQUEST', message: 'kind and envelope are required' } })
          return
        }
        const envelope = request.envelope
        if (envelope.schema !== (request.kind === 'command' ? 'zodo.command/1' : 'zodo.query/1')) {
          json(res, 400, { ok: false, error: { code: 'BAD_SCHEMA', message: 'envelope schema does not match kind' } })
          return
        }
        const { info, facts } = await core.ensure()
        // The Host owns the workspace binding; a browser cannot redirect the query/command.
        envelope.workspace_id = info.workspace_id
        const result = await coreCall(info, info.tokens.ui, request.kind, envelope, undefined)
        const extra = {
          'X-Zodo-Core-Instance': facts.instance_id,
          'X-Zodo-Bridge': BRIDGE_VERSION,
        }
        if (result.status >= 200 && result.status < 300) {
          json(res, 200, {
            ok: true,
            bridge: BRIDGE_VERSION,
            core_instance: facts.instance_id,
            result: result.body,
          }, extra)
        } else {
          json(res, result.status, {
            ok: false,
            bridge: BRIDGE_VERSION,
            core_instance: facts.instance_id,
            error: result.body?.error ?? { code: 'CORE_ERROR', message: `Core HTTP ${result.status}` },
            request_id: result.body?.request_id,
          }, extra)
        }
      } catch (error) {
        log(`bridge error: ${error?.message ?? String(error)}`)
        json(res, error?.code === 'CORE_UNAVAILABLE' ? 503 : 502, {
          ok: false,
          error: {
            code: error?.code ?? 'BRIDGE_INTERNAL',
            message: error?.message ?? 'bridge failure',
            retryable: true,
          },
        })
      }
    },
  }))

  disposers.push(ctx.webServer.register({
    kind: 'prefix',
    path: '/zodo-bridge/v1/runs',
    handler: async (req, res) => {
      try {
        const browserOrigin = req.headers.origin
        if (browserOrigin && !sameOrigin(req, origin()) && !allowedOrigins().has(browserOrigin)) {
          json(res, 403, { ok: false, error: { code: 'ORIGIN_DENIED', message: 'same-origin UI only' } })
          return
        }
        const token = uiSessionFrom(req)
        if (!sessions.verify(token, req.headers.origin)) {
          json(res, 401, { ok: false, error: { code: 'UI_SESSION_REQUIRED', message: 'mint a UI session first' } })
          return
        }
        const { info, facts } = await core.ensure()
        const requestPath = String(req.url ?? '').split('?')[0]
        const suffix = requestPath.startsWith('/zodo-bridge/v1/runs')
          ? requestPath.slice('/zodo-bridge/v1/runs'.length) || ''
          : ''
        if (suffix === '') {
          if (req.method === 'GET') {
            const result = await coreHttp(info, info.tokens.ui, 'GET', '/v1/runs')
            json(res, result.status, { ok: result.status < 300, core_status: result.status, result: result.body }, {
              'X-Zodo-Core-Instance': facts.instance_id,
            })
            return
          }
          if (req.method !== 'POST') {
            json(res, 405, { ok: false, error: { code: 'METHOD_NOT_ALLOWED', message: 'GET or POST only' } })
            return
          }
          if (!String(req.headers['content-type'] ?? '').toLowerCase().includes('application/json')) {
            json(res, 415, { ok: false, error: { code: 'CONTENT_TYPE_REQUIRED', message: 'application/json required' } })
            return
          }
          const raw = await readBody(req)
          let body
          try {
            body = JSON.parse(raw)
          } catch {
            json(res, 400, { ok: false, error: { code: 'BAD_JSON', message: 'invalid JSON' } })
            return
          }
          const result = await coreHttp(info, info.tokens.ui, 'POST', '/v1/runs', body)
          json(res, result.status, { ok: result.status < 300, core_status: result.status, result: result.body }, {
            'X-Zodo-Core-Instance': facts.instance_id,
          })
          return
        }
        const match = suffix.match(/^\/([A-Za-z0-9_-]+)(\/cancel)?$/)
        if (!match) {
          json(res, 404, { ok: false, error: { code: 'NOT_FOUND', message: 'run route not found' } })
          return
        }
        const id = match[1]
        if (match[2]) {
          if (req.method !== 'POST') {
            json(res, 405, { ok: false, error: { code: 'METHOD_NOT_ALLOWED', message: 'POST only' } })
            return
          }
          const result = await coreHttp(info, info.tokens.ui, 'POST', `/v1/runs/${id}/cancel`, {})
          json(res, result.status, { ok: result.status < 300, core_status: result.status, result: result.body }, {
            'X-Zodo-Core-Instance': facts.instance_id,
          })
          return
        }
        if (req.method !== 'GET') {
          json(res, 405, { ok: false, error: { code: 'METHOD_NOT_ALLOWED', message: 'GET only' } })
          return
        }
        const result = await coreHttp(info, info.tokens.ui, 'GET', `/v1/runs/${id}`)
        json(res, result.status, { ok: result.status < 300, core_status: result.status, result: result.body }, {
          'X-Zodo-Core-Instance': facts.instance_id,
        })
      } catch (error) {
        log(`runs bridge error: ${error?.message ?? String(error)}`)
        json(res, 502, {
          ok: false,
          error: { code: error?.code ?? 'BRIDGE_INTERNAL', message: error?.message ?? 'runs bridge failure', retryable: true },
        })
      }
    },
  }))

  disposers.push(ctx.webServer.register({
    kind: 'prefix',
    path: '/zodo-bridge/v1/data',
    handler: async (req, res) => {
      try {
        const browserOrigin = req.headers.origin
        if (browserOrigin && !sameOrigin(req, origin()) && !allowedOrigins().has(browserOrigin)) {
          json(res, 403, { ok: false, error: { code: 'ORIGIN_DENIED', message: 'same-origin UI only' } })
          return
        }
        const token = uiSessionFrom(req)
        if (!sessions.verify(token, req.headers.origin)) {
          json(res, 401, { ok: false, error: { code: 'UI_SESSION_REQUIRED', message: 'mint a UI session first' } })
          return
        }
        const { info } = await core.ensure()
        const requestPath = String(req.url ?? '').split('?')[0]
        const suffix = requestPath.startsWith('/zodo-bridge/v1/data')
          ? requestPath.slice('/zodo-bridge/v1/data'.length) || ''
          : ''
        let result
        if (suffix === '/export' && req.method === 'GET') {
          result = await coreHttp(info, info.tokens.ui, 'GET', '/v1/data/export')
        } else if (suffix === '/export.csv' && req.method === 'GET') {
          const response = await fetch(`http://${info.host}:${info.port}/v1/data/export.csv`, {
            headers: { Authorization: `Bearer ${info.tokens.ui}` },
          })
          const text = await response.text()
          res.writeHead(response.status, {
            'Content-Type': 'text/csv; charset=utf-8',
            'Content-Length': Buffer.byteLength(text),
            'Content-Disposition': 'attachment; filename="zodo-tasks.csv"',
          })
          res.end(text)
          return
        } else if (suffix === '/import/preflight' && req.method === 'POST') {
          const raw = await readBody(req)
          result = await coreHttp(info, info.tokens.ui, 'POST', '/v1/data/import/preflight', JSON.parse(raw))
        } else if (suffix === '/import' && req.method === 'POST') {
          const raw = await readBody(req)
          result = await coreHttp(info, info.tokens.ui, 'POST', '/v1/data/import', JSON.parse(raw))
        } else if (suffix === '/backup' && req.method === 'POST') {
          result = await coreHttp(info, info.tokens.ui, 'POST', '/v1/data/backup', {})
        } else if (suffix === '/backups' && req.method === 'GET') {
          result = await coreHttp(info, info.tokens.ui, 'GET', '/v1/data/backups')
        } else {
          json(res, 404, { ok: false, error: { code: 'NOT_FOUND', message: 'data route not found' } })
          return
        }
        json(res, result.status, { ok: result.status < 300, core_status: result.status, result: result.body })
      } catch (error) {
        log(`data bridge error: ${error?.message ?? String(error)}`)
        json(res, 502, { ok: false, error: { code: error?.code ?? 'BRIDGE_INTERNAL', message: error?.message ?? 'data bridge failure' } })
      }
    },
  }))

  disposers.push(ctx.webServer.register({
    kind: 'prefix',
    path: '/zodo-bridge/v1/events',
    handler: async (req, res) => {
      try {
        if (req.method !== 'GET') {
          json(res, 405, { ok: false, error: { code: 'METHOD_NOT_ALLOWED', message: 'GET only' } })
          return
        }
        const browserOrigin = req.headers.origin
        if (browserOrigin && !sameOrigin(req, origin()) && !allowedOrigins().has(browserOrigin)) {
          json(res, 403, { ok: false, error: { code: 'ORIGIN_DENIED', message: 'same-origin UI only' } })
          return
        }
        const token = uiSessionFrom(req)
        if (!sessions.verify(token, req.headers.origin)) {
          json(res, 401, { ok: false, error: { code: 'UI_SESSION_REQUIRED', message: 'mint a UI session first' } })
          return
        }
        const { info } = await core.ensure()
        const incoming = new URL(String(req.url ?? ''), 'http://127.0.0.1')
        const after = incoming.searchParams.get('after') ?? 'ev_0'
        const limit = Math.min(500, Math.max(1, Number(incoming.searchParams.get('limit') ?? 100) || 100))
        const result = await coreHttp(
          info,
          info.tokens.ui,
          'GET',
          `/v1/events?after=${encodeURIComponent(after)}&limit=${limit}`,
        )
        json(res, result.status, { ok: result.status < 300, core_status: result.status, result: result.body })
      } catch (error) {
        log(`events bridge error: ${error?.message ?? String(error)}`)
        json(res, 502, { ok: false, error: { code: error?.code ?? 'BRIDGE_INTERNAL', message: error?.message ?? 'events bridge failure' } })
      }
    },
  }))

  disposers.push(ctx.webServer.register({
    kind: 'prefix',
    path: '/zodo-bridge/v1/sync',
    handler: async (req, res) => {
      try {
        const browserOrigin = req.headers.origin
        if (browserOrigin && !sameOrigin(req, origin()) && !allowedOrigins().has(browserOrigin)) {
          json(res, 403, { ok: false, error: { code: 'ORIGIN_DENIED', message: 'same-origin UI only' } })
          return
        }
        const token = uiSessionFrom(req)
        if (!sessions.verify(token, req.headers.origin)) {
          json(res, 401, { ok: false, error: { code: 'UI_SESSION_REQUIRED', message: 'mint a UI session first' } })
          return
        }
        const { info } = await core.ensure()
        const requestPath = String(req.url ?? '').split('?')[0]
        const suffix = requestPath.startsWith('/zodo-bridge/v1/sync')
          ? requestPath.slice('/zodo-bridge/v1/sync'.length) || ''
          : ''
        let result
        if (suffix === '/export' && req.method === 'GET') {
          result = await coreHttp(info, info.tokens.ui, 'GET', '/v1/sync/export')
        } else if (suffix === '/import' && req.method === 'POST') {
          const raw = await readBody(req)
          result = await coreHttp(info, info.tokens.ui, 'POST', '/v1/sync/import', JSON.parse(raw))
        } else {
          json(res, 404, { ok: false, error: { code: 'NOT_FOUND', message: 'sync route not found' } })
          return
        }
        json(res, result.status, { ok: result.status < 300, core_status: result.status, result: result.body })
      } catch (error) {
        log(`sync bridge error: ${error?.message ?? String(error)}`)
        json(res, 502, { ok: false, error: { code: error?.code ?? 'BRIDGE_INTERNAL', message: error?.message ?? 'sync bridge failure' } })
      }
    },
  }))

  disposers.push(ctx.webServer.register({
    kind: 'exact',
    path: '/zodo-bridge/v1/model',
    handler: async (req, res) => {
      try {
        const browserOrigin = req.headers.origin
        if (browserOrigin && !sameOrigin(req, origin()) && !allowedOrigins().has(browserOrigin)) {
          json(res, 403, { ok: false, error: { code: 'ORIGIN_DENIED', message: 'same-origin UI only' } })
          return
        }
        const token = uiSessionFrom(req)
        if (!sessions.verify(token, req.headers.origin)) {
          json(res, 401, { ok: false, error: { code: 'UI_SESSION_REQUIRED', message: 'mint a UI session first' } })
          return
        }
        const { info } = await core.ensure()
        if (req.method === 'GET') {
          const result = await coreHttp(info, info.tokens.ui, 'GET', '/v1/model')
          json(res, result.status, { ok: result.status < 300, core_status: result.status, result: result.body })
          return
        }
        if (req.method !== 'POST') {
          json(res, 405, { ok: false, error: { code: 'METHOD_NOT_ALLOWED', message: 'GET or POST only' } })
          return
        }
        const raw = await readBody(req)
        let result = await coreHttp(info, info.tokens.ui, 'POST', '/v1/model', JSON.parse(raw))
        if (result.status < 300 && result.body.config?.source?.kind === 'dsh') {
          // A newly added DSH model must be usable now, not after the old binding expires.
          // Only a successful model save rotates the binding; catalog reads never interrupt runs.
          try { await hostGateway.ensureBound(info, { force: true }) } catch { log('Zodo Host model refresh unavailable; no fallback was selected.') }
          result = await coreHttp(info, info.tokens.ui, 'GET', '/v1/model')
        }
        json(res, result.status, { ok: result.status < 300, core_status: result.status, result: result.body })
      } catch (error) {
        log(`model bridge error: ${error?.message ?? String(error)}`)
        json(res, 502, { ok: false, error: { code: error?.code ?? 'BRIDGE_INTERNAL', message: error?.message ?? 'model bridge failure' } })
      }
    },
  }))

  // Read-only projection of callable Host routes. No endpoints or credential references leave the
  // Host; the response contains only provider/model identities and display names. Nothing here
  // mutates the Harness configuration.
  disposers.push(ctx.webServer.register({
    kind: 'exact',
    path: '/zodo-bridge/v1/harness/models',
    handler: async (req, res) => {
      if (req.method !== 'GET') {
        json(res, 405, { ok: false, error: { code: 'METHOD_NOT_ALLOWED', message: 'GET only' } })
        return
      }
      const browserOrigin = req.headers.origin
      if (browserOrigin && !sameOrigin(req, origin()) && !allowedOrigins().has(browserOrigin)) {
        json(res, 403, { ok: false, error: { code: 'ORIGIN_DENIED', message: 'same-origin UI only' } })
        return
      }
      const token = uiSessionFrom(req)
      if (!sessions.verify(token, req.headers.origin)) {
        json(res, 401, { ok: false, error: { code: 'UI_SESSION_REQUIRED', message: 'mint a UI session first' } })
        return
      }
      try {
        const catalog = await readHarnessModelCatalog(ctx, log)
        json(res, 200, catalog)
      } catch (error) {
        log('harness model catalog unavailable; provider error withheld')
        json(res, error?.code === 'LLM_UNAVAILABLE' ? 503 : 502, {
          ok: false,
          error: {
            code: error?.code ?? 'HARNESS_MODEL_CATALOG_FAILED',
            message: 'Harness model catalog unavailable; provider error withheld.',
          },
        })
      }
    },
  }))

  disposers.push(ctx.webServer.register({
    kind: 'exact',
    path: '/zodo-bridge/v1/admin/revocations',
    handler: async (req, res) => {
      try {
        if (req.method !== 'GET') {
          json(res, 405, { ok: false, error: { code: 'METHOD_NOT_ALLOWED', message: 'GET only' } })
          return
        }
        const browserOrigin = req.headers.origin
        if (browserOrigin && !sameOrigin(req, origin()) && !allowedOrigins().has(browserOrigin)) {
          json(res, 403, { ok: false, error: { code: 'ORIGIN_DENIED', message: 'same-origin UI only' } })
          return
        }
        const token = uiSessionFrom(req)
        if (!sessions.verify(token, req.headers.origin)) {
          json(res, 401, { ok: false, error: { code: 'UI_SESSION_REQUIRED', message: 'mint a UI session first' } })
          return
        }
        const { info } = await core.ensure()
        const result = await coreHttp(info, info.tokens.ui, 'GET', '/v1/admin/revocations')
        json(res, result.status, { ok: result.status < 300, core_status: result.status, result: result.body })
      } catch (error) {
        log(`revocations bridge error: ${error?.message ?? String(error)}`)
        json(res, 502, { ok: false, error: { code: error?.code ?? 'BRIDGE_INTERNAL', message: error?.message ?? 'revocations bridge failure' } })
      }
    },
  }))

  disposers.push(ctx.webServer.register({
    kind: 'exact',
    path: '/zodo-bridge/v1/admin/revoke',
    handler: async (req, res) => {
      try {
        if (req.method !== 'POST') {
          json(res, 405, { ok: false, error: { code: 'METHOD_NOT_ALLOWED', message: 'POST only' } })
          return
        }
        const browserOrigin = req.headers.origin
        if (browserOrigin && !sameOrigin(req, origin()) && !allowedOrigins().has(browserOrigin)) {
          json(res, 403, { ok: false, error: { code: 'ORIGIN_DENIED', message: 'same-origin UI only' } })
          return
        }
        const token = uiSessionFrom(req)
        if (!sessions.verify(token, req.headers.origin)) {
          json(res, 401, { ok: false, error: { code: 'UI_SESSION_REQUIRED', message: 'mint a UI session first' } })
          return
        }
        const { info } = await core.ensure()
        const raw = await readBody(req)
        const result = await coreHttp(info, info.tokens.ui, 'POST', '/v1/admin/revoke', JSON.parse(raw))
        json(res, result.status, { ok: result.status < 300, core_status: result.status, result: result.body })
      } catch (error) {
        log(`revoke bridge error: ${error?.message ?? String(error)}`)
        json(res, 502, { ok: false, error: { code: error?.code ?? 'BRIDGE_INTERNAL', message: error?.message ?? 'revoke bridge failure' } })
      }
    },
  }))

  }
  install()

  // New private data is refused before it enters canonical native/PTC values.
  // For protected exports, use host-owned session records and tool provenance, not
  // words in messages. Scan the full log, including compressed/shadowed prefixes.
  ctx.on('llm/stream', (options, next) => {
    const sessionId = options?.sessionId ?? (hostGateway.owns(options) ? undefined : ctx.agents.currentInitiator()?.session?.id)
    if (sessionId && sessionHasRestrictedExport(privacy, ctx.sessions.get(sessionId))) {
      // No destination-bound private export contract is available in this Host.
      // Do not trust a provider name (or user-maintained allowlist) as proof of
      // locality: a local adapter can point at a remote endpoint after switching.
      throw privacyError('ZODO_HISTORY_RESTRICTED',
        'Zodo history contains a verified private export. Use a clean session and the private Zodo UI/Core local-only workflow.')
    }
    return next()
  })
  ctx.on('llm/adapters-updated', () => hostGateway.invalidate())
  ctx.on('session/created', session => {
    if (session?.inheritedEventCount > 0 && sessionHasRestrictedExport(privacy, session)) {
      throw privacyError('ZODO_FORK_RESTRICTED',
        'Cannot fork verified private Zodo history into Host chat. Use the private Zodo UI/Core workflow.')
    }
  })

  const guardRoute = async (req, res, handler) => {
    try {
      if (req.method !== 'POST' && req.method !== 'GET') {
        json(res, 405, { ok: false, error: { code: 'METHOD_NOT_ALLOWED', message: 'GET/POST only' } })
        return
      }
      const browserOrigin = req.headers.origin
      if (browserOrigin && !sameOrigin(req, ctx.webServer.getOrigin())) {
        json(res, 403, { ok: false, error: { code: 'ORIGIN_DENIED', message: 'same-origin UI only' } })
        return
      }
      const token = uiSessionFrom(req)
      if (!sessions.verify(token, req.headers.origin)) {
        json(res, 401, { ok: false, error: { code: 'UI_SESSION_REQUIRED', message: 'mint a UI session first' } })
        return
      }
      await handler(req, res)
    } catch (error) {
      json(res, 500, { ok: false, error: { code: 'GUARD_PROBE_FAILED', message: error?.message ?? String(error) } })
    }
  }

  const guardProbeDisposer = ctx.webServer.register({
    kind: 'exact',
    path: '/zodo-bridge/v1/admin/guard-probe',
    handler: async (req, res) => guardRoute(req, res, async () => {
      let blocked = false
      let code = ''
      try {
        assertHostExportAllowed({ data: { data_class: 'device_only', body: 'synthetic privacy probe' } })
      } catch (error) {
        code = error?.code ?? ''
        blocked = code === 'ZODO_DEVICE_ONLY_EXPORT_DENIED'
      }
      json(res, 200, {
        ok: blocked,
        blocked,
        code,
        guard: PRIVACY_GUARD_VERSION,
        stage: 'before-canonical-tool-value',
        model_requests: 0,
      })
    }),
  })

  const sessionProbeDisposer = ctx.webServer.register({
    kind: 'exact',
    path: '/zodo-bridge/v1/admin/history-guard',
    handler: async (req, res) => guardRoute(req, res, async () => {
      const live = ctx.sessions.list()
      const sessionsView = live.map(session => ({
        id: session.id,
        inheritedEventCount: session.inheritedEventCount ?? 0,
        restricted: sessionHasRestrictedExport(privacy, session),
      }))
      json(res, 200, { ok: true, sessions: sessionsView })
    }),
  })

  const forkGuardDisposer = ctx.webServer.register({
    kind: 'exact',
    path: '/zodo-bridge/v1/admin/fork-guard',
    handler: async (req, res) => guardRoute(req, res, async () => {
      const raw = await readBody(req)
      let sourceId = ''
      try {
        sourceId = String(JSON.parse(raw || '{}').source_id ?? '')
      } catch {
        sourceId = ''
      }
      if (!sourceId) {
        json(res, 422, { ok: false, error: { code: 'SOURCE_REQUIRED', message: 'source_id is required' } })
        return
      }
      try {
        const child = ctx.sessions.fork(sourceId)
        json(res, 200, { ok: true, blocked: false, child_id: child.id })
      } catch (error) {
        json(res, 409, { ok: false, blocked: true, message: error?.message ?? String(error) })
      }
    }),
  })

  const reloadDisposer = ctx.webServer.register({
    kind: 'exact',
    path: '/zodo-bridge/v1/admin/reload',
    handler: async (req, res) => {
      if (req.method !== 'POST') {
        json(res, 405, { ok: false, error: { code: 'METHOD_NOT_ALLOWED', message: 'POST only' } })
        return
      }
      const browserOrigin = req.headers.origin
      if (browserOrigin && !sameOrigin(req, ctx.webServer.getOrigin())) {
        json(res, 403, { ok: false, error: { code: 'ORIGIN_DENIED', message: 'same-origin UI only' } })
        return
      }
      const token = uiSessionFrom(req)
      if (!sessions.verify(token, req.headers.origin)) {
        json(res, 401, { ok: false, error: { code: 'UI_SESSION_REQUIRED', message: 'mint a UI session first' } })
        return
      }
      const before = disposers.length
      const oldGeneration = generation
      for (const dispose of disposers.splice(0)) {
        try {
          dispose()
        } catch {
          // Already disposed.
        }
      }
      generation += 1
      install()
      json(res, 200, {
        ok: true,
        generation,
        previous_generation: oldGeneration,
        subscriptions_before: before,
        subscriptions_after: disposers.length,
        core_alive: true,
      })
    },
  })

  ctx.effect(() => async () => {
    await hostGateway.close()
    for (const dispose of [guardProbeDisposer, sessionProbeDisposer, forkGuardDisposer, reloadDisposer]) {
      try {
        dispose()
      } catch {
        // Already disposed.
      }
    }
    for (const dispose of disposers.splice(0)) {
      try {
        dispose()
      } catch {
        // Already disposed during host teardown.
      }
    }
    sessions.clear()
    // The independent Core is not killed by plugin unload; it is a shared runtime with its own
    // single-writer lock, durable runs and receipts.
  })

  log(`zodo Host adapter active; data=${core.stateDir}; core protocol=${CORE_PROTOCOL}`)
}
