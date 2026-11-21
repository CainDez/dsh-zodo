import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import {test} from 'node:test'
import vm from 'node:vm'
const source=readFileSync(new URL('../../adapters/deepseek-harness/plugin/client.js',import.meta.url),'utf8')
const zones=source.slice(source.indexOf('    function workspaceTimezone'),source.indexOf('    function truncateText'))
const dateCode=source.slice(source.indexOf('    function agendaDate'),source.indexOf('    function agendaShift'))
const code=source.slice(source.indexOf('    function scheduleRecovery'),source.indexOf('    function CapacityEditor'))
function mount({form={},send,guidedDraft,zone='Asia/Shanghai',capacityStart='2026-11-24T00:00Z',capacityEnd='2026-11-24T12:00Z'}={}) {
  const writes=[],errors=[],closed=[],modals=[]
  const Component=vm.runInNewContext(`${zones}\n${dateCode}\n${code}\nQuickSchedule`,{
    Modal:'Modal',Button:'Button',Field:'Field',React:{Fragment:'Fragment'},COPY:{cancel:'取消'},
    h:(tag,props,...children)=>({tag,props,children}),useRef:value=>({current:value}),
    useState:initial=>[typeof initial==='object'?{...initial,task:'t',capacity:'c',...form}:initial,value=>errors.push(value)],
  })
  const tree=Component({day:'2026-11-24',guidedDraft,snapshot:{workspace:{timezone:zone},tasks:[{id:'t',version:3,title:'Task'}],capacity_blocks:[{id:'c',title:'Capacity',start:capacityStart,end:capacityEnd}]},busy:false,
    send:async(...args)=>{writes.push(args);return send?send(...args):{data:{proposal:{id:'p',status:'validated',intent:'Core candidate'}}}},onClose:()=>closed.push(true),setModal:value=>modals.push(value)})
  return {tree,writes,errors,closed,modals,save:tree.props.footer.children[1].props.onClick}
}
test('open/cancel read-only; preview sends only strict proposal and returns to normal approval',async()=>{
  const f=mount()
  assert.equal(f.writes.length,0)
  f.tree.props.onClose()
  assert.equal(f.writes.length,0)
  await f.save()
  assert.equal(f.writes.length,1)
  const [operation,payload]=f.writes[0]
  assert.equal(operation,'proposal.create')
  assert.equal(payload.operations.length,1)
  assert.equal(payload.operations[0].operation,'allocation.schedule')
  const allocation=payload.operations[0].payload.allocation
  assert.equal(allocation.expected_task_version,3)
  assert.equal(allocation.start,'2026-11-24T01:00:00.000Z')
  assert.equal(allocation.end,'2026-11-24T02:00:00.000Z')
  assert.deepEqual(Object.keys(allocation).sort(),['capacity_block_id','end','expected_task_version','start','task_id'])
  assert.equal(f.modals[0].type,'plan')
  assert.equal(f.modals[0].grant,null)
})
test('unsupported Core visibly fails closed without manual fallback; retry is explicit',async()=>{
  const f=mount({send:async()=>{throw new Error('unknown operation allocation.schedule')}})
  await f.save()
  assert.equal(f.writes.length,1)
  assert.equal(f.modals.length,0)
  assert.ok(f.errors.at(-1).includes('不会使用宽松接口'))
  await f.save()
  assert.equal(f.writes.length,2)
  assert.ok(f.writes.every(([op])=>op==='proposal.create'))
})
test('double preview and pending close guarded; invalid candidate never gets approval screen',async()=>{
  let resolve
  const f=mount({send:()=>new Promise(done=>{resolve=done})})
  const pending=f.save()
  await f.save()
  f.tree.props.onClose()
  f.tree.props.footer.children[0].props.onClick()
  assert.equal(f.writes.length,1)
  assert.equal(f.closed.length,0)
  resolve({data:{proposal:{status:'draft'}}})
  await pending
  assert.equal(f.modals.length,0)
  assert.ok(f.errors.at(-1).includes('未返回有效'))
})
test('missing capacity, DST missing/repeated clock, bad zone and reversed interval do not propose',async()=>{
  for(const options of [{form:{capacity:''}},{zone:'bad-zone'},
    {zone:'America/New_York',form:{start:'2026-03-08T02:30',end:'2026-03-08T03:30'}},
    {zone:'America/New_York',form:{start:'2026-11-01T01:30',end:'2026-11-01T02:30'}},
    {form:{start:'2026-11-24T11:00',end:'2026-11-24T10:00'}},
  ]) {const f=mount(options);await f.save();assert.equal(f.writes.length,0);assert.ok(f.errors.at(-1))}
  const f=mount({zone:'America/New_York',capacityStart:'2026-11-01T00:00Z',capacityEnd:'2026-11-02T00:00Z',form:{start:'2026-11-01T01:30',end:'2026-11-01T02:30',startChoice:Date.parse('2026-11-01T06:30Z')}})
  await f.save()
  assert.equal(f.writes[0][1].operations[0].payload.allocation.start,'2026-11-01T06:30:00.000Z')
})

