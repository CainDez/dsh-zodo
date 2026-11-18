import test from 'node:test'
import assert from 'node:assert/strict'
import { createHostGateway, readModelCatalog, requestCore } from './host-gateway.js'
import { access, mkdtemp, readFile, rm, stat } from 'node:fs/promises'
import { constants } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import { randomUUID } from 'node:crypto'

const info = { host: '127.0.0.1', port: 1234, instance_id: 'test-instance', workspace_id: 'ws_test', tokens: { ui: 'synthetic-admin' } }
function stubLlm(mode = 'ok') {
  const calls = []
  return { calls,
    listProviders: () => [{ id: 'openai-mock', name: 'OpenAI mock' }, { id: 'subscription-mock', name: 'Subscription mock' }],
    listModels: async provider => [{ provider, id: 'test-model', name: 'Test model' }],
    async *stream(options) {
      calls.push(options)
      // DSH adapters consume content blocks, not OpenAI-style message strings.
      assert.equal(options.messages.length, 1)
      const [message] = options.messages
      assert.deepEqual(Object.keys(message).sort(), ['content', 'role'])
      assert.equal(message.role, 'user')
      assert.ok(Array.isArray(message.content))
      assert.equal(message.content.length, 1)
      const [block] = message.content
      assert.deepEqual(Object.keys(block).sort(), ['text', 'type'])
      assert.equal(block.type, 'text')
      assert.equal(typeof block.text, 'string')
      if (mode === 'error') throw new Error('SYNTHETIC_PROVIDER_SECRET_MUST_NOT_LEAK')
      if (mode === 'wait') {
        await new Promise(resolve => { if (options.signal.aborted) resolve(); else options.signal.addEventListener('abort', resolve, { once: true }) })
        throw new Error('SYNTHETIC_PROVIDER_SECRET_MUST_NOT_LEAK')
      }
      const result = options.system.includes('clarify')
        ? { questions: ['Synthetic clarification?'], suggested_kind: 'task', assumptions: [] }
        : { tasks: [{ title: 'Synthetic proposed task', estimate_minutes: 30, priority: 2, depends_on: [] }], assumptions: [], questions: [] }
      yield { type: 'text-delta', index: 0, text: mode === 'large' ? 'x'.repeat(200 * 1024) : JSON.stringify(result) }
      yield { type: 'finish', reason: { kind: 'stop' } }
    },
  }
}
async function fixture(mode = 'ok') {
  let binding
  let allowed = true
  const used = new Set()
  const llm = stubLlm(mode)
  const gateway = createHostGateway({ llm, coreRequest: async (_info, route, body) => {
    if (route === '/v1/host/bind') { binding = body; return { bound: true } }
    if (route === '/v1/host/unbind') return { bound: false }
    assert.equal(route, '/v1/host/authorize')
    if (!allowed || (body.phase === 'dispatch' && used.has(body.request_id))) throw new Error('denied')
    if (body.phase === 'dispatch') used.add(body.request_id)
    return { authorized: true }
  } })
  await gateway.ensureBound(info)
  const body = () => ({ schema: 'zodo.host-call/1', instance_id: info.instance_id, binding_id: binding.binding_id,
    request_id: randomUUID().replaceAll('-', ''), run_id: 'run_test', workspace_id: info.workspace_id,
    provider: 'subscription-mock', model: 'test-model', scope: 'goal.decompose', data_class: 'trusted_workspace',
    policy_revision: 1, model_revision: 0, expires_at_ms: Date.now() + 55_000,
    messages: [{ role: 'system', content: 'Core system prompt' }, { role: 'user', content: 'Core goal only' }] })
  return { gateway, llm, body, revoke: () => { allowed = false },
    call: async (body, headers = {}) => {
      const response = await fetch(binding.endpoint, { method: 'POST', headers: { 'Content-Type': 'application/json', Authorization: 'Bearer ' + binding.token, ...headers }, body: JSON.stringify(body) })
      return { status: response.status, body: await response.json() }
    },
  }
}

test('catalog accepts ordinary and subscription routes without reading settings or credentials', async () => {
  const llm = stubLlm()
  const catalog = await readModelCatalog(llm)
  assert.equal(catalog.groups.length, 2)
  for (const group of catalog.groups) {
    assert.deepEqual(Object.keys(group).sort(), ['models', 'name', 'provider', 'supported'])
    assert.equal(group.supported, true)
    assert.equal(group.models[0].supported, true)
  }
  assert.equal(llm.calls.length, 0)
})

