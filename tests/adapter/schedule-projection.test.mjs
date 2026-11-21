import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import vm from 'node:vm'

// Exercise the exact browser helper source without mounting a substitute UI or
// importing the host-only module loader. No real DSH or calendar data is accessed.
const source = readFileSync(new URL('../../adapters/deepseek-harness/plugin/client.js', import.meta.url), 'utf8')
const start = source.indexOf('    // BEGIN PURE AGENDA PROJECTION')
const end = source.indexOf('    // END PURE AGENDA PROJECTION')
assert.ok(start >= 0 && end > start)
const api = vm.runInNewContext(`${source.slice(start, end)}\n({ agendaDays, agendaShift, agendaEntries, calendarFreshness, todayNextAction, outstandingWork })`)
const row = (id, start, end) => ({ id, start, end })
const ids = entries => Array.from(entries, entry => `${entry.kind}:${entry.row.id}`)
function inZone(zone, run) {
  const previous = process.env.TZ
  process.env.TZ = zone
  try { run() } finally {
    if (previous === undefined) delete process.env.TZ
    else process.env.TZ = previous
  }
}

test('seven-day navigation crosses months/leap years and clamps at representable window bounds', () => {
  assert.equal(api.agendaShift('2024-02-27', 7), '2024-03-05')
  assert.equal(api.agendaShift('2024-03-05', -7), '2024-02-27')
  assert.equal(api.agendaShift('0001-01-01', -7), '0001-01-01')
  assert.equal(api.agendaShift('9999-12-24', 7), '9999-12-25')
  assert.equal(api.agendaShift('bad', 7), '0001-01-01')
  assert.equal(api.agendaShift('2024-02-30', 7), '0001-01-01')
  assert.equal(api.agendaShift('2024-02-27', Infinity), '0001-01-01')
  assert.deepEqual(Array.from(api.agendaDays('9999-12-25')), ['9999-12-25', '9999-12-26', '9999-12-27', '9999-12-28', '9999-12-29', '9999-12-30', '9999-12-31'])
})

test('local date membership uses browser timezone, not UTC date text', () => inZone('Asia/Shanghai', () => {
  const snapshot = { commitments: [row('offset', '2026-11-17T18:00:00Z', '2026-11-17T19:00:00Z')] }
  assert.equal(api.agendaEntries(snapshot, '2026-11-17').length, 0)
  assert.deepEqual(ids(api.agendaEntries(snapshot, '2026-11-18')), ['commitment:offset'])
}))

test('all types share chronological instant ordering despite different UTC offsets', () => inZone('UTC', () => {
  const snapshot = {
    capacity_blocks: [row('late', '2026-11-18T08:00:00-05:00', '2026-11-18T09:00:00-05:00')],
    commitments: [row('early', '2026-11-18T16:00:00+08:00', '2026-11-18T17:00:00+08:00')],
    allocations: [row('middle', '2026-11-18T10:00:00Z', '2026-11-18T11:00:00Z')],
  }
  const before = JSON.stringify(snapshot)
  assert.deepEqual(ids(api.agendaEntries(snapshot, '2026-11-18')), ['commitment:early', 'allocation:middle', 'capacity:late'])
  assert.equal(JSON.stringify(snapshot), before, 'projection never mutates the Core snapshot')
}))

test('cross-midnight rows overlap both days; exact midnight endings do not leak into next day', () => inZone('Asia/Shanghai', () => {
  const snapshot = { allocations: [row('cross', '2026-11-18T23:30:00+08:00', '2026-11-19T01:00:00+08:00'), row('midnight', '2026-11-18T23:00:00+08:00', '2026-11-19T00:00:00+08:00')] }
  const first = api.agendaEntries(snapshot, '2026-11-18')
  assert.deepEqual(ids(first), ['allocation:midnight', 'allocation:cross'])
  assert.equal(first[0].endsAtDayBoundary, true)
  assert.equal(first[0].continuesAfter, false)
  assert.equal(first[1].continuesAfter, true)
  const next = api.agendaEntries(snapshot, '2026-11-19')
  assert.deepEqual(ids(next), ['allocation:cross'])
  assert.equal(next[0].continuesBefore, true)
  assert.equal(next[0].visibleStart, Date.parse('2026-11-19T00:00:00+08:00'))
  assert.equal(next[0].row, snapshot.allocations[0], 'details keep the complete original row')
}))