test('global receipt notice distinguishes proposal metadata from saved facts',()=>{
  const line=source.split('\n').find(line=>line.includes("setNotice(`${['proposal.create', 'plan.propose']"))
  for(const operation of ['proposal.create','plan.propose','changes.commit','task.update']) {
    const notices=[]
    vm.runInNewContext(line,{operation,result:{revision:7,operation_id:'fixture-operation'},setNotice:value=>notices.push(value)})
    assert.ok(notices[0].startsWith(operation==='proposal.create'||operation==='plan.propose'?'提案已记录 · 尚未应用':'已保存'))
  }
})

test('draft diagnostics produce actionable hints without reflecting arbitrary private values',async()=>{
  const f=mount({send:async()=>({data:{proposal:{status:'draft',diagnostics:[
    {code:'VALIDATION_FAILED',message:'allocation overlaps fixed commitment PRIVATE_SENTINEL'},
    {code:'REVISION_CONFLICT',message:'task changed PRIVATE_SENTINEL'},
  ]}}})})
  await f.save()
  const error=f.errors.at(-1)
  assert.ok(error.includes('改选时段'))
  assert.ok(error.includes('重新生成预览'))
  assert.ok(error.includes('输入已保留'))
  assert.equal(error.includes('PRIVATE_SENTINEL'),false)
  assert.equal(f.modals.length,0)
})

test('recovery categories distinguish correcting constraints from blind retry',()=>{
  const recovery=vm.runInNewContext(`${code}\nscheduleRecovery`)
  for(const [failure,hint] of [
    [{code:'NOT_FOUND'},'重新选择'],[{code:'PERMISSION_DENIED'},'权限'],
    [{message:'scheduled interval must start in the future'},'未来时段'],
    [{message:'predecessor work is not fully scheduled before this interval'},'前置工作'],
    [{message:'only an unblocked leaf task can be scheduled'},'叶任务'],
    [{message:'scheduled interval violates task time constraints'},'硬截止'],
    [{message:'scheduled interval is outside workspace availability'},'工作窗口'],
    [{message:'capacity block is already fully consumed'},'剩余预算'],
    [{message:'allocation would exceed remaining work'},'剩余分钟'],
    [{code:'STORAGE_FAILURE',message:'PRIVATE_SENTINEL'},'提案记录'],
  ]) assert.ok(recovery(failure).includes(hint),hint)
})

