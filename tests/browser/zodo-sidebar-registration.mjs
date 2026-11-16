import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import vm from 'node:vm'

let plugin
vm.runInNewContext(readFileSync('adapters/deepseek-harness/plugin/client.js', 'utf8'), {
  window: { __ModuleLoader__: { load: module => { plugin = module.factory(() => ({ createElement: (type, props, ...children) => ({ type, props, children }) })) } } },
})
const registrations = []
const cleanups = []
plugin.apply({
  effect: callback => cleanups.push(callback()),
  sidebarRightTabs: { register: () => () => {} },
  slots: {
    inject: (name, callback) => callback(),
    register: (options, component) => {
      const entry = { options, component }
      registrations.push(entry)
      return () => registrations.splice(registrations.indexOf(entry), 1)
    },
  },
})
const panel = registrations.find(row => row.options.name === 'main')
const menu = registrations.find(row => row.options.name === 'sidebar.panellist')
assert.ok(panel)
assert.ok(menu)
assert.equal(menu.options.id, panel.options.key)
assert.equal(menu.options.label, 'Zodo')
assert.equal(menu.component({ size: 18 }).props.width, 18)
assert.ok(!registrations.some(row => row.options.name === 'conversation.composer.dock'))
for (const cleanup of cleanups.reverse()) cleanup()
assert.equal(registrations.length, 0)
console.log('PASS: native sidebar entry, matching main panel, no composer entry, clean disposal')
