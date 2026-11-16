/** Host export boundary, not a keyword filter over conversation text.
 * The current Host has no verified destination-bound private export contract.
 * Keep device-only payloads inside the authenticated Zodo UI/Core channel.
 */
export const PRIVACY_GUARD_VERSION = 'structured-export-v2'

export function privacyError(code, message) {
  return Object.assign(new Error(message), { code })
}

export function containsDeviceOnlyData(value) {
  if (value === null || typeof value !== 'object') return false
  if (Array.isArray(value)) return value.some(containsDeviceOnlyData)
  if (value.data_class === 'device_only') return true
  return Object.values(value).some(containsDeviceOnlyData)
}

const ZODO_TOOLS = new Set([
  'zodo_capture', 'zodo_list_tasks', 'zodo_propose', 'zodo_plan_propose',
  'zodo_replan', 'zodo_get_proposal',
])

function restrictedResult(data) {
  const values = []
  if (data.value !== undefined) values.push(data.value)
  for (const block of data.message?.content ?? data.content ?? []) {
    if (block.type !== 'text') continue
    try { values.push(JSON.parse(block.text)) } catch { /* Not a canonical JSON result. */ }
  }
  return values.some(value => containsDeviceOnlyData(value) || (
    value && typeof value === 'object' &&
    Object.hasOwn(value, 'restricted_history_marker') &&
    typeof value.restricted_history_marker === 'string' && value.restricted_history_marker.length > 0
  ))
}

// Host-only projection. The registry owns event replay/checkpoints, including
// inherited and compressed prefixes. State contains opaque call IDs, never bodies.
export const privacyProjection = {
  key: 'zodoHostPrivacy',
  stateVersion: 2,
  // Dependency-free implementation of the registry's parse contract.
  stateSchema: { parse(value) {
    if (!value || typeof value.restricted !== 'boolean' || !Array.isArray(value.calls) ||
        !value.calls.every(id => typeof id === 'string')) throw new Error('Invalid Zodo privacy checkpoint')
    return { restricted: value.restricted, calls: [...value.calls] }
  } },
  init: () => ({ restricted: false, calls: [] }),
  apply(state, event) {
    if (state.restricted) return state
    const data = event?.data
    if (!data) return state
    if (event.type === 'tool/call' && typeof data.callId === 'string') {
      // Provider call IDs can be reused in later rounds, including by other tools.
      const calls = state.calls.filter(id => id !== data.callId)
      if (ZODO_TOOLS.has(data.name)) calls.push(data.callId)
      return calls.length !== state.calls.length || calls.some((id, i) => id !== state.calls[i])
        ? { ...state, calls } : state
    }
    const resultCallId = data.message?.source?.callId ?? data.message?.toolCallId ?? data.callId
    const native = event.type === 'tool/result' && state.calls.includes(resultCallId)
    const ptc = event.type === 'tool/ptc-dispatch' && ZODO_TOOLS.has(data.name)
    if (!native && !ptc) return state
    const restricted = restrictedResult(data)
    if (restricted) return { restricted: true, calls: [] }
    return native ? { ...state, calls: state.calls.filter(id => id !== resultCallId) } : state
  },
}

export function hasRestrictedExport(events) {
  return events.reduce(privacyProjection.apply, privacyProjection.init()).restricted
}

export function sessionHasRestrictedExport(handle, session) {
  try {
    if (!session) throw new Error('Missing session')
    const state = handle.stateOf(session)
    if (!state || typeof state.restricted !== 'boolean') throw new Error('Missing privacy state')
    return state.restricted
  } catch {
    throw privacyError('ZODO_PRIVACY_HISTORY_UNAVAILABLE',
      'Cannot verify Zodo history privacy for this session; model dispatch is refused.')
  }
}


export function assertHostExportAllowed(value) {
  if (containsDeviceOnlyData(value)) {
    // Never include the rejected body, title, ID, or upstream error in this error.
    throw privacyError('ZODO_DEVICE_ONLY_EXPORT_DENIED',
      'Device-only Zodo data cannot enter Host chat history. Use the private Zodo UI or a Core local-only workflow.')
  }
  return value
}