test('in-flight form events cannot change the retained draft, then unlock after failure',async()=>{
  let reject
  const f=mount({send:()=>new Promise((_,fail)=>{reject=fail})})
  const find=(node,id)=>node?.props?.['data-testid']===id?node:
    (node?.children??[]).flat(Infinity).map(child=>find(child,id)).find(Boolean)
  const picker=find(f.tree,'schedule-slot-task')
  const pending=f.save()
  const calls=f.errors.length
  picker.props.onChange({target:{value:'different-task'}})
  assert.equal(f.errors.length,calls,'no state setter invoked before busy re-render')
  reject(new Error('network unavailable'))
  await pending
  picker.props.onChange({target:{value:'different-task'}})
  assert.equal(f.errors.at(-1).task,'different-task','editing resumes only after request settles')
})

test('global send hides strict-preview raw failures but preserves refresh and other command behavior',async()=>{
  const begin=source.indexOf('      const send = useCallback(async (operation, payload, guardedMoveSettlement')
  const end=source.indexOf('      const reader',begin)
  for(const [operation,payload,code,expectedRefresh,rawAllowed,guardedMoveSettlement] of [
    ['proposal.create',{operations:[{operation:'allocation.schedule'}]},'VALIDATION_FAILED',0,false],
    ['proposal.create',{operations:[{operation:'allocation.schedule'}]},'REVISION_CONFLICT',1,false],
    ['proposal.create',{operations:[{operation:'allocation.move'}]},'VALIDATION_FAILED',0,false],
    ['proposal.create',{operations:[{operation:'allocation.move'}]},'REVISION_CONFLICT',1,false],
    ['changes.commit',{proposal_id:'move'},'STORAGE_FAILURE',0,false,true],
    ['proposal.approve',{proposal_id:'move'},'STORAGE_FAILURE',0,false,true],
    ['task.update',{},'VALIDATION_FAILED',0,true],
    ['proposal.create',{operations:[{operation:'task.update'}]},'VALIDATION_FAILED',0,true],
  ]) {
    const errors=[],busy=[],retries=[];let refreshed=0
    const failure=Object.assign(new Error('PRIVATE_SENTINEL'),{code})
    const send=vm.runInNewContext(`${source.slice(begin,end)}\nsend`,{
      useCallback:fn=>fn,snapshot:{workspace:{revision:5}},setBusy:value=>busy.push(value),setError:value=>errors.push(value),setNotice:()=>{},
      command:async(_op,_payload,_version,_key,retrySession)=>{retries.push(retrySession);throw failure},refresh:async()=>{refreshed++},
    })
    await assert.rejects(send(operation,payload,guardedMoveSettlement),error=>error===failure)
    assert.deepEqual(retries,[!guardedMoveSettlement])
    assert.equal(errors.join(' ').includes('PRIVATE_SENTINEL'),rawAllowed)
    assert.equal(refreshed,expectedRefresh)
    assert.deepEqual(busy,[true,false])
  }
})

test('picker uses workspace civil-day overlap, keeps cross-midnight capacities and only open unblocked leaves',()=>{
  const options=vm.runInNewContext(`${zones}\n${dateCode}\n${code}\nschedulePickerOptions`)
  const snapshot={tasks:[{id:'parent'},{id:'child',parent_id:'parent'},{id:'blocked',blocked_manual:true},{id:'done',status:'done'},{id:'archived',archived:true}],capacity_blocks:[
    {id:'cross',start:'2026-11-23T15:30Z',end:'2026-11-23T17:00Z'},
    {id:'ends',start:'2026-11-23T14:00Z',end:'2026-11-23T16:00Z'},
    {id:'future',start:'2026-11-25T01:00Z',end:'2026-11-25T02:00Z'},
  ]}
  const form={start:'2026-11-24T00:15',end:'2026-11-24T00:45'}
  const result=options(snapshot,form,'Asia/Shanghai')
  assert.deepEqual(Array.from(result.tasks,x=>x.id),['child'])
  assert.deepEqual(Array.from(result.capacities,x=>x.id),['cross'])
  assert.equal(options(snapshot,{...form,start:''},'Asia/Shanghai').capacities.length,3)
  assert.equal(options(snapshot,{...form,end:'2026-11-25T10:00'},'Asia/Shanghai').capacities.length,2)
})
test('date changes clear only a definitively non-overlapping capacity; incomplete date retains draft',()=>{
  const f=mount()
  const findInputs=node=>[...(node?.tag==='input'&&node.props.type==='datetime-local'?[node]:[]),...(node?.children??[]).flat(Infinity).flatMap(child=>findInputs(child))]
  const [start,end]=findInputs(f.tree)
  start.props.onChange({target:{value:''}})
  assert.equal(f.errors.at(-1).capacity,'c')
  // Move end first (overlap still exists), then simulate a form with both dates
  // moved by mounting the earlier field state, as React would rerender it.
  end.props.onChange({target:{value:'2026-11-25T10:00'}})
  assert.equal(f.errors.at(-1).capacity,'c')
  const later=mount({form:{end:'2026-11-25T10:00'}})
  findInputs(later.tree)[0].props.onChange({target:{value:'2026-11-25T09:00'}})
  assert.equal(later.errors.at(-1).capacity,'')
  assert.equal(later.writes.length,0)
})

