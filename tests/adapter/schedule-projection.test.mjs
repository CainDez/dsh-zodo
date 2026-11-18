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
const api = vm.runInNewContext(`${source.slice(start, end)}\n({ agendaDays, agendaShift, agendaEntries, calendarFreshness })`)
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
