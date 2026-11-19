import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import {test} from 'node:test'
import vm from 'node:vm'
const source=readFileSync(new URL('../../adapters/deepseek-harness/plugin/client.js',import.meta.url),'utf8')
const moveHelpers=source.slice(source.indexOf('    function allocationMoveOperation'),source.indexOf('    function moveDraftSlot'))
const code=moveHelpers+'\n'+source.slice(source.indexOf('    function PlanModal'),source.indexOf('    function entityFactRows'))
function render(overrides={}) {
  const approvals=[],closed=[],errors=[]
  const Component=vm.runInNewContext(`${code}\nPlanModal`,{
    Modal:'Modal',Section:'Section',Badge:'Badge',Empty:'Empty',Button:'Button',React:{Fragment:'Fragment'},COPY:{approve:'批准'},
    h:(tag,props,...children)=>({tag,props,children}),useRef:value=>({current:value}),useState:value=>[value,message=>errors.push(message)],
    dateTimeFor:value=>value,timeFor:value=>value,taskTitle:(_,id)=>id,stateSnapshotRef:{current:{}},
  })
  const props={proposal:{base_revision:5,preview:{diff:{added:[{kind:'allocation',summary:'new'}],removed:[{kind:'allocation',summary:'old'}]}}},
    snapshotRevision:5,plan:{assignments:[],unplaced:[{task_id:'t',minutes:30,reason:'dependency_blocked'}]},
    onApprove:async()=>approvals.push(true),onClose:()=>closed.push(true),busy:false,...overrides}
  const before=JSON.stringify(props)
  const tree=Component(props)
  assert.equal(JSON.stringify(props),before)
  return {tree,approvals,closed,errors}
}
test('preview summarizes changes and reported gaps without claiming saved or insufficient capacity',async()=>{
  const {tree,approvals,closed}=render()
  const text=JSON.stringify(tree)
  for(const expected of ['将改变什么','什么保持不变','哪些尚未安排','新增 1 项','移除 1 项','30 分钟','dependency_blocked','成功回执后才算已保存','不能据此断定没有容量']) assert.ok(text.includes(expected),expected)
  assert.equal(approvals.length,0)
  tree.props.footer.children[0].props.onClick()
  assert.equal(closed.length,1)
  assert.equal(approvals.length,0)
  await tree.props.footer.children[1].props.onClick()
  assert.equal(approvals.length,1)
})
test('stale, busy and old grants cannot resubmit; unknown errors require verification',async()=>{
  assert.equal(render({snapshotRevision:6}).tree.props.footer.children[1].props.disabled,true)
  assert.equal(render({busy:true}).tree.props.footer.children[1].props.disabled,true)
  assert.equal(render({grant:{}}).tree.props.footer.children[1].children[0],'先核实提交状态')
  const {tree,errors}=render({onApprove:async()=>{throw new Error('fixture failure')}})
  await tree.props.footer.children[1].props.onClick()
  assert.equal(errors[1],'uncertain')
  assert.ok(errors[2].includes('可能已经生效'))
})
test('zero reported gap and non-plan proposal do not imply feasibility',()=>{
  assert.ok(JSON.stringify(render({plan:{assignments:[],unplaced:[]}}).tree).includes('这不代表所有任务均已完成'))
  const text=JSON.stringify(render({plan:null}).tree)
  assert.ok(text.includes('未提供排程缺口结论'))
  assert.equal(text.includes('排程保护规则'),false)
})

test('one synchronous apply across approval and commit; close blocked while pending',async()=>{
  let finish,calls=0
  const {tree,closed}=render({onApprove:()=>{calls++;return new Promise(resolve=>{finish=resolve})}})
  const apply=tree.props.footer.children[1].props.onClick
  const first=apply()
  await apply()
  tree.props.onClose()
  tree.props.footer.children[0].props.onClick()
  assert.equal(calls,1)
  assert.equal(closed.length,0)
  finish()
  await first
  await apply()
  assert.equal(calls,1,'resolved attempt cannot be repeated through old handler')
})
test('Core rejection requires new preview; ambiguous outcome never retries automatically or manually',async()=>{
  for(const code of ['VALIDATION_FAILED','REVISION_CONFLICT','APPROVAL_EXPIRED','APPROVAL_REQUIRED','POLICY_CHANGED','PERMISSION_DENIED','STORAGE_FAILURE',undefined]) {
    let calls=0
    const {tree,errors}=render({onApprove:async()=>{calls++;throw Object.assign(new Error('PRIVATE_RAW_FAILURE'),{code})}})
    const apply=tree.props.footer.children[1].props.onClick
    await apply()
    await apply()
    assert.equal(calls,1)
    assert.equal(errors.some(value=>value.includes('PRIVATE_RAW_FAILURE')),false)
    assert.ok(errors.at(-1).includes(['STORAGE_FAILURE',undefined].includes(code)?'核实前不要重新提交':'不能重试旧批准'))
  }
})
test('guard enforces stale, busy and inherited grant even if click handler invoked directly',async()=>{
  for(const overrides of [{snapshotRevision:6},{busy:true},{grant:'old-grant'}]) {
    const {tree,approvals}=render(overrides)
    await tree.props.footer.children[1].props.onClick()
    assert.equal(approvals.length,0)
  }
})

test('pending guard survives rerender between approval and commit when shared busy clears',async()=>{
  const hooks=[];let cursor=0,calls=0,closed=0,finish
  const Component=vm.runInNewContext(`${code}\nPlanModal`,{
    Modal:'Modal',Section:'Section',Badge:'Badge',Empty:'Empty',Button:'Button',React:{Fragment:'Fragment'},COPY:{approve:'批准'},
    h:(tag,props,...children)=>({tag,props,children}),
    useState:initial=>{const i=cursor++;if(!(i in hooks))hooks[i]=initial;return [hooks[i],value=>{hooks[i]=value}]},
    useRef:initial=>{const i=cursor++;if(!(i in hooks))hooks[i]={current:initial};return hooks[i]},
    dateTimeFor:value=>value,timeFor:value=>value,taskTitle:(_,id)=>id,stateSnapshotRef:{current:{}},
  })
  const props={proposal:{base_revision:5,preview:{}},snapshotRevision:5,plan:null,busy:false,onClose:()=>{closed++},onApprove:()=>{calls++;return new Promise(resolve=>{finish=resolve})}}
  const draw=extra=>{cursor=0;return Component({...props,...extra})}
  const initial=draw()
  const applying=initial.props.footer.children[1].props.onClick()
  const afterApproval=draw({grant:'approved-grant',busy:false})
  assert.equal(afterApproval.props.footer.children[1].props.disabled,true)
  await afterApproval.props.footer.children[1].props.onClick()
  afterApproval.props.onClose()
  assert.equal(calls,1)
  assert.equal(closed,0)
  finish();await applying
})