test('callback rejects forged token, origin, arbitrary fields/models/scope/class and stale authorization before dispatch', async () => {
  const f = await fixture()
  try {
    assert.equal((await f.call(f.body(), { Authorization: 'Bearer forged' })).status, 403)
    assert.equal((await f.call(f.body(), { Origin: 'http://127.0.0.1' })).status, 403)
    for (const patch of [
      { schema: 'wrong' }, { instance_id: 'another-instance' }, { workspace_id: 'other' }, { binding_id: 'other' },
      { provider: 'arbitrary' }, { model: 'arbitrary' }, { scope: 'chat' }, { data_class: 'device_only' },
      { expires_at_ms: Date.now() - 1 }, { sessionId: 'private-history' },
      { messages: [{ role: 'system', content: 'x' }, { role: 'user', content: 'x', history: [] }] },
    ]) assert.equal((await f.call({ ...f.body(), ...patch })).status, 502)
    f.revoke()
    assert.equal((await f.call(f.body())).status, 502)
    assert.equal(f.llm.calls.length, 0)
  } finally { await f.gateway.close() }
})

test('callback maps Core user text to a DSH content block and prevents replay', async () => {
  const f = await fixture()
  try {
    const body = f.body()
    assert.equal((await f.call(body)).status, 200)
    assert.equal((await f.call(body)).status, 502)
    assert.equal(f.llm.calls.length, 1)
    const options = f.llm.calls[0]
    assert.deepEqual(Object.keys(options).sort(), ['maxTokens', 'messages', 'model', 'provider', 'signal', 'system'])
    assert.equal(options.provider, 'subscription-mock')
    assert.equal(options.model, 'test-model')
    assert.equal(options.system, 'Core system prompt')
    assert.deepEqual(options.messages, [{ role: 'user', content: [{ type: 'text', text: body.messages[1].content }] }])
  } finally { await f.gateway.close() }
})

test('callback aborts on current authorization revocation and redacts all provider failures', async () => {
  const f = await fixture('wait')
  try {
    const pending = f.call(f.body())
    const deadline = Date.now() + 3000
    while (!f.llm.calls.length && Date.now() < deadline) await new Promise(resolve => setTimeout(resolve, 10))
    assert.equal(f.llm.calls.length, 1)
    f.revoke()
    const result = await pending
    assert.equal(result.status, 502)
    assert.equal(f.llm.calls[0].signal.aborted, true)
    assert.deepEqual(result.body, { error: 'HOST_MODEL_CALL_FAILED' })
  } finally { await f.gateway.close() }
  for (const mode of ['error', 'large']) {
    const f = await fixture(mode)
    try { assert.deepEqual((await f.call(f.body())).body, { error: 'HOST_MODEL_CALL_FAILED' }) } finally { await f.gateway.close() }
  }
})

const binary = process.env.ZODO_CORE_TEST_BIN || fileURLToPath(new URL('../../../target/debug/' + (process.platform === 'win32' ? 'zodo.exe' : 'zodo'), import.meta.url))

