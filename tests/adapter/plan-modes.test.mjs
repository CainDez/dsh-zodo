import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import {test} from 'node:test'
import vm from 'node:vm'
const source=readFileSync(new URL('../../adapters/deepseek-harness/plugin/client.js',import.meta.url),'utf8')
const begin=source.indexOf('      const openPlan = async')
const end=source.indexOf('      const approvePlan',begin)
function controller(clock = Date) {
  const writes=[],modals=[],errors=[]
  const openPlan=vm.runInNewContext(`${source.slice(begin,end)}\nopenPlan`,{
    Date:clock,setError:value=>errors.push(value),setModal:value=>modals.push(value),
    send:async(...args)=>{writes.push(args);return {data:{proposal:{id:'fixture'},plan:{}}}},
  })
  return {openPlan,writes,modals,errors}
}
test('default, event-like and unrecognized inputs are additive; only exact replan opts in',async()=>{
  for(const [mode,expected] of [[undefined,false],[{type:'click'},false],[true,false],['additive',false],['replan',true]]) {
    const f=controller();await f.openPlan(mode)
    assert.equal(f.writes.length,1)
    assert.equal(f.writes[0][0],'plan.propose')
    const payload=f.writes[0][1]
    assert.equal(payload.move_unlocked,expected)
    assert.ok(payload.intent.includes(expected?'允许重排':'仅补充安排'))
    assert.ok(payload.intent.includes('批准'))
    assert.equal(f.modals[0].type,'plan')
    assert.equal(f.modals[0].grant,null)
  }
})
test('Schedule owns both explicit planning modes; Today only navigates there',async()=>{
  const additive=source.split('\n').filter(line=>line.includes('onClick: () => openPlan(), disabled: busy'))
  const replan=source.split('\n').filter(line=>line.includes("onClick: () => openPlan('replan'), disabled: busy"))
  assert.equal(additive.length,1)
  assert.equal(replan.length,1)
  const today=source.slice(source.indexOf('      const renderToday ='),source.indexOf('      const renderInbox ='))
  assert.match(today,/today-open-schedule/)
  assert.doesNotMatch(today,/openPlan/)
  assert.equal(source.includes('onClick: openPlan'),false)
  for(const [lines,expected] of [[additive,false],[replan,true]]) for(const line of lines) {
    const f=controller()
    const button=vm.runInNewContext(line.trim().replace(/,$/,''),{h:(_,props)=>props,Button:'Button',COPY:{plan:'生成计划'},busy:false,openPlan:f.openPlan})
    await button.onClick({type:'click'})
    assert.equal(f.writes[0][1].move_unlocked,expected)
  }
})
test('command palette remains additive and only opens preview',async()=>{
  const line=source.split('\n').find(line=>line.includes("id: 'plan', glyph:"))
  const f=controller()
  const action=vm.runInNewContext(`(${line.trim().replace(/,$/,'')})`,{openPlan:f.openPlan,setModal:()=>{}})
  action.run()
  await new Promise(resolve=>setTimeout(resolve,0))
  assert.equal(f.writes[0][1].move_unlocked,false)
  assert.ok(action.hint.includes('不移动已有分配'))
})

test('fixed-clock review buffer is five minutes with unchanged seven-day duration',async()=>{
  const now=Date.parse('2026-11-18T23:58:30Z')
  class FixedDate extends Date { static now(){return now} }
  for(const mode of [undefined,'replan']) {
    const f=controller(FixedDate)
    await f.openPlan(mode)
    const payload=f.writes[0][1]
    assert.equal(payload.horizon_start,'2026-11-19T00:03:30.000Z')
    assert.equal(Date.parse(payload.horizon_end)-Date.parse(payload.horizon_start),7*24*60*60*1000)
    assert.ok(payload.intent.includes('5 分钟后'))
    assert.ok(payload.intent.includes('超过候选开始时间须重新生成'))
    assert.equal(payload.move_unlocked,mode==='replan')
  }
})
