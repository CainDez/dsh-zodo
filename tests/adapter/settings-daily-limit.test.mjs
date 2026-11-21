import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import {test} from 'node:test'
import vm from 'node:vm'
const source=readFileSync(new URL('../../adapters/deepseek-harness/plugin/client.js',import.meta.url),'utf8')
const code=source.slice(source.indexOf('    function SettingsView'),source.indexOf('    function PrivacyView'))
for(const stored of [null,0,240,undefined]) test(`Settings saves preserve dormant daily limit ${stored} and offer no editing control`,async()=>{
  const writes=[]
  const settings={timezone:'Asia/Shanghai',weekend_policy:'ask',default_estimate_minutes:30,work_windows:[]}
  if(stored!==undefined) settings.daily_limit_minutes=stored
  const snapshot={workspace:{id:'fixture',revision:1,policy_revision:1,settings},calendar_sources:[]}
  const before=JSON.stringify(snapshot)
  const Component=vm.runInNewContext(`${code}\nSettingsView`,{
    h:(tag,props,...children)=>({tag,props,children}),Button:'Button',Section:'Section',Field:'Field',Empty:'Empty',COPY:{save:'保存'},
    useState:initial=>[typeof initial==='object'?{...initial,timezone:'UTC',dailyLimit:999}:initial,()=>{}],
  })
  const tree=Component({snapshot,send:async(...args)=>writes.push(args),report:()=>{},busy:false})
  const find=(node,predicate)=>predicate(node)?node:(node?.children??[]).flat(Infinity).map(child=>find(child,predicate)).find(Boolean)
  const display=find(tree,node=>node?.props?.['data-testid']==='settings-daily-limit')
  assert.ok(JSON.stringify(display).includes('尚未启用，不参与自动排程或指定时段安排'))
  assert.equal(find(display,node=>node?.tag==='input'||node?.tag==='select'),undefined)
  assert.equal(writes.length,0)
  await find(tree,node=>node?.tag==='Button').props.onClick()
  assert.equal(writes.length,1)
  assert.equal(writes[0][0],'workspace.update')
  assert.equal(writes[0][1].settings.timezone,'UTC')
  assert.equal(writes[0][1].settings.daily_limit_minutes,stored)
  assert.equal(Object.hasOwn(writes[0][1].settings,'daily_limit_minutes'),stored!==undefined)
  assert.equal(JSON.stringify(snapshot),before)
})
