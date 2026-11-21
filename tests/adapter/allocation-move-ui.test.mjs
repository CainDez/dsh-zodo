import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import vm from 'node:vm'

const source = readFileSync(new URL('../../adapters/deepseek-harness/plugin/client.js', import.meta.url), 'utf8')
const part = (from, to) => source.slice(source.indexOf(from), source.indexOf(to, source.indexOf(from)))
const common = [
  part('    function workspaceTimezone', '    function truncateText'),
  part('    function agendaDate', '    function agendaShift'),
  part('    function scheduleRecovery', '    function firstScheduleStep'),
  part('    function allocationRemovalReason', '    function DuplicateSuggestions'),
  part('    function PlanModal', '    function entityFactRows'),
].join('\n')

const row = { id: 'a', task_id: 't', locked: false, state: 'planned', minutes: 60,
  start: '2026-11-01T03:00:00Z', end: '2026-11-01T04:00:00Z', source: { kind: 'user' } }
const timeZone = 'America/New_York'
const snapshot = { workspace: { timezone: timeZone, revision: 5 }, tasks: [{ id: 't', title: 'Task', version: 2 }],
  allocations: [row], capacity_blocks: [{ id: 'c', title: 'Existing', start: '2026-11-01T04:00:00Z', end: '2026-11-01T09:00:00Z' }] }
const original = { row, taskVersion: 2, timeZone }
const fold = +new Date('2026-11-01T05:30:00Z')
const form = { capacity: 'c', start: '2026-11-01T01:30', startChoice: fold }
const proposalFor = op => ({ id: 'p', status: 'validated', base_revision: 5, operations: [op], intent: 'Do not infer slots from this text',
  preview: { added: [], removed: [], changed: [{ kind: 'allocation', id: 'a', before: row,
    after: { ...row, capacity_block_id: op.payload.capacity_block_id, start: op.payload.start, end: op.payload.end } }] } })
const expectedOp = { operation: 'allocation.move', payload: { id: 'a', expected_task_version: 2, capacity_block_id: 'c',
  start: '2026-11-01T05:30:00.000Z', end: '2026-11-01T06:30:00.000Z' } }
const h = (tag, props, ...children) => ({ tag, props, children })
const find = (node, id) => node?.props?.['data-testid'] === id || node?.props?.testid === id
  ? node : (node?.children ?? []).flat(Infinity).map(child => find(child, id)).find(Boolean)
const byId = (tree, id) => find(tree, id) ?? find(tree.props.footer, id)
const clone = value => JSON.parse(JSON.stringify(value))

function harness() {
  const hooks = [], edits = [], closed = [], writes = []
  let cursor = 0, now = +new Date('2026-11-01T00:01:00Z')
  const mockDate = class extends Date { static now() { return now } }
  const api = vm.runInNewContext(common + '\n({ MoveAllocation, TimeDetail, PlanModal, moveDraftSlot })', {
    h, Date: mockDate, Modal: 'Modal', Section: 'Section', Button: 'Button', Field: 'Field', Badge: 'Badge', Empty: 'Empty',
    React: { Fragment: 'Fragment' }, COPY: { close: '关闭', cancel: '取消', approve: '批准' },
    useState(initial) { const i = cursor++; if (!(i in hooks)) hooks[i] = initial; return [hooks[i], value => { hooks[i] = value }] },
    useRef(initial) { const i = cursor++; if (!(i in hooks)) hooks[i] = { current: initial }; return hooks[i] },
    stateSnapshotRef: { current: snapshot }, dateTimeFor: value => value, taskTitle: (_, id) => id === 't' ? 'Task' : 'Unknown',
  })
  const draw = (component, extra = {}) => {
    cursor = 0
    return api[component]({ original, snapshot, busy: false, onClose: () => closed.push(true), setModal: modal => edits.push(modal),
      send: async (op, payload) => { writes.push([op, payload]); return { data: { proposal: proposalFor(payload.operations[0]) } } },
      ...extra })
  }
  return { draw, edits, closed, writes, api, setTime: value => { now = +new Date(value) } }
}

test('move detail entry guards current lock and opens/cancels without writes', () => {
  const f = harness()
  const detail = f.draw('TimeDetail', { kind: 'allocation', row })
  const open = byId(detail, 'time-detail-move')
  assert.equal(open.props.disabled, false)
  open.props.onClick()
  assert.equal(f.edits[0].type, 'allocation-move')
  const editor = harness()
  editor.draw('MoveAllocation').props.onClose()
  assert.equal(editor.closed.length, 1)
  assert.equal(editor.writes.length, 0)
  assert.equal(f.writes.length, 0)
  f.setTime(row.start)
  open.props.onClick()
  assert.equal(f.edits.length, 1, 'a stale enabled control still must gate current time')
  const locked = { ...row, locked: true }
  assert.equal(byId(harness().draw('TimeDetail', { kind: 'allocation', row: locked, snapshot: { ...snapshot, allocations: [locked] } }), 'time-detail-move').props.disabled, true)
  assert.equal(byId(harness().draw('TimeDetail', { kind: 'capacity', row }), 'time-detail-move'), undefined)
})