test('first schedule guide advances through prerequisites and never treats history as first run', () => {
  const pick = vm.runInNewContext(`${zones}\n${dateCode}\n${code}\nfirstScheduleStep`)
  const now = Date.parse('2026-11-23T15:55Z')
  const snapshot = { tasks: [], capacity_blocks: [], allocations: [] }
  const run = zone => pick(snapshot, now, zone ?? 'Asia/Shanghai')
  assert.equal(run().kind, 'task')
  snapshot.tasks = [
    { id: 'parent', estimate_minutes: 60 },
    { id: 'child', parent_id: 'parent' },
    { id: 'done', status: 'done', estimate_minutes: 30 },
    { id: 'blocked', blocked_manual: true, estimate_minutes: 30 },
  ]
  assert.equal(run().kind, 'estimate')
  assert.equal(run().task.id, 'child')
  snapshot.tasks[1].estimate_minutes = 30
  snapshot.capacity_blocks = [
    { start: '2026-11-23T14:00Z', end: '2026-11-23T15:55Z', capacity_minutes: 90 },
    { start: '2026-11-23T15:00Z', end: '2026-11-23T18:00Z', capacity_minutes: 0 },
    { start: '2026-11-23T21:00Z', end: '2026-11-23T19:00Z', capacity_minutes: 90 },
  ]
  assert.equal(run().kind, 'capacity', 'expired, invalid and zero-minute blocks are not usable evidence')
  snapshot.capacity_blocks.push(
    { start: '2026-11-25T09:00Z', end: '2026-11-25T10:00Z', capacity_minutes: 60 },
    { start: '2026-11-23T16:30Z', end: '2026-11-23T17:30Z', capacity_minutes: 60 },
  )
  const before = JSON.stringify(snapshot)
  assert.equal(run().kind, 'schedule')
  assert.equal(run().day, '2026-11-24', 'earliest usable day is in workspace time')
  assert.equal(run('Not/AZone').kind, 'timezone')
  assert.equal(JSON.stringify(snapshot), before, 'guidance is read-only')
  snapshot.allocations.push({ id: 'saved' })
  assert.equal(run(), null)
})