for (const [day, next, hours] of [['2026-03-08', '2026-03-09', 23], ['2026-11-01', '2026-11-02', 25]]) {
  test(`DST ${hours}-hour day uses consecutive local midnights`, () => inZone('America/New_York', () => {
    const snapshot = { commitments: [row('long', `${day}T00:00:00`, `${next}T00:00:00`)] }
    const [entry] = api.agendaEntries(snapshot, day)
    assert.equal((entry.visibleEnd - entry.visibleStart) / 3600000, hours)
    assert.equal(api.agendaEntries(snapshot, next).length, 0)
    assert.equal(api.agendaShift(day, 7), day === '2026-03-08' ? '2026-03-15' : '2026-11-08')
  }))
}

test('fall-back repeated clock hour remains in actual chronological order', () => inZone('America/New_York', () => {
  const snapshot = { commitments: [row('second', '2026-11-01T01:15:00-05:00', '2026-11-01T01:45:00-05:00'), row('first', '2026-11-01T01:30:00-04:00', '2026-11-01T01:45:00-04:00')] }
  assert.deepEqual(ids(api.agendaEntries(snapshot, '2026-11-01')), ['commitment:first', 'commitment:second'])
}))

test('invalid, reversed and zero-length intervals are omitted, imported calendar content stays excluded', () => inZone('UTC', () => {
  const snapshot = { commitments: [row('invalid', 'oops', 'oops'), row('zero', '2026-11-18T10:00Z', '2026-11-18T10:00Z'), row('reverse', '2026-11-18T11:00Z', '2026-11-18T10:00Z')], calendar_events: [{ ...row('private', '2026-11-18T10:00Z', '2026-11-18T11:00Z'), title: 'PRIVATE_SENTINEL' }] }
  assert.equal(api.agendaEntries(snapshot, '2026-11-18').length, 0)
  assert.equal(api.agendaEntries(snapshot, '2026-02-30').length, 0)
  assert.equal(api.agendaEntries({}, '2026-11-18').length, 0)
}))

test('navigation, unified detail wiring and existing action hooks are retained', () => {
  for (const hook of ['schedule-previous', 'schedule-today', 'schedule-next', 'schedule-range', 'schedule-time', 'capacity-suggest', 'meeting-propose']) assert.ok(source.includes(`'${hook}'`), hook)
  assert.ok(source.includes("setModal({ type: 'time-detail', kind, row })"))
  assert.ok(source.includes('`time-${kind}-${row.id}`'))
})

test('busy projection mirrors Core, strips private fields, retains stale/orphan busy and never mutates input', () => inZone('UTC', () => {
  const secret = 'PRIVATE_SENTINEL'
  const snapshot = { calendar_events: [
    { ...row(secret, '2026-11-18T09:00Z', '2026-11-18T10:00Z'), busy: true, title: secret, notes: secret, source_id: secret, external_id: secret },
    { ...row('free', '2026-11-18T08:00Z', '2026-11-18T09:00Z'), busy: false },
    { ...row('missing', '2026-11-18T08:00Z', '2026-11-18T09:00Z') },
  ], commitments: [row('local', '2026-11-18T08:00Z', '2026-11-18T08:30Z')] }
  const before = JSON.stringify(snapshot)
  const result = api.agendaEntries(snapshot, '2026-11-18')
  assert.deepEqual(ids(result), ['commitment:local', 'busy:busy-0'])
  assert.deepEqual(Object.keys(result[1].row).sort(), ['end', 'id', 'start'])
  assert.equal(JSON.stringify(result).includes(secret), false)
  assert.equal(JSON.stringify(snapshot), before)
  assert.equal(api.agendaEntries(snapshot, '2026-11-19').length, 0)
}))

