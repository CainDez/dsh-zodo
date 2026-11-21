import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import {test} from 'node:test'
import vm from 'node:vm'
const source = readFileSync(new URL('../../adapters/deepseek-harness/plugin/client.js', import.meta.url), 'utf8')
const zones = source.slice(source.indexOf('    function workspaceTimezone'), source.indexOf('    function truncateText'))
const editor = source.slice(source.indexOf('    // BEGIN APPOINTMENT DEFAULTS'), source.indexOf('    function CapacityEditor'))
const helper = vm.runInNewContext(`${zones}\n${editor}\n({appointmentDefaults})`)

test('selected display-zone day converts to workspace clocks without changing instants', () => {
  const form = helper.appointmentDefaults('2026-11-18', 'America/New_York', 'Asia/Shanghai')
  assert.equal(form.start, '2026-11-17T20:00')
  assert.equal(form.end, '2026-11-17T21:00')
})

test('real editor open/cancel never writes; explicit save uses existing Core command once', async () => {
  const writes = [], closed = [], errors = []
  const context = {
    Intl, Date, Modal:'Modal', Field:'Field', Button:'Button', React:{Fragment:'Fragment'}, COPY:{cancel:'取消',save:'保存'},
    h:(tag,props,...children)=>({tag,props,children}),
    useRef: value => ({current:value}),
    useState: initial => [typeof initial === 'function' ? initial() : initial, value => errors.push(value)],
  }
  const Component = vm.runInNewContext(`${zones}\n${editor}\nCommitmentEditor`, context)
  const tree = Component({day:'2026-11-18', snapshot:{workspace:{timezone:'UTC'}}, busy:false,
    send:async (...args)=>writes.push(args), onClose:()=>closed.push(true)})
  assert.equal(writes.length,0)
  const [cancel,save] = tree.props.footer.children
  cancel.props.onClick()
  assert.equal(writes.length,0)
  await save.props.onClick()
  assert.equal(writes.length,1)
  assert.equal(writes[0][0],'commitment.create')
  assert.equal(writes[0][1].commitment.locked,true)
  assert.equal(Date.parse(writes[0][1].commitment.end)-Date.parse(writes[0][1].commitment.start),3600000)
  assert.equal(errors.length,0)
  assert.equal(closed.length,2)
})

test('invalid zone cannot save; column action only opens prefilled editor', async () => {
  const writes=[], errors=[]
  const Component=vm.runInNewContext(`${zones}\n${editor}\nCommitmentEditor`, {
    Intl, Date, Modal:'Modal', Field:'Field', Button:'Button', React:{Fragment:'Fragment'}, COPY:{cancel:'取消',save:'保存'},
    h:(tag,props,...children)=>({tag,props,children}),
    useRef: value => ({current:value}),
    useState:initial=>[typeof initial==='function'?initial():initial,value=>errors.push(value)],
  })
  const tree=Component({day:'2026-11-18',snapshot:{workspace:{timezone:'bad-zone'}},send:async (...args)=>writes.push(args),onClose:()=>{},busy:false})
  await tree.props.footer.children[1].props.onClick()
  assert.equal(writes.length,0)
  assert.ok(errors[0].includes('IANA'))
  const line=source.split('\n').find(line=>line.includes('testid: `schedule-add-appointment-${day}`'))
  const opened=[]
  const action=vm.runInNewContext(line.trim().replace(/,$/,''),{h:(tag,props)=>props,Button:'Button',day:'2026-11-18',busy:false,setModal:value=>opened.push(value)})
  assert.equal(opened.length,0)
  action.onClick()
  assert.equal(opened[0].day,'2026-11-18')
  assert.equal(opened[0].type,'commitment')
})

test('workspace missing/repeated DST clocks and reverse intervals never silently save', async () => {
  for (const form of [
    {start:'2026-03-08T02:30',end:'2026-03-08T03:30'},
    {start:'2026-11-01T01:30',end:'2026-11-01T02:30'},
    {start:'2026-11-18T10:00',end:'2026-11-18T09:00'},
  ]) {
    const writes=[],errors=[]
    const Component=vm.runInNewContext(`${zones}\n${editor}\nCommitmentEditor`, {
      Intl, Date, Modal:'Modal', Field:'Field', Button:'Button', React:{Fragment:'Fragment'}, COPY:{cancel:'取消',save:'保存'},
      h:(tag,props,...children)=>({tag,props,children}),
      useRef: value => ({current:value}),
    useState:initial=>[typeof initial==='function'?{title:'Fixture',locked:true,...form}:initial,value=>errors.push(value)],
    })
    const tree=Component({snapshot:{workspace:{timezone:'America/New_York'}},send:async (...args)=>writes.push(args),onClose:()=>{},busy:false})
    await tree.props.footer.children[1].props.onClick()
    assert.equal(writes.length,0)
    assert.equal(errors.length,1)
  }
})

test('concurrent save is synchronously guarded, pending close ignored, failure permits retry', async () => {
  let release
  const writes=[],closed=[],errors=[]
  const Component=vm.runInNewContext(`${zones}\n${editor}\nCommitmentEditor`, {
    Intl, Date, Modal:'Modal', Field:'Field', Button:'Button', React:{Fragment:'Fragment'}, COPY:{cancel:'取消',save:'保存'},
    h:(tag,props,...children)=>({tag,props,children}), useRef:value=>({current:value}),
    useState:initial=>[typeof initial==='function'?initial():initial,value=>errors.push(value)],
  })
  const tree=Component({day:'2026-11-18',snapshot:{workspace:{timezone:'UTC'}},busy:false,
    send:async (...args)=>{writes.push(args); await new Promise((resolve,reject)=>{release={resolve,reject}})},onClose:()=>closed.push(true)})
  const [cancel,save]=tree.props.footer.children
  const first=save.props.onClick()
  await save.props.onClick()
  assert.equal(writes.length,1)
  cancel.props.onClick()
  tree.props.onClose()
  assert.equal(closed.length,0)
  release.reject(new Error('retryable fixture error'))
  await first
  assert.equal(errors.length,1)
  const retry=save.props.onClick()
  assert.equal(writes.length,2)
  release.resolve()
  await retry
  assert.equal(closed.length,1)
})
