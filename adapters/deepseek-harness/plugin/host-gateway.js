/** Authenticated, ephemeral Core-only callback into the real Host llm service.
 * No provider credentials, chat defaults, histories, tools or sessions are read here.
 */
import { createServer, request as httpRequest } from 'node:http'
import { randomBytes, createHash, timingSafeEqual } from 'node:crypto'
import { isIP } from 'node:net'

const PATH = '/zodo-host/v1/complete'
const MAX_INPUT = 128 * 1024
const MAX_OUTPUT = 192 * 1024
const CALL_MS = 60_000
const BIND_MS = 540_000

const failure = code => Object.assign(new Error(code), { code })
const isObject = value => value !== null && typeof value === 'object' && !Array.isArray(value)
function keys(value, expected) {
  return isObject(value) && Object.keys(value).length === expected.length && expected.every(key => Object.hasOwn(value, key))
}
function loopback(host) {
  const bare = String(host).replace(/^\[|\]$/g, '')
  return bare === 'localhost' || (isIP(bare) === 4 && bare.startsWith('127.')) || bare === '::1'
}
function sameSecret(received, expected) {
  if (typeof received !== 'string') return false
  const actual = Buffer.from(received)
  const wanted = Buffer.from(expected)
  return actual.length === wanted.length && timingSafeEqual(actual, wanted)
}
function send(res, status, body) {
  if (res.destroyed || res.writableEnded) return
  res.writeHead(status, { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' })
  res.end(JSON.stringify(body))
}

// Explicit literal loopback connection: no DNS, proxy, redirect, or external URL support.
export function requestCore(info, pathname, body) {
  if (!loopback(info.host) || !Number.isInteger(info.port) || info.port < 1 || info.port > 65535) throw failure('HOST_CORE_NOT_LOOPBACK')
  const hostname = info.host === 'localhost' ? '127.0.0.1' : String(info.host).replace(/^\[|\]$/g, '')
  const bytes = Buffer.from(JSON.stringify(body))
  return new Promise((resolve, reject) => {
    const req = httpRequest({ hostname, port: info.port, path: pathname, method: 'POST',
      headers: { Authorization: 'Bearer ' + info.tokens.ui, 'Content-Type': 'application/json', 'Content-Length': bytes.length },
      agent: false, timeout: 5000,
    }, res => {
      let size = 0
      const chunks = []
      res.on('data', chunk => {
        size += chunk.length
        if (size > 16 * 1024) { res.destroy(); reject(failure('HOST_CORE_RESPONSE_INVALID')); return }
        chunks.push(chunk)
      })
      res.on('error', () => reject(failure('HOST_CORE_UNAVAILABLE')))
      res.on('end', () => {
        if (res.statusCode !== 200) { reject(failure('HOST_CORE_AUTHORIZATION_DENIED')); return }
        try { resolve(JSON.parse(Buffer.concat(chunks).toString('utf8'))) } catch { reject(failure('HOST_CORE_RESPONSE_INVALID')) }
      })
    })
    req.on('timeout', () => req.destroy(failure('HOST_CORE_TIMEOUT')))
    req.on('error', () => reject(failure('HOST_CORE_UNAVAILABLE')))
    req.end(bytes)
  })
}

export async function readModelCatalog(llm) {
  if (!llm || typeof llm.stream !== 'function' || typeof llm.listProviders !== 'function' || typeof llm.listModels !== 'function') throw failure('LLM_UNAVAILABLE')
  const groups = []
  const failures = []
  for (const provider of await llm.listProviders()) {
    if (typeof provider?.id !== 'string' || !provider.id || provider.id.length > 200) continue
    try {
      const models = (await llm.listModels(provider.id)).filter(model => typeof model?.id === 'string' && model.id && model.id.length <= 200)
        .map(model => ({ id: model.id, name: typeof model.name === 'string' ? model.name : model.id, supported: true }))
      groups.push({ provider: provider.id, name: typeof provider.name === 'string' ? provider.name : provider.id, supported: true, models })
    } catch { failures.push({ provider: provider.id, message: 'HOST_MODEL_CATALOG_UNAVAILABLE' }) }
  }
  return { groups, failures }
}

function validateCall(body, binding, now) {
  if (!keys(body, ['schema','instance_id','binding_id','request_id','run_id','workspace_id','provider','model','scope','data_class','policy_revision','model_revision','expires_at_ms','messages'])) throw failure('HOST_REQUEST_INVALID')
  if (body.schema !== 'zodo.host-call/1' || body.instance_id !== binding.info.instance_id || body.binding_id !== binding.id
    || body.workspace_id !== binding.info.workspace_id || now >= binding.expires
    || !Number.isSafeInteger(body.expires_at_ms) || body.expires_at_ms <= now || body.expires_at_ms > now + CALL_MS
    || body.expires_at_ms > binding.expires || !/^[a-f0-9]{32}$/.test(body.request_id)
    || typeof body.run_id !== 'string' || body.run_id.length > 200
    || !['goal.decompose','capture.clarify'].includes(body.scope)
    || !['trusted_workspace','cloud_allowed'].includes(body.data_class)
    || !Number.isSafeInteger(body.policy_revision) || !Number.isSafeInteger(body.model_revision)
    || !binding.routes.some(route => route.provider === body.provider && route.model === body.model)) throw failure('HOST_REQUEST_REJECTED')
  if (!Array.isArray(body.messages) || body.messages.length !== 2 || body.messages[0]?.role !== 'system' || body.messages[1]?.role !== 'user'
    || body.messages.some(message => !keys(message, ['role','content']) || typeof message.content !== 'string')
    || body.messages.reduce((sum, message) => sum + Buffer.byteLength(message.content), 0) > 64 * 1024) throw failure('HOST_MESSAGES_REJECTED')
}

function abortable(promise, signal) {
  return new Promise((resolve, reject) => {
    const aborted = () => reject(failure('HOST_CALL_ABORTED'))
    if (signal.aborted) { aborted(); return }
    signal.addEventListener('abort', aborted, { once: true })
    Promise.resolve(promise).then(resolve, reject).finally(() => signal.removeEventListener('abort', aborted))
  })
}

export function createHostGateway({ llm, coreRequest = requestCore, now = Date.now }) {
  let binding
  let opening
  let bindingPromise
  let server
  let closed = false
  let renewal
  let lastInfo
  const owned = new WeakSet()
  const active = new Set()

  const handle = async (req, res) => {
    const current = binding
    if (req.url !== PATH || req.method !== 'POST') { send(res, 404, { error: 'HOST_ROUTE_NOT_FOUND' }); return }
    if (!current || req.headers.origin || !sameSecret(req.headers.authorization, 'Bearer ' + current.token)) { send(res, 403, { error: 'HOST_AUTHORIZATION_DENIED' }); return }
    const controller = new AbortController()
    active.add(controller)
    req.on('aborted', () => controller.abort())
    res.on('close', () => { if (!res.writableEnded) controller.abort() })
    let timer
    let monitor
    let iterator
    try {
      let size = 0
      const chunks = []
      for await (const chunk of req) {
        size += chunk.length
        if (size > MAX_INPUT) throw failure('HOST_INPUT_TOO_LARGE')
        chunks.push(chunk)
      }
      const raw = Buffer.concat(chunks)
      const body = JSON.parse(raw.toString('utf8'))
      validateCall(body, current, now())
      timer = setTimeout(() => controller.abort(), Math.max(1, body.expires_at_ms - now()))
      const authorization = { instance_id: body.instance_id, binding_id: body.binding_id, request_id: body.request_id,
        body_sha256: createHash('sha256').update(raw).digest('hex'), phase: 'dispatch' }
      // Revalidate registry, then make Core's fresh one-shot policy/model/run authorization the LAST
      // awaited step before dispatch. Core matches the hash to its exact, internally-built payload.
      const models = await abortable(llm.listModels(body.provider), controller.signal)
      if (!models.some(model => model.id === body.model)) throw failure('HOST_MODEL_UNAVAILABLE')
      const check = async phase => {
        const result = await coreRequest(current.info, '/v1/host/authorize', { ...authorization, phase })
        if (result?.authorized !== true || current !== binding || controller.signal.aborted) throw failure('HOST_AUTHORIZATION_STALE')
      }
      await abortable(check('dispatch'), controller.signal)
      // The Core wire format uses strings; DSH one-shot user input uses typed content blocks.
      const options = { provider: body.provider, model: body.model, system: body.messages[0].content,
        messages: [{ role: 'user', content: [{ type: 'text', text: body.messages[1].content }] }],
        maxTokens: 4096, signal: controller.signal }
      owned.add(options)
      iterator = llm.stream(options)[Symbol.asyncIterator]()
      let checking = false
      monitor = setInterval(() => {
        if (checking) return
        checking = true
        check('continue').catch(() => controller.abort()).finally(() => { checking = false })
      }, 200)
      let content = ''
      let outputBytes = 0
      let generatedBytes = 0
      let finished = false
      while (true) {
        const next = await abortable(iterator.next(), controller.signal)
        if (next.done) break
        const chunk = next.value
        if (chunk.type === 'tool-call-delta' || (chunk.type === 'block-start' && chunk.blockType === 'tool-call')) throw failure('HOST_UNEXPECTED_TOOL_CALL')
        if (chunk.type === 'text-delta' || chunk.type === 'reasoning-delta') {
          if (typeof chunk.text !== 'string') throw failure('HOST_INVALID_STREAM')
          generatedBytes += Buffer.byteLength(chunk.text)
          if (generatedBytes > 256 * 1024) throw failure('HOST_OUTPUT_TOO_LARGE')
        }
        if (chunk.type === 'text-delta') {
          if (typeof chunk.text !== 'string' || finished) throw failure('HOST_INVALID_STREAM')
          outputBytes += Buffer.byteLength(chunk.text)
          if (outputBytes > MAX_OUTPUT) throw failure('HOST_OUTPUT_TOO_LARGE')
          content += chunk.text
        }
        if (chunk.type === 'finish') {
          if (finished || chunk.reason?.kind !== 'stop') throw failure('HOST_MODEL_FAILED')
          finished = true
        }
      }
      if (!finished || !content.trim()) throw failure('HOST_INVALID_STREAM')
      await abortable(check('continue'), controller.signal)
      send(res, 200, { content })
    } catch {
      // Never echo provider errors, raw responses, credentials, prompts, or authorization details.
      send(res, 502, { error: 'HOST_MODEL_CALL_FAILED' })
    } finally {
      clearTimeout(timer)
      clearInterval(monitor)
      controller.abort()
      active.delete(controller)
      if (iterator?.return) Promise.resolve(iterator.return()).catch(() => {})
    }
  }

  const open = async () => {
    if (closed) throw failure('HOST_GATEWAY_CLOSED')
    if (!opening) opening = new Promise((resolve, reject) => {
      server = createServer((req, res) => { void handle(req, res) })
      server.requestTimeout = 10_000
      server.headersTimeout = 5_000
      server.on('error', () => reject(failure('HOST_GATEWAY_UNAVAILABLE')))
      server.listen(0, '127.0.0.1', () => resolve(server.address().port))
      server.unref()
    })
    return opening
  }
  const gateway = {
    owns: options => owned.has(options),
    async ensureBound(info, { force = false } = {}) {
      if (closed) throw failure('HOST_GATEWAY_CLOSED')
      lastInfo = info
      if (!loopback(info.host)) throw failure('HOST_CORE_NOT_LOOPBACK')
      if (!force && binding?.info.instance_id === info.instance_id && binding.expires > now() + 60_000) return
      if (bindingPromise) return bindingPromise
      bindingPromise = (async () => {
        const port = await open()
        const { groups } = await readModelCatalog(llm)
        const routes = groups.flatMap(group => group.models.map(model => ({ provider: group.provider, model: model.id })))
        const next = { info, id: randomBytes(16).toString('hex'), token: randomBytes(32).toString('hex'), expires: now() + BIND_MS, routes }
        await coreRequest(info, '/v1/host/bind', { schema: 'zodo.host-binding/1', instance_id: info.instance_id,
          binding_id: next.id, token: next.token, endpoint: 'http://127.0.0.1:' + port + PATH, expires_at_ms: next.expires, routes })
        if (closed) {
          try { await coreRequest(info, '/v1/host/unbind', { instance_id: info.instance_id, binding_id: next.id }) } catch {}
          throw failure('HOST_GATEWAY_CLOSED')
        }
        binding = next
        if (!renewal) {
          renewal = setInterval(() => { if (lastInfo && !closed) void gateway.ensureBound(lastInfo).catch(() => {}) }, 30_000)
          renewal.unref()
        }
        for (const controller of active) controller.abort()
      })()
      try { await bindingPromise } finally { bindingPromise = undefined }
    },
    invalidate() { binding = undefined; for (const controller of active) controller.abort() },
    async close() {
      closed = true
      clearInterval(renewal)
      const current = binding
      binding = undefined
      for (const controller of active) controller.abort()
      if (server) { server.closeAllConnections(); await new Promise(resolve => server.close(resolve)) }
      if (current) {
        try { await coreRequest(current.info, '/v1/host/unbind', { instance_id: current.info.instance_id, binding_id: current.id }) } catch {}
      }
    },
  }
  return gateway
}
