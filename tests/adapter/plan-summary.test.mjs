import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import {test} from 'node:test'
import vm from 'node:vm'
const source=readFileSync(new URL('../../adapters/deepseek-harness/plugin/client.js',import.meta.url),'utf8')
const code=source.slice(source.indexOf('    function PlanModal'),source.indexOf('    function entityFactRows'))
function render(overrides={}) {
  const approvals=[],closed=[],errors=[]
  const Component=vm.runInNewContext(`${code}\nPlanModal`,{
    Modal:'Modal',Section:'Section',Badge:'Badge',Empty:'Empty',Button:'Button',React:{Fragment:'Fragment'},COPY:{approve:'批准'},
    h:(tag,props,...children)=>({tag,props,children}),useState:value=>[value,message=>errors.push(message)],
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
test('stale, busy, retry and approval error semantics remain unchanged',async()=>{
  assert.equal(render({snapshotRevision:6}).tree.props.footer.children[1].props.disabled,true)
  assert.equal(render({busy:true}).tree.props.footer.children[1].props.disabled,true)
  assert.equal(render({grant:{}}).tree.props.footer.children[1].children[0],'重试提交')
  const {tree,errors}=render({onApprove:async()=>{throw new Error('fixture failure')}})
  await tree.props.footer.children[1].props.onClick()
  assert.deepEqual(errors,['','fixture failure'])
})
test('zero reported gap and non-plan proposal do not imply feasibility',()=>{
  assert.ok(JSON.stringify(render({plan:{assignments:[],unplaced:[]}}).tree).includes('这不代表所有任务均已完成'))
  const text=JSON.stringify(render({plan:null}).tree)
  assert.ok(text.includes('未提供排程缺口结论'))
  assert.equal(text.includes('排程保护规则'),false)
})
