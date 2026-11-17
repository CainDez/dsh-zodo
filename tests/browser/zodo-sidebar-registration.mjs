import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import vm from 'node:vm'

let plugin
vm.runInNewContext(readFileSync('adapters/deepseek-harness/plugin/client.js', 'utf8'), {
  window: { __ModuleLoader__: { load: module => { plugin = module.factory(() => ({ createElement: (type, props, ...children) => ({ type, props, children }) })) } } },
})
// The client declares the slots surface only: no TAB_KIND/definition, no sidebarRightTabs, no right pane.
// Array.from bridges the vm realm so strict-deep equality compares plain main-realm arrays.
assert.deepEqual(Array.from(plugin.inject), ['slots'])
const registrations = []
const cleanups = []
const injected = []
const applyContext = {
  effect: callback => cleanups.push(callback()),
  slots: {
    inject: (name, callback) => { injected.push(name); return callback() },
    register: (options, component) => {
      const entry = { options, component }
      registrations.push(entry)
      return () => registrations.splice(registrations.indexOf(entry), 1)
    },
  },
}
// Any attempt to register through the removed right-sidebar surface fails the test explicitly.
Object.defineProperty(applyContext, 'sidebarRightTabs', { get() { throw new Error('client must not register right-sidebar tabs: surface removed') } })
plugin.apply(applyContext)
assert.deepEqual(injected, ['main', 'sidebar.panellist'], 'the client injects only the main panel and the sidebar entry')
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
console.log('PASS: slots-only injection, native sidebar entry, matching main panel, no composer entry, clean disposal')