test('DST gap refuses guesses and fold requires choosing UTC; duration is elapsed time', async () => {
  const f = harness()
  assert.equal(f.api.moveDraftSlot({ start: '2026-03-08T02:30' }, timeZone, 60).start, null)
  let tree = f.draw('MoveAllocation', { savedForm: { ...form, startChoice: null } })
  assert.equal(byId(tree, 'move-slot-fold').props.value, '')
  await byId(tree, 'move-slot-preview').props.onClick()
  assert.equal(f.writes.length, 0)
  byId(tree, 'move-slot-fold').props.onChange({ target: { value: String(fold) } })
  tree = f.draw('MoveAllocation')
  assert.match(byId(tree, 'move-slot-utc').children[0], /05:30:00.*06:30:00/)
  assert.match(byId(tree, 'move-slot-end').children[0], /01:30/)
  await byId(tree, 'move-slot-preview').props.onClick()
  assert.deepEqual(clone(f.writes), [['proposal.create', { intent: '移动任务「Task」的已安排时段', operations: [expectedOp] }]])
  assert.deepEqual(clone(f.edits[0].moveEditor.savedForm), form)
})

test('changed task, source, capacity and Core failure preserve the draft without fallback', async () => {
  for (const changed of [
    { ...snapshot, tasks: [{ id: 't', title: 'Task', version: 3 }] },
    { ...snapshot, allocations: [{ ...row, updated_at: '2026-11-01T00:02:00Z' }] },
    { ...snapshot, capacity_blocks: [] },
  ]) {
    const f = harness()
    const tree = f.draw('MoveAllocation', { savedForm: form, snapshot: changed })
    await byId(tree, 'move-slot-preview').props.onClick()
    assert.equal(f.writes.length, 0)
    assert.equal(byId(f.draw('MoveAllocation', { snapshot: changed }), 'move-slot-start').props.value, form.start)
  }
  for (const send of [
    async () => ({ data: { proposal: { status: 'draft', diagnostics: [{ message: 'retained successor raw_PRIVATE' }] } } }),
    async () => ({ data: { proposal: { status: 'validated' } } }),
    async () => { throw Object.assign(new Error('unsupported PRIVATE'), { code: 'UNSUPPORTED_OPERATION' }) },
  ]) {
    const f = harness()
    await byId(f.draw('MoveAllocation', { savedForm: form, send }), 'move-slot-preview').props.onClick()
    const editor = f.draw('MoveAllocation')
    assert.equal(byId(editor, 'move-slot-start').props.value, form.start)
    assert.match(JSON.stringify(editor), /输入已保留/)
    assert.doesNotMatch(JSON.stringify(editor), /PRIVATE/)
    assert.equal(f.edits.length, 0)
  }
})

test('review renders structural capacity/clock diff; extra changes or existing grant prevent apply', async () => {
  for (const mode of ['good', 'grant', 'uncertain', 'extra', 'source changed']) {
    const f = harness()
    const p = proposalFor(expectedOp)
    if (mode === 'extra') p.preview.added.push({ kind: 'task' })
    if (mode === 'source changed') p.preview.changed[0].after.source = { kind: 'different' }
    let applied = 0
    const tree = f.draw('PlanModal', { proposal: p, snapshotRevision: 5,
      grant: mode === 'grant', moveUncertain: mode === 'uncertain', onApprove: async () => { applied++ } })
    if (mode === 'good' || mode === 'grant') {
      assert.match(JSON.stringify(byId(tree, 'plan-move-before')), /03:00:00Z.*未绑定容量|未绑定容量.*03:00:00Z/)
      assert.match(JSON.stringify(byId(tree, 'plan-move-after')), /Existing.*05:30:00.000Z/)
    }
    if (mode === 'uncertain') assert.match(JSON.stringify(tree), /还未核实/);
    await byId(tree, 'plan-apply').props.onClick()
    assert.equal(applied, mode === 'good' ? 1 : 0)
  }
})

test('move review offers retained edit only before attempt or definitive rejection; uncertainty never resends', async () => {
  for (const code of ['REVISION_CONFLICT', undefined]) {
    const f = harness()
    let applications = 0, edits = 0, uncertain = 0
    const props = { proposal: proposalFor(expectedOp), snapshotRevision: 5,
      onEditMove: () => { edits++ }, onUncertainProposal: () => { uncertain++ },
      onApprove: async () => { applications++; throw Object.assign(new Error('private transport'), { code }) } }
    const tree = f.draw('PlanModal', props)
    byId(tree, 'plan-edit-move').props.onClick()
    assert.equal(edits, 1)
    await byId(tree, 'plan-apply').props.onClick()
    const after = f.draw('PlanModal', props)
    const edit = byId(after, 'plan-edit-move')
    assert.equal(Boolean(edit), Boolean(code))
    edit?.props.onClick()
    await byId(after, 'plan-apply').props.onClick()
    assert.equal(edits, code ? 2 : 1)
    assert.equal(applications, 1)
    assert.equal(uncertain, code ? 0 : 1)
  }
})

test('guarded move settlement HTTP 401 clears session but does not send another RPC', async () => {
  const rpc = part('    async function bridgeRpc', '    async function query')
  for (const retry of [false, true]) {
    let requests = 0
    const bridge = vm.runInNewContext(`let sessionPromise, uiSession, bridgeToken; ${rpc}; bridgeRpc`, {
      BRIDGE: '', ensureSession: async () => {}, fetch: async () => {
        requests++
        return requests === 1
          ? { status: 401, ok: false, json: async () => ({ error: { code: 'UI_SESSION_REQUIRED' } }) }
          : { status: 200, ok: true, json: async () => ({ ok: true, result: 'definitive' }) }
      },
    })
    const request = bridge('command', { operation: 'changes.commit' }, retry)
    if (retry) assert.equal(await request, 'definitive')
    else await assert.rejects(request, { code: 'UI_SESSION_REQUIRED' })
    assert.equal(requests, retry ? 2 : 1)
  }
})
