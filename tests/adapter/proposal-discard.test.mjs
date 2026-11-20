import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import vm from 'node:vm'

const source = readFileSync(new URL('../../adapters/deepseek-harness/plugin/client.js', import.meta.url), 'utf8')
const code = source.slice(source.indexOf('    function canDiscardProposal'), source.indexOf('    function PlanModal'))
const h = (tag, props, ...children) => ({ tag, props, children })
const { canDiscardProposal, DiscardProposal } = vm.runInNewContext(code + '\n({ canDiscardProposal, DiscardProposal })', {
  h, Modal: 'modal', Button: 'button', React: { Fragment: 'fragment' }, COPY: { cancel: '取消' },
  useRef: current => ({ current }), useState: value => [value, () => {}],
})
const proposal = { id: 'synthetic', intent: 'Disposable preview', status: 'validated' }

test('only known validated proposals can be explicitly discarded', () => {
  assert.equal(canDiscardProposal(proposal, false), true)
  assert.equal(canDiscardProposal(proposal, true), false)
  for (const status of ['draft', 'applied', 'rejected', 'stale', 'expired']) {
    assert.equal(canDiscardProposal({ ...proposal, status }, false), false)
  }
  assert.equal(canDiscardProposal(undefined, false), false)
})

test('closing the confirmation does not send a command; confirm is gated while pending', async () => {
  let finish
  const gate = new Promise(resolve => { finish = resolve })
  const writes = []
  let closed = 0
  const props = { proposal, available: true, busy: false, onClose: () => { closed++ },
    send: async (...args) => { writes.push(args); await gate }, onUncertain: assert.fail }
  const initial = DiscardProposal(props)
  initial.props.footer.children[0].props.onClick()
  assert.equal(closed, 1)
  assert.equal(writes.length, 0)
  const dialog = DiscardProposal(props)
  const confirm = dialog.props.footer.children[1].props.onClick
  const first = confirm()
  dialog.props.onClose()
  await confirm()
  assert.equal(closed, 1)
  assert.equal(writes.length, 1)
  assert.equal(writes[0][0], 'proposal.reject')
  assert.equal(writes[0][1].proposal_id, proposal.id)
  finish()
  await first
  assert.equal(closed, 2)
})

test('failed confirmations require status verification before trying again', async () => {
  let writes = 0
  let unknown = 0
  const dialog = DiscardProposal({ proposal, available: true, busy: false, onClose: assert.fail,
    send: async () => { writes++; throw Error('unknown outcome') }, onUncertain: () => { unknown++ } })
  const confirm = dialog.props.footer.children[1].props.onClick
  await confirm()
  await confirm()
  assert.equal(writes, 1)
  assert.equal(unknown, 1)
})