test('freshness fails closed for missing, invalid and future timestamps, TTL and stale data', () => {
  const now = Date.parse('2026-11-18T12:00Z')
  const valid = { last_sync_at: '2026-11-18T11:00Z', freshness_seconds: 3600 }
  assert.equal(api.calendarFreshness(valid, now), '新鲜')
  assert.equal(api.calendarFreshness(valid, now + 1), '过期')
  for (const source of [{}, {...valid, last_sync_at:'bad'}, {...valid, last_sync_at:'2026-11-19T12:00Z'}, {...valid, freshness_seconds:undefined}, {...valid, freshness_seconds:-1}]) {
    assert.equal(api.calendarFreshness(source, now), '未知')
  }
})

test('Today consumes shared chronological projection; busy slots have no detail handler or identifier attribute', () => {
  assert.ok(source.includes('const todayEntries = agendaEntries(snapshot ?? {}, today)'))
  assert.ok(source.includes('todayEntries.map(entry =>'))
  assert.equal(source.includes('const todayCommitments ='), false)
  const busyStart = source.indexOf("if (kind === 'busy') return h('div'")
  const busyEnd = source.indexOf('const openDetail', busyStart)
  const busyRender = source.slice(busyStart, busyEnd)
  assert.ok(busyRender.includes('只读忙碌快照'))
  for (const excluded of ['onClick', 'onKeyDown', 'tabIndex', 'row.title', 'source_id', 'external_id', 'notes', 'setModal']) assert.equal(busyRender.includes(excluded), false, excluded)
  assert.ok(source.includes('空白不代表可预约'))
})

test('Settings calendar source section executes with populated and missing sources without leaked scope', () => {
  const begin = source.indexOf("h(Section, { title: '时间连接与日历源' }")
  const end = source.indexOf("h(Section, { title: '无模型手工路径' }", begin)
  assert.ok(begin > 0 && end > begin)
  // Execute the production render expression, not a reimplementation. A bare
  // out-of-scope `freshness` reference must throw here, even without a browser.
  const expression = source.slice(begin, end).trim().replace(/,$/, '')
  const render = calendar_sources => vm.runInNewContext(expression, {
    snapshot: { calendar_sources }, Section: 'Section', Empty: 'Empty',
    h: (tag, props, ...children) => ({ tag, props, children }),
    calendarFreshness: api.calendarFreshness,
    dateTimeFor: value => `formatted:${value}`,
  })
  const valid = { id: 'source', provider: 'fixture', display_name: 'Calendar', last_sync_at: '2026-01-01T12:00Z', freshness_seconds: 60, writable: false }
  assert.ok(JSON.stringify(render([valid])).includes('formatted:2026-01-01T12:00Z'))
  assert.ok(JSON.stringify(render([{ ...valid, last_sync_at: undefined }])).includes('同步 未知'))
  assert.ok(JSON.stringify(render([])).includes('尚未连接外部日历'))
})

test('Today chooses saved current/next allocation, not priority or a running focus claim', () => inZone('UTC', () => {
  const now = Date.parse('2026-11-18T10:00Z')
  const tasks = [{id:'a'}, {id:'b'}, {id:'urgent', not_before:'2026-11-20T10:00Z', dependencies:['blocked']}]
  const snapshot = { allocations: [
    {...row('ended', '2026-11-18T08:00Z', '2026-11-18T10:00Z'), task_id:'urgent'},
    {...row('current', '2026-11-18T09:00Z', '2026-11-18T11:00Z'), task_id:'a'},
    {...row('next', '2026-11-18T12:00Z', '2026-11-18T13:00Z'), task_id:'b'},
  ] }
  const entries = api.agendaEntries(snapshot, '2026-11-18')
  const current = api.todayNextAction(entries, tasks, tasks[2], now)
  assert.equal(current.kind, 'scheduled-now')
  assert.equal(current.task.id, 'a')
  assert.equal(current.entry.row.id, 'current')
  const next = api.todayNextAction(entries, tasks, tasks[2], Date.parse('2026-11-18T11:00Z'))
  assert.equal(next.kind, 'scheduled-next')
  assert.equal(next.task.id, 'b')
  // No remaining allocation: deadline shortlist remains explicitly a suggestion,
  // including potentially blocked/future tasks; never infer feasibility.
  const fallback = api.todayNextAction(entries, tasks, tasks[2], Date.parse('2026-11-18T14:00Z'))
  assert.equal(fallback.kind, 'suggestion')
  assert.equal(fallback.task, tasks[2])
  assert.equal(fallback.entry, undefined)
  assert.equal(api.todayNextAction(entries, [], null, now), null)
  assert.equal(api.todayNextAction([], tasks, tasks[2], now).kind, 'suggestion')
}))