test('first schedule CTA only opens existing editors or settings', () => {
  const start = source.indexOf('      const renderFirstSchedule =')
  const end = source.indexOf('      const renderTaskDetail =', start)
  const clean = (value) => JSON.parse(JSON.stringify(value))
  const task = { id: 'saved-task' }
  const trigger = { test: 'original button' }
  for (const [step, expected] of [
    [{ kind: 'task' }, { type: 'task', task: null, restoreFocus: trigger }],
    [{ kind: 'estimate', task }, { type: 'task', task, restoreFocus: trigger }],
    [{ kind: 'capacity' }, { type: 'capacity' }],
    [{ kind: 'schedule', day: '2026-11-24' }, { type: 'quick-schedule', day: '2026-11-24', guidedDraft: { start: '', end: '', capacity: '' } }],
    [{ kind: 'timezone' }, 'settings-overview'],
  ]) {
    const calls = []
    const tree = vm.runInNewContext(`${source.slice(start, end)}\nrenderFirstSchedule()`, {
      snapshot: {}, todayCheckedAt: 0, workspaceTimezone: () => 'Asia/Shanghai', firstScheduleStep: () => step, guidedScheduleDraft: () => ({ start: '', end: '', capacity: '' }),
      h: (tag, props, ...children) => ({ tag, props, children }), Section: 'Section', Button: 'Button', busy: false,
      setModal: data => calls.push(data), navigate: data => calls.push(data),
    })
    assert.equal(tree.props.testid, 'first-schedule-guide')
    assert.deepEqual(calls, [], 'render is read-only')
    tree.children[1].props.onClick({ currentTarget: trigger })
    assert.deepEqual(clean(calls), [expected])
    if (expected.restoreFocus) assert.equal(calls[0].restoreFocus, trigger)
  }
})

const nextDraft = vm.runInNewContext(`${zones}\n${dateCode}\n${code}\nguidedScheduleDraft`)
const cap = (id, start, end, capacity_minutes = 30) => ({ id, start, end, capacity_minutes })
const guided = (blocks, now, zone, estimate_minutes = 30) => JSON.parse(JSON.stringify(nextDraft({
  tasks: [{ id: 't', estimate_minutes }], capacity_blocks: blocks,
}, Date.parse(now), zone)))
const blankDraft = { capacity: '', start: '', end: '' }
test('guided draft rounds inward, supports narrow late-night capacity and uses strict future minutes', () => {
  assert.deepEqual(guided([
    cap('sub-minute', '2026-11-23T15:54:50Z', '2026-11-23T15:55:25Z'),
    cap('late', '2026-11-23T15:57:30Z', '2026-11-23T16:01:12Z'),
  ], '2026-11-23T15:58:00Z', 'Asia/Shanghai'), {
    capacity: 'late', start: '2026-11-23T23:59', end: '2026-11-24T00:01',
  })
  assert.deepEqual(guided([cap('too-short', '2026-11-23T15:57:30Z', '2026-11-23T15:58:59Z')],
    '2026-11-23T15:00:00Z', 'Asia/Shanghai'), blankDraft)
  assert.deepEqual(guided([cap('budget', '2026-11-23T15:30:00Z', '2026-11-23T17:00:00Z', 8)],
    '2026-11-23T15:00:00Z', 'Asia/Shanghai', 5), {
    capacity: 'budget', start: '2026-11-23T23:30', end: '2026-11-23T23:35',
  })
})
test('guided draft never guesses across DST gaps or folds', () => {
  const zone = 'America/New_York'
  assert.deepEqual(guided([cap('gap', '2026-03-08T06:50:00Z', '2026-03-08T07:20:00Z')],
    '2026-03-08T06:00:00Z', zone), {
    capacity: 'gap', start: '2026-03-08T01:50', end: '2026-03-08T01:51',
  })
  assert.deepEqual(guided([cap('fold', '2026-11-01T05:30:00Z', '2026-11-01T06:45:00Z')],
    '2026-11-01T05:00:00Z', zone), blankDraft)
  assert.deepEqual(guided([
    cap('fold', '2026-11-01T05:30:00Z', '2026-11-01T06:45:00Z'),
    cap('safe', '2026-11-01T07:30:00Z', '2026-11-01T08:00:00Z'),
  ], '2026-11-01T05:00:00Z', zone), {
    capacity: 'safe', start: '2026-11-01T02:30', end: '2026-11-01T03:00',
  })
})
test('guided dialog can intentionally leave times blank while standard date defaults stay unchanged', () => {
  const findInputs = node => [
    ...(node?.tag === 'input' && node.props.type === 'datetime-local' ? [node.props.value] : []),
    ...(node?.children ?? []).flat(Infinity).flatMap(findInputs),
  ]
  assert.deepEqual(findInputs(mount().tree), ['2026-11-24T09:00', '2026-11-24T10:00'])
  const guidedForm = mount({ guidedDraft: blankDraft })
  assert.deepEqual(findInputs(guidedForm.tree), ['', ''])
  assert.equal(guidedForm.writes.length, 0)
  assert.deepEqual(findInputs(mount({
    guidedDraft: { capacity: 'c', start: '2026-11-24T14:00', end: '2026-11-24T14:20' },
  }).tree), ['2026-11-24T14:00', '2026-11-24T14:20'])
})