test('isolated real Core run -> Host llm ordinary/subscription mock -> proposal, never fallback or private context', { timeout: 45_000 }, async () => {
  try {
    if (!(await stat(binary)).isFile()) throw new Error('Not a file')
    await access(binary, constants.X_OK)
  } catch {
    throw new Error('Test Core executable unavailable. Run cargo build -p zodo-runtime --locked or set ZODO_CORE_TEST_BIN to that build.')
  }
  const dir = await mkdtemp(path.join(tmpdir(), 'zodo-host-contract-'))
  const proc = spawn(binary, ['--data-dir', dir, 'serve', '--port', '0'], { stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true })
  let failedToSpawn = false
  proc.on('error', () => { failedToSpawn = true })
  const finished = new Promise(resolve => proc.once('close', resolve))
  let gateway
  let stdout = ''
  proc.stdout.on('data', chunk => { stdout = (stdout + chunk.toString()).slice(-16_384) })
  proc.stderr.resume() // Do not print process logs containing runtime credentials.
  let seq = 0
  try {
    let runtime
    const end = Date.now() + 10_000
    while (!runtime && Date.now() < end) {
      if (failedToSpawn) throw new Error('Could not start the test Core executable.')
      if (proc.exitCode !== null || proc.signalCode !== null) throw new Error('Synthetic Core exited before readiness')
      for (const line of stdout.split(String.fromCharCode(10))) {
        try { const parsed = JSON.parse(line); if (parsed.protocol === 'zodo/1' && parsed.pid === proc.pid) runtime = parsed } catch {}
      }
      if (!runtime) await new Promise(resolve => setTimeout(resolve, 20))
    }
    assert.ok(runtime, 'Synthetic Core must start')
    const api = async (method, route, body, token = runtime.tokens.ui) => {
      const response = await fetch('http://127.0.0.1:' + runtime.port + route, { method,
        headers: { 'Content-Type': 'application/json', Authorization: 'Bearer ' + token },
        ...(body === undefined ? {} : { body: JSON.stringify(body) }) })
      return { status: response.status, body: await response.json() }
    }
    const command = async (operation, payload, token) => {
      const key = 'host-test-' + (++seq)
      return api('POST', '/v1/commands', { schema: 'zodo.command/1', request_id: key, idempotency_key: key,
        workspace_id: runtime.workspace_id, operation, operation_version: 1, payload }, token)
    }
    const policy = async (goals, projects = false, captures = false) => {
      const result = await command('workspace.update', { policy: { model_access: { goals, projects, captures }, diagnostic_capture: false } })
      assert.equal(result.status, 200)
    }
    const select = async provider => {
      const result = await api('POST', '/v1/model', { version: 2, source: { kind: 'dsh', provider, model: 'test-model' } })
      assert.equal(result.status, 200)
      assert.equal(result.body.effective.connection_tested, false)
      return result.body
    }
    const run = async (intent, input) => {
      const created = await api('POST', '/v1/runs', { intent, input, idempotency_key: 'run-' + (++seq) })
      assert.equal(created.status, 202)
      const deadline = Date.now() + 10_000
      while (Date.now() < deadline) {
        const result = await api('GET', '/v1/runs/' + created.body.run.id)
        if (['succeeded', 'failed', 'cancelled'].includes(result.body.run.state)) return result.body.run
        await new Promise(resolve => setTimeout(resolve, 30))
      }
      throw new Error('Synthetic run did not finish')
    }
    const goal = await command('goal.create', { goal: { title: 'ALLOWED_GOAL', success_criteria: 'ALLOWED_SUCCESS' } })
    const project = await command('project.create', { project: { title: 'DENIED_PROJECT_TITLE', scope: 'DENIED_PROJECT_SCOPE' } })
    const note = await command('note.create', { note: { title: 'PRIVATE_NOTE', body: 'DEVICE_ONLY_SECRET_NEVER_SEND', data_class: 'device_only' } })
    assert.equal(goal.status, 200); assert.equal(project.status, 200); assert.equal(note.status, 200)
    const input = { goal_id: goal.body.data.changes[0].id, project_id: project.body.data.changes[0].id }
    await policy(true)
    assert.equal((await select('openai-mock')).effective.selected.reason, 'host_unavailable')
    const missing = await run('goal.decompose', input)
    assert.equal(missing.state, 'failed')
    assert.equal(missing.error.message, 'host_unavailable')
    let hostToken
    const llm = stubLlm()
    gateway = createHostGateway({ llm, coreRequest: (info, route, body) => {
      if (route === '/v1/host/bind') hostToken = body.token
      return requestCore(info, route, body)
    } })
    await gateway.ensureBound(runtime)
    const results = []
    for (const provider of ['openai-mock', 'subscription-mock']) {
      await select(provider)
      const result = await run('goal.decompose', input)
      assert.equal(result.state, 'succeeded')
      assert.equal(result.output.source, 'dsh')
      assert.ok(result.output.proposal_id)
      results.push(result)
    }
    assert.equal(llm.calls.length, 2)
    assert.deepEqual(llm.calls.map(call => call.provider), ['openai-mock', 'subscription-mock'])
    for (const call of llm.calls) {
      const text = JSON.stringify(call.messages)
      assert.ok(text.includes('ALLOWED_GOAL'))
      for (const forbidden of ['DENIED_PROJECT_TITLE', 'DENIED_PROJECT_SCOPE', 'DEVICE_ONLY_SECRET_NEVER_SEND', input.project_id]) assert.ok(!text.includes(forbidden))
      assert.equal(call.sessionId, undefined); assert.equal(call.tools, undefined); assert.equal(call.history, undefined)
    }
    await policy(false)
    assert.equal((await run('goal.decompose', input)).error.message, 'goals_not_allowed')
    assert.equal((await run('capture.clarify', { text: 'UNAUTHORIZED_CAPTURE' })).error.message, 'captures_not_allowed')
    await policy(true)
    assert.equal((await run('goal.decompose', { ...input, data_class: 'device_only' })).error.message, 'device_only_requires_custom_loopback')
    assert.equal(llm.calls.length, 2)
    await policy(true, false, true)
    assert.equal((await run('capture.clarify', { text: 'AUTHORIZED_CAPTURE' })).state, 'succeeded')
    assert.equal(llm.calls.length, 3)
    assert.ok(llm.calls[2].messages[0].content[0].text.includes('AUTHORIZED_CAPTURE'))
    // Pause registry lookup, revoke actual Core policy, then release callback: zero new dispatch.
    let releaseLookup
    let reachedLookup
    const lookupGate = new Promise(resolve => { releaseLookup = resolve })
    const lookupReached = new Promise(resolve => { reachedLookup = resolve })
    const listModels = llm.listModels
    llm.listModels = async provider => { reachedLookup(); await lookupGate; return listModels(provider) }
    const delayed = run('goal.decompose', input)
    await lookupReached
    await policy(false)
    releaseLookup()
    assert.equal((await delayed).state, 'failed')
    assert.equal(llm.calls.length, 3)
    llm.listModels = listModels
    const approve = await command('proposal.approve', { proposal_id: results[0].output.proposal_id }, runtime.tokens.agent)
    assert.equal(approve.status, 403)
    const snapshot = await api('POST', '/v1/queries', { schema: 'zodo.query/1', request_id: 'snapshot', workspace_id: runtime.workspace_id, query: 'workspace.snapshot', payload: {} })
    assert.equal(snapshot.body.data.tasks.length, 0, 'model output only produces proposals')
    const model = (await api('GET', '/v1/model')).body
    for (const value of [model, ...results, snapshot.body]) assert.ok(!JSON.stringify(value).includes(hostToken))
    for (const file of ['model.json', 'runtime.json']) assert.ok(!(await readFile(path.join(dir, file), 'utf8')).includes(hostToken))
    // Non-admin and browser calls cannot install even a loopback callback capability.
    assert.equal((await api('POST', '/v1/host/bind', { schema: 'zodo.host-binding/1', instance_id: runtime.instance_id, binding_id: 'a'.repeat(32), endpoint: 'http://127.0.0.1:1/zodo-host/v1/complete', token: 'b'.repeat(64), expires_at_ms: Date.now() + 1000, routes: [] }, runtime.tokens.agent)).status, 403)
  } finally {
    try {
      if (gateway) await gateway.close()
    } finally {
      if (proc.pid !== undefined && proc.exitCode === null && proc.signalCode === null) proc.kill()
      await finished
      await rm(dir, { recursive: true, force: true })
    }
  }
})

test('explicit DSH save refreshes newly added catalog routes without inference or chat selection', async () => {
  const llm = stubLlm()
  const calls = []
  let added = false
  llm.listModels = async provider => [{ provider, id: 'test-model', name: 'Test model' },
    ...(added ? [{ provider, id: 'new-model', name: 'New model' }] : [])]
  const gateway = createHostGateway({ llm, coreRequest: async (_info, route, body) => {
    calls.push({ route, body }); return { bound: true }
  } })
  try {
    await gateway.ensureBound(info)
    added = true
    await gateway.ensureBound(info)
    assert.equal(calls.filter(row => row.route === '/v1/host/bind').length, 1)
    await gateway.ensureBound(info, { force: true })
    const binds = calls.filter(row => row.route === '/v1/host/bind')
    assert.equal(binds.length, 2)
    assert.equal(binds[1].body.routes.some(route => route.model === 'new-model'), true)
    assert.notEqual(binds[0].body.token, binds[1].body.token)
    assert.equal(llm.calls.length, 0)
  } finally { await gateway.close() }
})