test('Today default card opens task; focus is explicit and does not bubble; timestamp and caveat render', () => {
  const begin = source.indexOf("h(Section, { title: '下一步' }")
  const end = source.indexOf("h(Section, { title: '风险与未排入' }", begin)
  const expression = source.slice(begin, end).trim().replace(/,$/, '')
  const opened = [], focused = []
  const task = {id:'task', title:'Fixture', estimate_minutes:15}
  const tree = vm.runInNewContext(expression, {
    Section:'Section', Button:'Button', Empty:'Empty', busy:false,
    todayCheckedAt: Date.parse('2026-11-18T10:00Z'), todayAction:{kind:'suggestion',task},
    dateTimeFor: value => value,
    openTask: id => opened.push(id), focusStart: task => focused.push(task.id),
    h: (tag, props, ...children) => ({tag, props, children}),
  })
  assert.ok(JSON.stringify(tree).includes('2026-11-18T10:00:00.000Z'))
  assert.ok(JSON.stringify(tree).includes('建议不是可执行性判断'))
  const card = tree.children.find(child => child?.props?.['data-testid'] === 'today-next-task')
  card.props.onClick()
  let prevented = false
  const cardElement = {}
  const childButtonElement = {}
  // Native child buttons own Enter/Space; bubbling must not navigate or cancel
  // their browser activation. Only keyboard input on the card itself opens it.
  for (const key of ['Enter', ' ']) {
    card.props.onKeyDown({key, target:childButtonElement, currentTarget:cardElement, preventDefault:()=>{prevented=true}})
  }
  assert.equal(prevented, false)
  assert.deepEqual(opened, ['task'])
  for (const key of ['Enter', ' ']) {
    card.props.onKeyDown({key, target:cardElement, currentTarget:cardElement, preventDefault:()=>{prevented=true}})
  }
  assert.equal(prevented, true)
  assert.deepEqual(opened, ['task','task','task'])
  assert.deepEqual(focused, [])
  const button = card.children.find(child => child?.props?.testid === 'today-focus')
  let stopped = false
  button.props.onClick({stopPropagation:()=>{stopped=true}})
  assert.equal(stopped, true)
  assert.deepEqual(focused, ['task'])
})

test('outstanding leaf work uses full not-ended reservations and one as-of instant', () => {
  const now=Date.parse('2026-11-18T10:00Z')
  const snapshot={tasks:[{id:'parent',remaining_minutes:100},{id:'leaf',parent_id:'parent',remaining_minutes:60},{id:'done',status:'done',remaining_minutes:90}],allocations:[
    {...row('ended','2026-11-18T09:00Z','2026-11-18T10:00Z'),task_id:'leaf',minutes:30},
    {...row('straddling','2026-11-18T09:45Z','2026-11-18T10:15Z'),task_id:'leaf',minutes:30},
  ]}
  const before=JSON.stringify(snapshot)
  const result=api.outstandingWork(snapshot,now)
  assert.equal(result.minutes,30)
  assert.equal(result.rows.length,1)
  assert.equal(result.rows[0].task.id,'leaf')
  assert.equal(result.unknown,false)
  assert.equal(api.outstandingWork(snapshot,Date.parse('2026-11-18T10:15Z')).minutes,60)
  assert.equal(JSON.stringify(snapshot),before)
})

test('invalid or missing outstanding data is unknown, never a fabricated complete total', () => {
  const now=Date.parse('2026-11-18T10:00Z')
  for(const snapshot of [{},{tasks:[],allocations:null},{tasks:[null],allocations:[]},
    {tasks:[{id:'x'}],allocations:[]},
    {tasks:[{id:'x',remaining_minutes:30}],allocations:[{task_id:'x',start:'invalid',end:'invalid',minutes:30}]},
    {tasks:[{id:'x',remaining_minutes:'30'}],allocations:[]},
  ]) assert.equal(api.outstandingWork(snapshot,now).unknown,true)
  const partial=api.outstandingWork({tasks:[{id:'unknown'},{id:'known',estimate_minutes:20}],allocations:[]},now)
  assert.equal(partial.minutes,20)
  assert.equal(partial.unknown,true)
  assert.equal(api.outstandingWork({tasks:[],allocations:[]},NaN).unknown,true)
  assert.ok(source.includes('const outstanding = outstandingWork(snapshot, todayCheckedAt)'))
  assert.ok(source.includes('const unplacedList = outstanding.rows'))
})

