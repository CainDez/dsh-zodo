import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import {test} from 'node:test'
import vm from 'node:vm'
const source=readFileSync(new URL('../../adapters/deepseek-harness/plugin/client.js',import.meta.url),'utf8')
const zones=source.slice(source.indexOf('    function workspaceTimezone'),source.indexOf('    function truncateText'))
const defaults=source.slice(source.indexOf('    function defaultTimeForm'),source.indexOf('    // L04: 时间与时区编辑'))
const editor=source.slice(source.indexOf('    function TimeEditor'),source.indexOf('    function ',source.indexOf('    function TimeEditor')+5))
const context={todayIso:()=> '2026-11-18',agendaDate:day=>/^\d{4}-\d{2}-\d{2}$/.test(day??''),localDateTimeInput:value=>value}
const form=vm.runInNewContext(`${zones}\n${defaults}\ndefaultTimeForm`,context)
test('selected date seeds only undated tasks, preserving existing hard/soft and earliest-start constraints',()=>{
  assert.equal(form(null,'Asia/Shanghai','2026-11-24').date,'2026-11-24')
  assert.equal(form({},'Asia/Shanghai','bad').date,'2026-11-18')
  const soft=form({target_due:'2026-12-01',not_before:'2026-11-23T01:00Z'},'Asia/Shanghai','2026-11-24')
  assert.equal(soft.date,'2026-12-01')
  assert.equal(soft.notBefore,'2026-11-23T09:00')
  const hard=form({hard_deadline:'2026-12-01T01:00Z'},'Asia/Shanghai','2026-11-24')
  assert.equal(hard.date,'2026-12-01')
  assert.equal(hard.time,'09:00')
  assert.equal(hard.strength,'hard')
})
test('column action only opens date-context editor, never schedules or starts focus',()=>{
  const line=source.split('\n').find(line=>line.includes('testid: `schedule-task-date-${day}`'))
  const opened=[]
  const button=vm.runInNewContext(line.trim().replace(/,$/,''),{h:(_,props)=>props,Button:'Button',day:'2026-11-24',activeTasks:[],busy:false,setModal:value=>opened.push(value)})
  assert.equal(opened.length,0)
  button.onClick()
  assert.equal(opened[0].type,'time')
  assert.equal(opened[0].day,'2026-11-24')
})
test('editor open and cancel are read-only, explicit save preserves existing versioned task-update path',async()=>{
  const writes=[],closed=[]
  const Component=vm.runInNewContext(`${zones}\n${defaults}\n${editor}\nTimeEditor`,{
    ...context,Modal:'Modal',Section:'Section',Field:'Field',Button:'Button',Empty:'Empty',React:{Fragment:'Fragment'},COPY:{save:'保存',cancel:'取消'},DUE_STRENGTH_LABELS:{},
    h:(tag,props,...children)=>({tag,props,children}),useState:value=>[typeof value==='function'?value():value,()=>{}],useEffect:()=>{},useMemo:fn=>fn(),
    taskSpec:task=>({...task}),dateTimeFor:value=>value,
  })
  const tree=Component({task:{id:'task',title:'Fixture',version:7},day:'2026-11-24',snapshot:{workspace:{timezone:'Asia/Shanghai'}},send:async (...args)=>writes.push(args),onClose:()=>closed.push(true),busy:false})
  assert.equal(writes.length,0)
  tree.props.footer.children[0].props.onClick()
  assert.equal(writes.length,0)
  assert.ok(JSON.stringify(tree).includes('不创建分配'))
  await tree.props.footer.children[2].props.onClick()
  assert.equal(writes.length,1)
  assert.equal(writes[0][0],'task.update')
  assert.equal(writes[0][1].expected_version,7)
  assert.equal(writes[0][1].task.due.date,'2026-11-24')
  assert.equal(writes[0][1].task.due.strength,'soft')
})