test('guided task editor restores the explicit trigger after child autofocus and dialog removal', () => {
  const start = source.indexOf('    function Modal(')
  const end = source.indexOf('    function Section(', start)
  const effects = []
  const timers = []
  const focused = []
  class Element {
    constructor(name) { this.name = name; this.attached = true }
    focus() { focused.push(this.name) }
  }
  const trigger = new Element('guide')
  const pointer = new Element('navigation')
  const autofocus = new Element('dialog title field')
  const Modal = vm.runInNewContext(`${source.slice(start, end)}\nModal`, {
    COPY: { close: '关闭' }, h: () => ({}), useRef: current => ({ current }),
    useEffect: effect => effects.push(effect), HTMLElement: Element,
    document: { activeElement: autofocus, contains: el => el.attached },
    lastFocusedElement: { current: autofocus }, lastPointerButton: { current: pointer },
    setTimeout: callback => timers.push(callback),
  })
  Modal({ title: '新建任务', restoreFocus: trigger })
  const cleanup = effects[0]()
  autofocus.attached = false
  cleanup()
  assert.deepEqual(focused, [], 'restoration waits for removal')
  assert.equal(timers.length, 1)
  timers[0]()
  assert.deepEqual(focused, ['guide'])
})

test('time details protect only ineligible allocations and recheck the clock before removal', async () => {
  const start = source.indexOf('    function allocationRemovalReason')
  const end = source.indexOf('    function DuplicateSuggestions', start)
  let clock = +new Date('2026-11-20T00:00:00Z')
  const writes = [], alerts = []
  const TimeDetail = vm.runInNewContext(`${source.slice(start, end)}\nTimeDetail`, {
    Modal: 'Modal', Button: 'Button', React: { Fragment: 'Fragment' }, COPY: { close: '关闭' },
    h: (tag, props, ...children) => ({ tag, props, children }),
    useState: value => [value, next => alerts.push(next)],
    Date: { now: () => clock, parse: Date.parse }, taskTitle: () => 'Task', dateTimeFor: s => s,
    workspaceTimezone: () => 'UTC', manualScheduleTasks: snapshot => snapshot.tasks ?? [],
  })
  const primary = { id: 'same', state: 'planned', locked: false, start: '2026-11-20T00:01:00Z' }
  const button = (kind, overrides = {}) => TimeDetail({
    kind, row: { ...primary, ...overrides }, snapshot: {}, onClose: () => {}, busy: false,
    send: async operation => writes.push(operation),
  }).props.footer.children.at(-1)
  for (const override of [
    { locked: true }, { state: 'started' }, { state: 'done' },
    { start: '2026-11-19T23:59:00Z' }, { start: '2026-11-20T00:00:00Z' }, { start: 'invalid' },
  ]) assert.equal(button('allocation', override).props.disabled, true)
  const remove = button('allocation')
  assert.equal(remove.props.disabled, false)
  await remove.props.onClick()
  clock = +new Date('2026-11-20T00:01:00Z')
  await remove.props.onClick()
  assert.match(alerts.at(-1), /已到开始时间/)
  for (const kind of ['commitment', 'capacity']) {
    const control = button(kind)
    assert.equal(control.props.disabled, false)
    await control.props.onClick()
  }
  assert.deepEqual(writes, ['allocation.remove', 'commitment.remove', 'capacity.remove'])
})
