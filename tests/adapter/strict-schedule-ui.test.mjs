import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import {test} from 'node:test'
import vm from 'node:vm'
const source=readFileSync(new URL('../../adapters/deepseek-harness/plugin/client.js',import.meta.url),'utf8')
const zones=source.slice(source.indexOf('    function workspaceTimezone'),source.indexOf('    function truncateText'))
const code=source.slice(source.indexOf('    function scheduleRecovery'),source.indexOf('    function CapacityEditor'))
function mount({form={},send,zone='Asia/Shanghai'}={}) {
  const writes=[],errors=[],closed=[],modals=[]
  const Component=vm.runInNewContext(`${zones}\n${code}\nQuickSchedule`,{
    Modal:'Modal',Button:'Button',Field:'Field',React:{Fragment:'Fragment'},COPY:{cancel:'取消'},
    h:(tag,props,...children)=>({tag,props,children}),useRef:value=>({current:value}),
    useState:initial=>[typeof initial==='object'?{...initial,task:'t',capacity:'c',...form}:initial,value=>errors.push(value)],
  })
  const tree=Component({day:'2026-11-24',snapshot:{workspace:{timezone:zone},tasks:[{id:'t',version:3,title:'Task'}],capacity_blocks:[{id:'c',title:'Capacity',start:'2026-11-24T00:00Z',end:'2026-11-24T12:00Z'}]},busy:false,
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
  const f=mount({zone:'America/New_York',form:{start:'2026-11-01T01:30',end:'2026-11-01T02:30',startChoice:Date.parse('2026-11-01T06:30Z')}})
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
  const begin=source.indexOf('      const send = useCallback(async (operation, payload)')
  const end=source.indexOf('      const reader',begin)
  for(const [operation,payload,code,expectedRefresh,rawAllowed] of [
    ['proposal.create',{operations:[{operation:'allocation.schedule'}]},'VALIDATION_FAILED',0,false],
    ['proposal.create',{operations:[{operation:'allocation.schedule'}]},'REVISION_CONFLICT',1,false],
    ['task.update',{},'VALIDATION_FAILED',0,true],
    ['proposal.create',{operations:[{operation:'task.update'}]},'VALIDATION_FAILED',0,true],
  ]) {
    const errors=[],busy=[];let refreshed=0
    const failure=Object.assign(new Error('PRIVATE_SENTINEL'),{code})
    const send=vm.runInNewContext(`${source.slice(begin,end)}\nsend`,{
      useCallback:fn=>fn,snapshot:{workspace:{revision:5}},setBusy:value=>busy.push(value),setError:value=>errors.push(value),setNotice:()=>{},
      command:async()=>{throw failure},refresh:async()=>{refreshed++},
    })
    await assert.rejects(send(operation,payload),error=>error===failure)
    assert.equal(errors.join(' ').includes('PRIVATE_SENTINEL'),rawAllowed)
    assert.equal(refreshed,expectedRefresh)
    assert.deepEqual(busy,[true,false])
  }
})