test('single-day and seven-day windows preserve bounded civil-date navigation',()=>{
  assert.deepEqual(Array.from(api.agendaDays('2026-11-30',1)),['2026-11-30'])
  assert.equal(api.agendaShift('2026-11-30',1,1),'2026-12-01')
  assert.equal(api.agendaShift('2026-12-01',-1,1),'2026-11-30')
  assert.equal(api.agendaShift('9999-12-31',1,1),'9999-12-31')
  assert.equal(api.agendaShift('0001-01-01',-1,1),'0001-01-01')
  assert.equal(api.agendaShift('9999-12-31',0,7),'9999-12-25')
  assert.equal(api.agendaDays('9999-12-31',1).length,1)
  assert.equal(api.agendaDays('2026-11-30',7).length,7)
})

test('native mode/date controls rerender navigation without commands and retain current date',()=>{
  const begin=source.indexOf("h('div', { className: 'zw-head-actions', role: 'group', 'aria-label': '日程日期导航' }")
  const end=source.indexOf("h('p', { className: 'zw-fineprint' }, '按本机时区显示",begin)
  const expression=source.slice(begin,end).trim().replace(/,$/,'')
  let length=7,start='2026-11-30'
  const render=()=>vm.runInNewContext(expression,{
    ...api,agendaDate:value=>/^\d{4}-\d{2}-\d{2}$/.test(value),AGENDA_MIN:'0001-01-01',AGENDA_MAX:'9999-12-25',
    h:(tag,props,...children)=>({tag,props,children}),Button:'Button',scheduleLength:length,scheduleStart:start,today:'2026-11-18',days:api.agendaDays(start,length),
    setScheduleLength:value=>{length=value},setScheduleStart:value=>{start=typeof value==='function'?value(start):value},
  })
  const find=(node,id)=>node?.props?.['data-testid']===id||node?.props?.testid===id?node:
    (node?.children??[]).flat(Infinity).map(child=>find(child,id)).find(Boolean)
  let tree=render()
  assert.equal(find(tree,'schedule-next').children[0],'后 7 天')
  find(tree,'schedule-mode').props.onChange({target:{value:'1'}})
  assert.equal(start,'2026-11-30')
  tree=render()
  assert.equal(find(tree,'schedule-mode').props['aria-label'],'日程视图')
  assert.equal(find(tree,'schedule-date').props['aria-label'],'日程起始日期')
  assert.equal(find(tree,'schedule-next').children[0],'后一天')
  find(tree,'schedule-next').props.onClick()
  assert.equal(start,'2026-12-01')
  find(render(),'schedule-previous').props.onClick()
  assert.equal(start,'2026-11-30')
  find(render(),'schedule-date').props.onChange({target:{value:'2026-12-15'}})
  assert.equal(start,'2026-12-15')
  find(render(),'schedule-mode').props.onChange({target:{value:'7'}})
  assert.equal(start,'2026-12-15')
  find(render(),'schedule-next').props.onClick()
  assert.equal(start,'2026-12-22')
  find(render(),'schedule-today').props.onClick()
  assert.equal(start,'2026-11-18')
})

test('single-day CSS overrides responsive seven-column rules and date action hooks stay stable',()=>{
  assert.ok(source.includes('.zw-week.zw-single { grid-template-columns:minmax(0,1fr);'))
  assert.ok(source.includes('.zw-single .zw-slot, .zw-single button { max-width:100%; white-space:normal;'))
  for(const hook of ['schedule-day-${day}','schedule-slot-${day}','schedule-task-date-${day}','schedule-add-appointment-${day}','time-${kind}-${row.id}']) assert.ok(source.includes(hook),hook)
  assert.ok(source.includes('const [scheduleLength, setScheduleLength] = useState(7)'))
})
