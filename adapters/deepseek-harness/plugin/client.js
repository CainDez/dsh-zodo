/**
 * Zodo workbench for DeepSeek Harness (A2).
 *
 * The browser is an adapter: it maintains only navigation, form drafts and query projections. Every
 * mutation is a Core command through the Host BFF; only a committed Core receipt updates the view.
 * The Host mints an interactive UI session; this module never sees a Core or model credential.
 */
window.__ModuleLoader__.load({
  id: '@zodo/harness-plugin',
  factory(require) {
    const React = require('react')
    const h = React.createElement
    const { useCallback, useEffect, useMemo, useRef, useState } = React

    const BRIDGE = '/zodo-bridge/v1'
    const PANEL_ID = 'zodo'
    const PACKAGE_ID = '@zodo/harness-plugin'
    const NS = 'zodo'

    const COPY = {
      today: '今天',
      inbox: '收集箱',
      goals: '目标',
      projects: '项目',
      tasks: '任务',
      schedule: '日程',
      review: '回顾',
      focus: '专注',
      search: '搜索',
      settings: '设置',
      privacy: '隐私',
      agents: 'Agent',
      audit: '活动',
      more: '更多',
      capture: '收集',
      plan: '生成计划',
      save: '保存',
      cancel: '取消',
      close: '关闭',
      approve: '确认应用',
      refresh: '刷新',
      loading: '正在连接 Core…',
      connected: 'Core 已连接',
    }

    const nav = [
      { id: 'today', label: COPY.today, glyph: '◐' },
      { id: 'inbox', label: COPY.inbox, glyph: '⌄' },
      { id: 'goals', label: COPY.goals, glyph: '◎' },
      { id: 'schedule', label: COPY.schedule, glyph: '▤' },
      { id: 'review', label: COPY.review, glyph: '✓' },
    ]
    const moreNav = [
      { id: 'tasks', label: COPY.tasks, glyph: '☷' },
      { id: 'projects', label: COPY.projects, glyph: '▱' },
      { id: 'habits', label: '习惯', glyph: '↻' },
      { id: 'views', label: '视图', glyph: '▦' },
      { id: 'automation', label: '自动化', glyph: '⚡' },
      { id: 'focus', label: COPY.focus, glyph: '◉' },
      { id: 'proposals', label: '提案', glyph: '⚑' },
      { id: 'runs', label: 'AI 运行', glyph: '▷' },
      { id: 'search', label: COPY.search, glyph: '⌕' },
      { id: 'models', label: '模型', glyph: '◈' },
      { id: 'data', label: '数据', glyph: '▣' },
      { id: 'sync', label: '同步', glyph: '⇄' },
      { id: 'settings', label: COPY.settings, glyph: '⚙' },
      { id: 'privacy', label: COPY.privacy, glyph: '◇' },
      { id: 'agents', label: COPY.agents, glyph: '⌘' },
      { id: 'audit', label: COPY.audit, glyph: '≡' },
    ]

    let sessionPromise = null
    let uiSession = null
    // Desktop face: the shell strips Set-Cookie, so /session hands the bearer here and every
    // bridge request replays it through this header instead of the zodo_ui cookie.
    let bridgeToken = null

    function newId(prefix) {
      const suffix = globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(16).slice(2)}`
      return `${prefix}_${suffix}`
    }

    async function openUiSession() {
      const response = await fetch(`${BRIDGE}/session`, {
        method: 'POST',
        credentials: 'same-origin',
        headers: { 'Content-Type': 'application/json' },
        body: '{}',
      })
      const body = await response.json().catch(() => undefined)
      if (!response.ok || !body?.ok) {
        const error = new Error(body?.error?.message ?? `无法建立 Zodo UI 会话 (${response.status})`)
        error.code = body?.error?.code ?? `HTTP_${response.status}`
        throw error
      }
      uiSession = body
      bridgeToken = typeof body.session_token === 'string' && body.session_token ? body.session_token : null
      return body
    }

    async function ensureSession() {
      if (sessionPromise) return sessionPromise
      sessionPromise = openUiSession().catch(error => {
        sessionPromise = null
        throw error
      })
      return sessionPromise
    }

    async function bridgeRpc(kind, envelope, retried = false) {
      await ensureSession()
      const response = await fetch(`${BRIDGE}/rpc`, {
        method: 'POST',
        credentials: 'same-origin',
        headers: { 'Content-Type': 'application/json', ...(bridgeToken ? { 'X-Zodo-UI-Session': bridgeToken } : {}) },
        body: JSON.stringify({ kind, envelope }),
      })
      const body = await response.json().catch(() => undefined)
      if (response.status === 401 && !retried) {
        sessionPromise = null
        uiSession = null
        bridgeToken = null
        return bridgeRpc(kind, envelope, true)
      }
      if (!response.ok || !body?.ok) {
        const error = new Error(body?.error?.message ?? `Zodo 请求失败 (${response.status})`)
        error.code = body?.error?.code ?? `HTTP_${response.status}`
        error.requestId = body?.request_id
        throw error
      }
      return body.result
    }

    async function query(name, payload = {}) {
      await ensureSession()
      const workspaceId = uiSession?.core?.workspace_id ?? 'ws_local'
      return bridgeRpc('query', {
        schema: 'zodo.query/1',
        request_id: newId('uiq'),
        workspace_id: workspaceId,
        query: name,
        payload,
      })
    }

    async function command(name, payload, baseRevision, key) {
      await ensureSession()
      const workspaceId = uiSession?.core?.workspace_id ?? 'ws_local'
      return bridgeRpc('command', {
        schema: 'zodo.command/1',
        request_id: newId('uir'),
        idempotency_key: key ?? newId('uiop'),
        workspace_id: workspaceId,
        operation: name,
        operation_version: 1,
        base_revision: baseRevision ?? null,
        payload,
      })
    }

    async function bridgeJson(pathname, options = {}, retried = false) {
      await ensureSession()
      const response = await fetch(`${BRIDGE}${pathname}`, {
        ...options,
        credentials: 'same-origin',
        headers: {
          ...(options.body ? { 'Content-Type': 'application/json' } : {}),
          ...(options.headers ?? {}),
          ...(bridgeToken ? { 'X-Zodo-UI-Session': bridgeToken } : {}),
        },
      })
      const body = await response.json().catch(() => undefined)
      if (response.status === 401 && !retried) {
        sessionPromise = null
        uiSession = null
        bridgeToken = null
        return bridgeJson(pathname, options, true)
      }
      if (!response.ok || !body?.ok) {
        const error = new Error(body?.error?.message ?? `Zodo 请求失败 (${response.status})`)
        error.code = body?.error?.code ?? `HTTP_${response.status}`
        throw error
      }
      return body.result ?? body
    }

    async function startRun(intent, input) {
      return bridgeJson('/runs', {
        method: 'POST',
        body: JSON.stringify({ intent, input, idempotency_key: newId('run') }),
      })
    }

    async function listRuns() {
      const result = await bridgeJson('/runs')
      return result.runs ?? []
    }

    async function getRun(id) {
      const result = await bridgeJson(`/runs/${encodeURIComponent(id)}`)
      return result.run
    }

    async function cancelRun(id) {
      return bridgeJson(`/runs/${encodeURIComponent(id)}/cancel`, { method: 'POST', body: '{}' })
    }

    function requireModelResponse(result) {
      if (result?.schema !== 'zodo.model/1' || result.config?.version !== 2 || !result.effective?.selected
        || !['dsh', 'custom'].includes(result.config.source?.kind)) {
        throw new Error('Core 模型接口无效，请检查插件与 Core 构建是否一致。')
      }
      return result
    }

    async function getModelConfig() {
      return requireModelResponse(await bridgeJson('/model'))
    }

    async function updateModel(config) {
      return requireModelResponse(await bridgeJson('/model', {
        method: 'POST',
        body: JSON.stringify(config),
      }))
    }

    async function getHarnessModels() {
      return bridgeJson('/harness/models')
    }

    async function getRevocations() {
      return bridgeJson('/admin/revocations')
    }

    async function revokeChannel(channel, revoked) {
      return bridgeJson('/admin/revoke', {
        method: 'POST',
        body: JSON.stringify({ channel, revoked }),
      })
    }

    async function exportData() {
      return bridgeJson('/data/export')
    }

    async function createBackup() {
      return bridgeJson('/data/backup', { method: 'POST', body: '{}' })
    }

    async function listBackups() {
      return bridgeJson('/data/backups')
    }

    async function syncExport() {
      return bridgeJson('/sync/export')
    }

    async function syncImport(bundle) {
      return bridgeJson('/sync/import', {
        method: 'POST',
        body: JSON.stringify({ bundle }),
      })
    }

    function dateForLocal(value) {
      if (!value) return ''
      const date = new Date(value)
      if (Number.isNaN(date.getTime())) return value
      return new Intl.DateTimeFormat('zh-CN', { month: 'numeric', day: 'numeric' }).format(date)
    }

    function timeFor(value) {
      if (!value) return ''
      const date = new Date(value)
      if (Number.isNaN(date.getTime())) return value
      return new Intl.DateTimeFormat('zh-CN', { hour: '2-digit', minute: '2-digit', hour12: false }).format(date)
    }

    function dateTimeFor(value) {
      if (!value) return ''
      const date = new Date(value)
      if (Number.isNaN(date.getTime())) return value
      return new Intl.DateTimeFormat('zh-CN', {
        month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit', hour12: false,
      }).format(date)
    }

    function localDateInput(value) {
      if (!value) return ''
      const date = typeof value === 'string' ? new Date(value) : value
      if (Number.isNaN(date.getTime())) return String(value).slice(0, 10)
      const offset = date.getTimezoneOffset() * 60000
      return new Date(date.getTime() - offset).toISOString().slice(0, 10)
    }

    function localDateTimeInput(value) {
      if (!value) return ''
      const date = new Date(value)
      if (Number.isNaN(date.getTime())) return String(value).slice(0, 16)
      const offset = date.getTimezoneOffset() * 60000
      return new Date(date.getTime() - offset).toISOString().slice(0, 16)
    }

    function toIso(value) {
      if (!value) return null
      const date = new Date(value)
      return Number.isNaN(date.getTime()) ? null : date.toISOString()
    }

    // L04: the workspace owns the IANA timezone. The browser only converts wall-clock input for
    // preview; it never rewrites the browser or host timezone and never guesses a DST solution.
    function workspaceTimezone(snapshot) {
      return snapshot?.workspace?.timezone ?? snapshot?.workspace?.settings?.timezone ?? 'Asia/Shanghai'
    }

    function isKnownTimezone(timeZone) {
      if (!timeZone) return false
      try {
        new Intl.DateTimeFormat('en-US', { timeZone }).format(new Date())
        return true
      } catch {
        return false
      }
    }

    function zonePartsAt(instantMs, timeZone) {
      const formatter = new Intl.DateTimeFormat('en-US', {
        timeZone,
        hour12: false,
        year: 'numeric',
        month: '2-digit',
        day: '2-digit',
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit',
      })
      const parts = {}
      for (const part of formatter.formatToParts(new Date(instantMs))) {
        if (part.type !== 'literal') parts[part.type] = part.value
      }
      return {
        year: Number(parts.year),
        month: Number(parts.month),
        day: Number(parts.day),
        hour: Number(parts.hour === '24' ? '0' : parts.hour),
        minute: Number(parts.minute),
        second: Number(parts.second),
      }
    }

    // Resolve a wall-clock date/time in the workspace timezone to every UTC instant that renders as
    // that wall clock. Two instants mean the local clock repeats (DST fall back); zero means the
    // local clock does not exist (DST spring forward). No silent nearest-match is returned.
    function resolveWallTime(dateText, timeText, timeZone) {
      const [year, month, day] = String(dateText ?? '').split('-').map(Number)
      const [hour, minute] = String(timeText || '00:00').split(':').map(Number)
      if (![year, month, day, hour, minute].every(value => Number.isFinite(value))) {
        return { instants: [], ambiguous: false, missing: true }
      }
      const naive = Date.UTC(year, month - 1, day, hour, minute, 0)
      const instants = []
      // Real UTC offsets today live in [-12:00, +14:00] and all are multiples of 15 minutes.
      for (let offset = -12 * 60; offset <= 14 * 60; offset += 15) {
        const candidate = naive - offset * 60_000
        const parts = zonePartsAt(candidate, timeZone)
        const matches = parts.year === year && parts.month === month && parts.day === day
          && parts.hour === hour && parts.minute === minute
        if (matches && !instants.includes(candidate)) instants.push(candidate)
      }
      instants.sort((left, right) => left - right)
      return { instants, ambiguous: instants.length > 1, missing: instants.length === 0 }
    }

    function wallClockText(instantMs, timeZone) {
      const parts = zonePartsAt(instantMs, timeZone)
      return `${parts.year}-${String(parts.month).padStart(2, '0')}-${String(parts.day).padStart(2, '0')} ${String(parts.hour).padStart(2, '0')}:${String(parts.minute).padStart(2, '0')}`
    }

    function wallClockInput(instantMs, timeZone) {
      const parts = zonePartsAt(instantMs, timeZone)
      return `${parts.year}-${String(parts.month).padStart(2, '0')}-${String(parts.day).padStart(2, '0')}T${String(parts.hour).padStart(2, '0')}:${String(parts.minute).padStart(2, '0')}`
    }

    function truncateText(value, limit = 80) {
      const text = value == null ? '' : typeof value === 'string' ? value : JSON.stringify(value)
      if (text.length <= limit) return { text, truncated: false }
      return { text: `${text.slice(0, limit)}…`, truncated: true }
    }

    const DUE_STRENGTH_LABELS = { soft: '期望完成（软约束）', hard: '必须完成（硬约束）' }

    const DATA_CLASS_LABELS = {
      device_only: '仅此设备',
      trusted_workspace: '受信工作空间',
      cloud_allowed: '允许云端',
    }

    const SOURCE_KIND_LABELS = {
      harness_ui: '人工 UI 提交（没有模型外发）',
      harness_agent: 'Harness Agent 工具提交（结果可能进入宿主会话日志）',
      mcp_stdio: 'MCP 外部客户端提交',
      ai: 'Core AI 运行提交（外发发生在运行阶段）',
      user: '人工操作',
      system: '系统动作',
    }

    const SCOPE_LABELS = {
      read: '读取事实',
      read_private: '读取私密正文',
      write: '写入',
      capture: '收集',
      propose: '提出候选',
      approve: '批准',
      execute: '执行',
      admin: '管理',
      export: '导出',
    }

    const SCOPE_SHORT_LABELS = {
      read: '读',
      read_private: '私密读',
      write: '写',
      capture: '收集',
      propose: '提议',
      approve: '批准',
      execute: '执行',
      admin: '管理',
      export: '导出',
    }

    const CHANNEL_RULES = [
      {
        channel: 'harness_ui',
        alias: 'ui',
        label: 'Harness 人工 UI',
        principal: 'user_ui',
        kind: 'interactive_ui',
        scopes: ['read', 'read_private', 'write', 'capture', 'propose', 'approve', 'execute', 'admin'],
        selfManaged: true,
        summary: '当前浏览器会话的管理面。撤销会切断所有 UI 命令，无法从浏览器自助恢复。',
      },
      {
        channel: 'harness_agent',
        alias: 'agent',
        label: 'Harness Agent',
        principal: 'agent_harness',
        kind: 'delegated_agent',
        scopes: ['read', 'capture', 'propose', 'execute'],
        summary: '宿主 Agent 工具通道：读事实、记录收集、提交候选、执行已批准动作，但不能批准自己的建议。',
      },
      {
        channel: 'mcp_stdio',
        alias: 'mcp',
        label: 'MCP',
        principal: 'agent_mcp',
        kind: 'mcp_client',
        scopes: ['read', 'propose'],
        summary: '外部 MCP 客户端通道：只读事实与提交候选，不能批准或写入正式事实。',
      },
      {
        channel: 'cli',
        alias: 'cli',
        label: 'CLI',
        principal: 'owner_cli',
        kind: 'owner',
        scopes: ['read', 'read_private', 'write', 'capture', 'propose', 'approve', 'execute', 'admin', 'export'],
        summary: '本机所有者 CLI：直接持有 owner token；UI 不能声明它的运行边界。',
      },
    ]

    function channelRule(channel) {
      return CHANNEL_RULES.find(rule => rule.channel === channel) ?? null
    }

    function todayIso() {
      const date = new Date()
      const offset = date.getTimezoneOffset() * 60000
      return new Date(date.getTime() - offset).toISOString().slice(0, 10)
    }

    function addDays(isoDate, days) {
      const date = new Date(`${isoDate}T12:00:00`)
      date.setDate(date.getDate() + days)
      const offset = date.getTimezoneOffset() * 60000
      return new Date(date.getTime() - offset).toISOString().slice(0, 10)
    }

    function startOfWeekIso(isoDate) {
      const date = new Date(`${isoDate}T12:00:00`)
      const day = date.getDay() === 0 ? 7 : date.getDay()
      return addDays(isoDate, -(day - 1))
    }

    // BEGIN PURE AGENDA PROJECTION: also exercised directly by schedule-projection.test.mjs.
    // Navigation is civil-date arithmetic; event boundaries are browser-local midnights.
    const AGENDA_MIN = '0001-01-01'
    const AGENDA_MAX = '9999-12-25' // Leave room for the complete seven-day window.

    function agendaDate(value) {
      if (typeof value !== 'string' || !/^\d{4}-\d{2}-\d{2}$/.test(value)) return null
      const date = new Date(`${value}T12:00:00Z`)
      return Number.isFinite(date.getTime()) && date.toISOString().slice(0, 10) === value ? date : null
    }

    function agendaShift(value, days, length = 7) {
      const maximum = length === 1 ? '9999-12-31' : AGENDA_MAX
      const date = agendaDate(value)
      if (!date || !Number.isSafeInteger(days)) return AGENDA_MIN
      date.setUTCDate(date.getUTCDate() + days)
      if (!Number.isFinite(date.getTime()) || date.getUTCFullYear() > 9999) return days < 0 ? AGENDA_MIN : maximum
      if (date.getUTCFullYear() < 1) return AGENDA_MIN
      const result = date.toISOString().slice(0, 10)
      return result < AGENDA_MIN ? AGENDA_MIN : result > maximum ? maximum : result
    }

    function agendaDays(start, length = 7) {
      const count = length === 1 ? 1 : 7
      const date = agendaDate(agendaShift(start, 0, count))
      return Array.from({ length: count }, (_, index) => {
        const day = new Date(date.getTime())
        day.setUTCDate(day.getUTCDate() + index)
        return day.toISOString().slice(0, 10)
      })
    }

    function agendaEntries(snapshot, day) {
      if (!agendaDate(day)) return []
      const boundary = new Date(`${day}T00:00:00`)
      const dayStart = boundary.getTime()
      boundary.setDate(boundary.getDate() + 1)
      const dayEnd = boundary.getTime()
      const entries = []
      for (const [kind, rows] of [
        ['capacity', snapshot.capacity_blocks],
        ['commitment', snapshot.commitments],
        ['allocation', snapshot.allocations],
        // Match Core planner's busy predicate. Project only interval data, never
        // imported titles, notes, source IDs or external IDs into view models.
        ['busy', (snapshot.calendar_events ?? []).filter(event => event.busy === true)
          .map((event, index) => ({ id: `busy-${index}`, start: event.start, end: event.end }))],
      ]) {
        for (const row of rows ?? []) {
          const start = typeof row.start === 'string' ? Date.parse(row.start) : NaN
          const end = typeof row.end === 'string' ? Date.parse(row.end) : NaN
          // Half-open intervals exclude midnight endings from the following day.
          if (!Number.isFinite(start) || !Number.isFinite(end) || end <= start || start >= dayEnd || end <= dayStart) continue
          entries.push({ kind, row, start, end,
            visibleStart: Math.max(start, dayStart), visibleEnd: Math.min(end, dayEnd),
            continuesBefore: start < dayStart, continuesAfter: end > dayEnd,
            endsAtDayBoundary: end >= dayEnd,
          })
        }
      }
      // Compare instants, not offset-bearing strings or grouped event types.
      return entries.sort((a, b) => a.start - b.start || a.end - b.end ||
        a.kind.localeCompare(b.kind) || String(a.row.id).localeCompare(String(b.row.id)))
    }
    function calendarFreshness(source, now = Date.now()) {
      const synced = typeof source.last_sync_at === 'string' ? Date.parse(source.last_sync_at) : NaN
      const ttl = source.freshness_seconds
      if (!Number.isFinite(synced) || synced > now || !Number.isFinite(ttl) || ttl < 0) return '未知'
      return (now - synced) / 1000 > ttl ? '过期' : '新鲜'
    }
    // Read-only selection, not a feasibility check or a running focus session.
    function todayNextAction(entries, tasks, suggestion, now) {
      const scheduled = entries.filter(entry => entry.kind === 'allocation' && entry.end > now)
        .map(entry => ({ entry, task: tasks.find(task => task.id === entry.row.task_id) }))
        .find(candidate => candidate.task)
      if (scheduled) return { ...scheduled, kind: scheduled.entry.start <= now ? 'scheduled-now' : 'scheduled-next' }
      return suggestion ? { kind: 'suggestion', task: suggestion } : null
    }
    function outstandingWork(snapshot, now) {
      const unknown = { rows: [], minutes: 0, unknown: true }
      if (!Number.isFinite(now) || !Array.isArray(snapshot?.tasks) || !Array.isArray(snapshot?.allocations)) return unknown
      const tasks = snapshot.tasks
      if (tasks.some(task => !task || typeof task.id !== 'string')) return unknown
      const rows = []
      let hasUnknown = false
      const validMinutes = value => Number.isSafeInteger(value) && value >= 0
      const parents = new Set(tasks.filter(task => !task.archived).map(task => task.parent_id))
      const reservations = new Map()
      let malformedAllocation = false
      for (const allocation of snapshot.allocations) {
        if (!allocation || typeof allocation.task_id !== 'string') { malformedAllocation = true; continue }
        const reserved = reservations.get(allocation.task_id) ?? { minutes: 0, invalid: false }
        const start = typeof allocation.start === 'string' ? Date.parse(allocation.start) : NaN
        const end = typeof allocation.end === 'string' ? Date.parse(allocation.end) : NaN
        if (!Number.isFinite(start) || !Number.isFinite(end) || end <= start || !validMinutes(allocation.minutes)) {
          reserved.invalid = true
        } else if (end > now) {
          // Core reserves the whole allocation until its end, including straddling
          // intervals. Elapsed time is not evidence of task completion.
          reserved.minutes += allocation.minutes
        }
        reservations.set(allocation.task_id, reserved)
      }
      for (const task of tasks) {
        if (task.archived || task.status === 'done' || task.status === 'cancelled' || parents.has(task.id)) continue
        const remaining = task.remaining_minutes ?? task.estimate_minutes
        if (!validMinutes(remaining)) { hasUnknown = true; continue }
        const reserved = reservations.get(task.id) ?? { minutes: 0, invalid: false }
        if (malformedAllocation || reserved.invalid || !Number.isSafeInteger(reserved.minutes)) { hasUnknown = true; continue }
        const minutes = Math.max(0, remaining - reserved.minutes)
        if (minutes > 0) rows.push({ task, minutes })
      }
      const minutes = rows.reduce((sum, row) => sum + row.minutes, 0)
      return Number.isSafeInteger(minutes) ? { rows, minutes, unknown: hasUnknown } : unknown
    }
    // END PURE AGENDA PROJECTION

    function statusLabel(status) {
      return ({ open: '待处理', in_progress: '进行中', done: '已完成', cancelled: '已取消' })[status] ?? status
    }

    function taskTitle(snapshot, id) {
      return snapshot?.tasks?.find(task => task.id === id)?.title ?? id
    }

    function findTask(snapshot, id) {
      return snapshot?.tasks?.find(task => task.id === id)
    }

    function projectName(snapshot, id) {
      return snapshot?.projects?.find(project => project.id === id)?.title ?? ''
    }

    function taskSpec(task) {
      return {
        title: task.title,
        description: task.description ?? null,
        project_id: task.project_id ?? null,
        parent_id: task.parent_id ?? null,
        estimate_minutes: task.estimate_minutes ?? null,
        priority: task.priority ?? 2,
        target_due: task.target_due ?? null,
        hard_deadline: task.hard_deadline ?? null,
        not_before: task.not_before ?? null,
        preferred_window: task.preferred_window ?? null,
        tags: task.tags ?? [],
      }
    }

    function Button({ children, onClick, tone = 'secondary', disabled, type = 'button', testid, title, style }) {
      return h('button', {
        type,
        onClick,
        onMouseDown: event => {
          // Some host shells suppress default mousedown focus. Keep the keyboard restore target
          // explicit so closing a dialog returns focus to the control that opened it.
          if (!disabled && event.currentTarget instanceof HTMLElement) event.currentTarget.focus()
        },
        disabled: Boolean(disabled),
        title,
        'data-testid': testid,
        className: `zw-btn zw-btn-${tone}`,
        style,
      }, children)
    }

    function Badge({ children, tone = 'neutral', testid }) {
      return h('span', { className: `zw-badge zw-badge-${tone}`, 'data-testid': testid }, children)
    }

    function Field({ label, hint, children, wide }) {
      return h('label', { className: `zw-field ${wide ? 'zw-field-wide' : ''}` },
        h('span', { className: 'zw-field-label' }, label),
        children,
        hint ? h('span', { className: 'zw-field-hint' }, hint) : null,
      )
    }

    const lastFocusedElement = { current: null }
    const lastPointerButton = { current: null }
    if (typeof document !== 'undefined') {
      document.addEventListener('focusin', event => {
        if (event.target instanceof HTMLElement) lastFocusedElement.current = event.target
      }, true)
      document.addEventListener('pointerdown', event => {
        const button = event.target instanceof Element ? event.target.closest('button') : null
        if (button instanceof HTMLElement) lastPointerButton.current = button
      }, true)
    }

    function Modal({ title, subtitle, children, footer, onClose, wide }) {
      const ref = useRef(null)
      const restoreTarget = useRef(lastPointerButton.current ?? null)
      useEffect(() => {
        // Accepting dialog guidance: remember the trigger so closing returns focus instead of
        // dropping keyboard users on document.body. Cleanup runs for save, cancel and Escape alike.
        restoreTarget.current = typeof HTMLElement !== 'undefined' && document.activeElement instanceof HTMLElement
          ? document.activeElement
          : null
        return () => {
          const candidate = restoreTarget.current
          const pointer = lastPointerButton.current
          const fallback = lastFocusedElement.current
          const target = candidate && document.contains(candidate) ? candidate
            : pointer && document.contains(pointer) ? pointer
            : fallback && document.contains(fallback) ? fallback : null
          if (target && typeof target.focus === 'function') {
            // Run after React has removed the dialog; focusing during cleanup can be overridden by
            // the browser's own focus handling and leave keyboard users on document.body.
            setTimeout(() => {
              if (document.contains(target)) target.focus({ preventScroll: true })
            }, 0)
          }
        }
      }, [])
      useEffect(() => {
        const onKey = event => {
          if (event.key === 'Escape') onClose()
        }
        window.addEventListener('keydown', onKey)
        ref.current?.focus()
        return () => window.removeEventListener('keydown', onKey)
      }, [onClose])
      return h('div', { className: 'zw-overlay', role: 'presentation', onMouseDown: event => {
        if (event.target === event.currentTarget) onClose()
      } },
      h('section', {
        className: `zw-modal ${wide ? 'zw-modal-wide' : ''}`,
        role: 'dialog',
        'aria-modal': 'true',
        'aria-label': title,
        tabIndex: -1,
        ref,
      },
      h('header', { className: 'zw-modal-head' },
        h('div', null, h('h2', null, title), subtitle ? h('p', null, subtitle) : null),
        h('button', { className: 'zw-icon-btn', onClick: onClose, 'aria-label': COPY.close }, '×'),
      ),
      h('div', { className: 'zw-modal-body' }, children),
      footer ? h('footer', { className: 'zw-modal-foot' }, footer) : null,
      ))
    }

    function Section({ title, action, children, className = '', testid }) {
      return h('section', { className: `zw-section ${className}`, 'data-testid': testid },
        (title || action) ? h('header', { className: 'zw-section-head' },
          title ? h('h3', null, title) : h('span'),
          action,
        ) : null,
        children,
      )
    }

    function Empty({ children }) {
      return h('div', { className: 'zw-empty' }, children)
    }

    function Pill({ children, active, onClick, testid }) {
      return h('button', {
        type: 'button',
        className: `zw-pill ${active ? 'zw-pill-active' : ''}`,
        onClick,
        'data-testid': testid,
      }, children)
    }

    function TaskRow({ snapshot, task, onSelect, compact = false, selectable = false, selected = false, onToggleSelection }) {
      const project = projectName(snapshot, task.project_id)
      const overdue = task.target_due && task.target_due < todayIso() && task.status !== 'done'
      return h('button', {
        type: 'button',
        className: `zw-task-row ${compact ? 'zw-task-row-compact' : ''} ${task.status === 'done' ? 'zw-task-done' : ''}`,
        onClick: () => onSelect(task.id),
        'data-testid': `task-row-${task.id}`,
      },
      selectable ? h('input', {
        type: 'checkbox',
        className: 'zw-select',
        checked: selected,
        onClick: event => {
          event.stopPropagation()
          onToggleSelection(task.id)
        },
        onChange: () => {},
        'aria-label': `选择 ${task.title}`,
      }) : null,
      h('span', { className: `zw-check ${task.status === 'in_progress' ? 'zw-check-active' : ''}` },
        task.status === 'done' ? '✓' : task.status === 'in_progress' ? '•' : ''),
      h('span', { className: 'zw-task-main' },
        h('span', { className: 'zw-task-title' }, task.title),
        h('span', { className: 'zw-task-meta' },
          task.estimate_minutes ? `${task.estimate_minutes} 分钟` : '未估时',
          project ? ` · ${project}` : '',
          task.target_due ? ` · 期望 ${dateForLocal(`${task.target_due}T12:00:00`)}` : '',
          task.hard_deadline ? ` · 必须 ${dateForLocal(task.hard_deadline)}` : '',
        ),
      ),
      overdue ? h(Badge, { tone: 'warn' }, '逾期') : null,
      h(Badge, { tone: task.status === 'in_progress' ? 'accent' : 'neutral' }, statusLabel(task.status)),
      )
    }

    function useContainerWidth(ref) {
      const [width, setWidth] = useState(860)
      useEffect(() => {
        if (!ref.current) return
        const observer = new ResizeObserver(entries => {
          const entry = entries[0]
          if (entry) setWidth(entry.contentRect.width)
        })
        observer.observe(ref.current)
        return () => observer.disconnect()
      }, [ref])
      return width
    }

    function TaskEditor({ task, snapshot, projectId, onClose, send, busy }) {
      const [form, setForm] = useState({
        title: task?.title ?? '',
        estimate: task?.estimate_minutes ?? 30,
        priority: task?.priority ?? 2,
        project: task?.project_id ?? projectId ?? '',
        parent: task?.parent_id ?? '',
        targetDue: task?.target_due ?? '',
        hardDeadline: task?.hard_deadline ? localDateTimeInput(task.hard_deadline) : '',
        tags: (task?.tags ?? []).join(', '),
        description: task?.description ?? '',
      })
      const [error, setError] = useState('')
      const submit = async () => {
        setError('')
        if (!form.title.trim()) return setError('请填写标题')
        const spec = {
          title: form.title.trim(),
          description: form.description.trim() || null,
          project_id: form.project || null,
          parent_id: form.parent || null,
          estimate_minutes: Number(form.estimate) > 0 ? Number(form.estimate) : null,
          priority: Number(form.priority) || 0,
          target_due: form.targetDue || null,
          hard_deadline: form.hardDeadline ? toIso(form.hardDeadline) : null,
          not_before: task?.not_before ?? null,
          preferred_window: task?.preferred_window ?? null,
          tags: form.tags.split(',').map(value => value.trim()).filter(Boolean),
        }
        try {
          if (task) {
            await send('task.update', { id: task.id, task: spec, expected_version: task.version })
          } else {
            await send('task.create', { task: spec })
          }
          onClose()
        } catch (failure) {
          setError(failure.message)
        }
      }
      return h(Modal, {
        title: task ? '编辑任务' : '新建任务',
        subtitle: '保存后由 Core 校验关系与版本。',
        onClose,
        footer: h(React.Fragment, null,
          h(Button, { onClick: onClose }, COPY.cancel),
          h(Button, { tone: 'primary', onClick: submit, disabled: busy, testid: 'task-save' }, COPY.save),
        ),
      },
      error ? h('div', { className: 'zw-alert zw-alert-error' }, error) : null,
      h('div', { className: 'zw-grid2' },
        h(Field, { label: '标题', wide: true }, h('input', {
          value: form.title, autoFocus: true,
          onChange: event => setForm({ ...form, title: event.target.value }),
        })),
        h(Field, { label: '预计分钟' }, h('input', {
          type: 'number', min: 1, value: form.estimate,
          onChange: event => setForm({ ...form, estimate: event.target.value }),
        })),
        h(Field, { label: '优先级 0–4' }, h('input', {
          type: 'number', min: 0, max: 4, value: form.priority,
          onChange: event => setForm({ ...form, priority: event.target.value }),
        })),
        h(Field, { label: '所属项目' }, h('select', {
          value: form.project,
          onChange: event => setForm({ ...form, project: event.target.value }),
        }, h('option', { value: '' }, '无'), (snapshot?.projects ?? []).map(project =>
          h('option', { key: project.id, value: project.id }, project.title)))),
        h(Field, { label: '父任务' }, h('select', {
          value: form.parent,
          onChange: event => setForm({ ...form, parent: event.target.value }),
        }, h('option', { value: '' }, '无'), (snapshot?.tasks ?? []).filter(candidate => !task || candidate.id !== task.id).map(candidate =>
          h('option', { key: candidate.id, value: candidate.id }, candidate.title)))),
        h(Field, { label: '期望完成' }, h('input', {
          type: 'date', value: form.targetDue,
          onChange: event => setForm({ ...form, targetDue: event.target.value }),
        })),
        h(Field, { label: '必须完成（时刻）' }, h('input', {
          type: 'datetime-local', value: form.hardDeadline,
          onChange: event => setForm({ ...form, hardDeadline: event.target.value }),
        })),
        h(Field, { label: '标签（逗号分隔）', wide: true }, h('input', {
          value: form.tags,
          onChange: event => setForm({ ...form, tags: event.target.value }),
        })),
        h(Field, { label: '说明', wide: true }, h('textarea', {
          rows: 3, value: form.description,
          onChange: event => setForm({ ...form, description: event.target.value }),
        })),
      ))
    }

    function defaultTimeForm(task, timeZone, day) {
      const zoneUsable = isKnownTimezone(timeZone)
      const hardInstant = task?.hard_deadline ? new Date(task.hard_deadline).getTime() : null
      const hardKnown = hardInstant != null && Number.isFinite(hardInstant)
      const hardWall = hardKnown && zoneUsable ? wallClockInput(hardInstant, timeZone) : null
      return {
        date: hardWall ? hardWall.slice(0, 10) : (task?.target_due ?? (agendaDate(day) ? day : todayIso())),
        time: hardWall ? hardWall.slice(11, 16) : '',
        strength: task?.hard_deadline ? 'hard' : 'soft',
        notBefore: task?.not_before
          ? (zoneUsable ? wallClockInput(new Date(task.not_before).getTime(), timeZone) : localDateTimeInput(task.not_before))
          : '',
        dueChoice: null,
        notBeforeChoice: null,
      }
    }

    // L04: 时间与时区编辑。Every save writes a complete TaskSpec (existing fields preserved) so
    // Core keeps its version/relationship validation; the dialog never sends a partial patch.
    function TimeEditor({ task, tasks, day, snapshot, onClose, send, busy }) {
      const timeZone = workspaceTimezone(snapshot)
      const zoneValid = isKnownTimezone(timeZone)
      const [pickedId, setPickedId] = useState(task?.id ?? '')
      const selected = task ?? (tasks ?? []).find(candidate => candidate.id === pickedId) ?? null
      const [form, setForm] = useState(() => defaultTimeForm(selected, timeZone, day))
      const [error, setError] = useState('')
      useEffect(() => {
        setForm(defaultTimeForm(selected, timeZone, day))
      }, [selected?.id, day])
      const dueResolution = useMemo(() => {
        if (!zoneValid || !form.date) return null
        return resolveWallTime(form.date, form.time || '00:00', timeZone)
      }, [zoneValid, form.date, form.time, timeZone])
      const notBeforeResolution = useMemo(() => {
        if (!zoneValid || form.notBefore.length < 16) return null
        return resolveWallTime(form.notBefore.slice(0, 10), form.notBefore.slice(11, 16), timeZone)
      }, [zoneValid, form.notBefore, timeZone])
      const hasTime = Boolean(form.time)
      const pickInstant = (resolution, choice) => {
        if (!resolution?.instants?.length) return null
        return choice != null && resolution.instants.includes(choice) ? choice : resolution.instants[0]
      }
      const dueInstant = pickInstant(dueResolution, form.dueChoice)
      const notBeforeInstant = pickInstant(notBeforeResolution, form.notBeforeChoice)
      // Core turns a hard `date` due into 23:59:59 on that local day; mirror that in the preview.
      const dateOnlyHardResolved = zoneValid && !hasTime && form.strength === 'hard' && form.date
        ? resolveWallTime(form.date, '23:59', timeZone)
        : null
      const dateOnlyHardInstant = dateOnlyHardResolved?.instants?.[0] ?? null
      const dueReferenceInstant = hasTime ? dueInstant : (form.strength === 'hard' ? dateOnlyHardInstant : null)
      const orderingError = (() => {
        if (notBeforeInstant == null) return ''
        if (hasTime && dueInstant != null && notBeforeInstant > dueInstant) return '最早开始晚于截止时刻；请调整后再保存。'
        if (!hasTime && form.strength === 'hard' && dateOnlyHardInstant != null && notBeforeInstant > dateOnlyHardInstant + 59_000) {
          return '最早开始晚于当地 23:59:59 的硬截止；请调整后再保存。'
        }
        if (!hasTime && form.strength === 'soft' && form.date && form.notBefore.slice(0, 10) > form.date) {
          return '最早开始晚于期望完成日期；请调整后再保存。'
        }
        return ''
      })()
      const softUtcDate = form.strength === 'soft' && hasTime && dueInstant != null
        ? new Date(dueInstant).toISOString().slice(0, 10)
        : null
      const softDateShift = softUtcDate != null && softUtcDate !== form.date
      const save = async () => {
        setError('')
        if (!selected) return setError('请选择要设置时间的任务')
        if (!zoneValid) return setError(`无法解析工作空间时区「${timeZone}」，请先在设置中修正 IANA 时区`)
        if (!form.date) return setError('请选择日期')
        if (dueResolution?.missing) return setError(`当地时刻在 ${timeZone} 不存在（夏令时跳变），请换一个钟点或只保存日期`)
        if (notBeforeResolution?.missing) return setError(`最早开始时刻在 ${timeZone} 不存在（夏令时跳变），请换一个时刻`)
        if (hasTime && dueResolution?.ambiguous && form.dueChoice == null) {
          return setError('该当地钟点在夏令时回拨时出现两次，请选择要保存的 UTC 时刻')
        }
        if (notBeforeResolution?.ambiguous && form.notBeforeChoice == null) {
          return setError('最早开始时刻在夏令时回拨时出现两次，请选择要保存的 UTC 时刻')
        }
        if (orderingError) return setError(orderingError)
        const spec = taskSpec(selected)
        delete spec.target_due
        delete spec.hard_deadline
        spec.due = hasTime
          ? { at: new Date(dueInstant).toISOString(), strength: form.strength }
          : { date: form.date, strength: form.strength }
        spec.not_before = notBeforeInstant != null ? new Date(notBeforeInstant).toISOString() : null
        try {
          await send('task.update', { id: selected.id, task: spec, expected_version: selected.version })
          onClose()
        } catch (failure) {
          setError(failure.message)
        }
      }
      const clearTimes = async () => {
        if (!selected) return
        if (!window.confirm(`清除「${selected.title}」的期望完成、必须完成与最早开始限制？`)) return
        setError('')
        const spec = taskSpec(selected)
        delete spec.target_due
        delete spec.hard_deadline
        delete spec.not_before
        try {
          await send('task.update', { id: selected.id, task: spec, expected_version: selected.version })
          onClose()
        } catch (failure) {
          setError(failure.message)
        }
      }
      const ambiguityChoices = (resolution, choiceKey) => {
        if (!resolution?.ambiguous) return null
        const current = form[choiceKey]
        return h('div', { className: 'zw-time-choices' }, resolution.instants.map((instant, index) =>
          h('label', { className: 'zw-radio-line', key: instant },
            h('input', {
              type: 'radio',
              name: `${selected?.id ?? 'time'}-${choiceKey}`,
              checked: current === instant,
              onChange: () => setForm({ ...form, [choiceKey]: instant }),
            }),
            `选择${index === 0 ? '较早' : '较晚'}时刻：${new Date(instant).toISOString()}（${wallClockText(instant, timeZone)} ${timeZone}）`,
          )))
      }
      const preview = selected ? h(Section, { title: '写回预览（保存前）' },
        h('div', { className: 'zw-time-preview' },
          h('div', null, h('span', null, '工作空间时区'), h('strong', null, zoneValid ? `${timeZone}（IANA）` : `无效：${timeZone}`)),
          h('div', null, h('span', null, '当地时间'), h('strong', null, `${form.date || '—'} ${form.time || '（仅日期，无钟点）'}`)),
          h('div', null, h('span', null, '换算 UTC'),
            h('strong', { className: 'zw-mono' },
              hasTime && dueInstant != null ? new Date(dueInstant).toISOString()
                : form.strength === 'hard' && dateOnlyHardInstant != null
                  ? `${new Date(dateOnlyHardInstant + 59_000).toISOString()}（当地当日结束 23:59:59）`
                  : '仅日期，Core 不保存钟点')),
          h('div', null, h('span', null, '写回字段'),
            h('strong', null, hasTime
              ? `due: { at, strength: '${form.strength}' } → ${form.strength === 'hard' ? 'hard_deadline' : 'target_due（仅保留 UTC 日期）'}`
              : `due: { date: '${form.date}', strength: '${form.strength}' } → ${form.strength === 'hard' ? 'hard_deadline（当地 23:59:59）' : 'target_due'}`)),
          h('div', null, h('span', null, '最早开始 not_before'),
            h('strong', { className: 'zw-mono' }, notBeforeInstant != null ? new Date(notBeforeInstant).toISOString() : '未设置')),
        ),
        h('dl', { className: 'zw-kv' },
          h('dt', null, '当前期望完成'), h('dd', null, selected.target_due ?? '未设置'),
          h('dt', null, '当前必须完成'), h('dd', null, selected.hard_deadline ? `${dateTimeFor(selected.hard_deadline)}（${wallClockText(new Date(selected.hard_deadline).getTime(), timeZone)} ${timeZone}）` : '未设置'),
          h('dt', null, '当前最早开始'), h('dd', null, selected.not_before ? `${dateTimeFor(selected.not_before)}（${wallClockText(new Date(selected.not_before).getTime(), timeZone)} ${timeZone}）` : '未设置'),
        ),
        ambiguityChoices(dueResolution, 'dueChoice'),
        ambiguityChoices(notBeforeResolution, 'notBeforeChoice'),
        softDateShift ? h('div', { className: 'zw-alert zw-alert-warn' },
          `软约束只保留日期，且 Core 由 UTC 日期推导：将保存为 ${softUtcDate}，与当地 ${form.date} 不一致。需要准确当地日期时请只保存日期或改用硬约束。`) : null,
        orderingError ? h('div', { className: 'zw-alert zw-alert-error' }, orderingError) : null,
        h('p', { className: 'zw-fineprint' }, '浏览器只用工作空间时区换算并显示 UTC；不会修改浏览器或宿主时区。保存后由 Core 校验版本与关系。'),
      ) : h(Empty, null, '先选择一个任务，再设置时间。')
      const taskPicker = !task
        ? h(Field, { label: '任务', wide: true }, h('select', {
            value: pickedId,
            'data-testid': 'time-task',
            onChange: event => setPickedId(event.target.value),
          }, h('option', { value: '' }, '选择任务…'), (tasks ?? []).map(candidate =>
            h('option', { key: candidate.id, value: candidate.id }, `${candidate.title} · r${candidate.version}`))))
        : h('p', { className: 'zw-fineprint' }, `写回任务：${selected?.title ?? ''} · v${selected?.version ?? '—'}`)
      return h(Modal, {
        title: '时间与时区编辑',
        subtitle: '日期、当地钟点与绝对时刻分别处理；保存写回完整任务规格。',
        onClose,
        wide: true,
        footer: h(React.Fragment, null,
          h(Button, { onClick: onClose }, COPY.cancel),
          selected ? h(Button, { onClick: clearTimes, disabled: busy }, '清除时间限制') : null,
          h(Button, { tone: 'primary', onClick: save, disabled: busy || !selected, testid: 'time-save' }, COPY.save),
        ),
      },
      error ? h('div', { className: 'zw-alert zw-alert-error' }, error) : null,
      !zoneValid ? h('div', { className: 'zw-alert zw-alert-error' }, `工作空间时区「${timeZone}」不是可用的 IANA 时区；请先在设置中修正，避免时间被错误换算。`) : null,
      day ? h('p', { className: 'zw-fineprint', 'data-testid': 'time-date-context' }, `来自日程日期列 ${day}（本机时区）。这里只设置任务完成日期，不创建分配、不移动锁定安排，也不会开始专注。没有已有截止的任务默认使用相同日期标签，按工作空间时区 ${timeZone} 解释，不是跨时区时刻换算；已有截止与最早开始限制保留，请核对后保存。`) : null,
      taskPicker,
      selected ? h(React.Fragment, null,
        h('div', { className: 'zw-grid2' },
          h(Field, { label: '日期', hint: '必填；按工作空间时区解释' }, h('input', {
            type: 'date', value: form.date, 'data-testid': 'time-date',
            onChange: event => setForm({ ...form, date: event.target.value, dueChoice: null }),
          })),
          h(Field, { label: '钟点（可选）', hint: '留空表示只保存日期，不保存钟点' }, h('div', { className: 'zw-inline-field' },
            h('input', {
              type: 'time', value: form.time, 'data-testid': 'time-clock',
              onChange: event => setForm({ ...form, time: event.target.value, dueChoice: null }),
            }),
            form.time ? h('button', {
              type: 'button', className: 'zw-icon-btn', 'aria-label': '清除钟点',
              onClick: () => setForm({ ...form, time: '', dueChoice: null }),
            }, '×') : null,
          )),
          h(Field, { label: '截止强度', hint: '软约束是期望日期，硬约束是必须满足的截止' }, h('select', {
            value: form.strength, 'data-testid': 'time-strength',
            onChange: event => setForm({ ...form, strength: event.target.value }),
          }, h('option', { value: 'soft' }, DUE_STRENGTH_LABELS.soft), h('option', { value: 'hard' }, DUE_STRENGTH_LABELS.hard))),
          h(Field, { label: '最早开始（可选）', hint: '绝对时刻；按工作空间时区解释' }, h('div', { className: 'zw-inline-field' },
            h('input', {
              type: 'datetime-local', value: form.notBefore, 'data-testid': 'time-notbefore',
              onChange: event => setForm({ ...form, notBefore: event.target.value, notBeforeChoice: null }),
            }),
            form.notBefore ? h('button', {
              type: 'button', className: 'zw-icon-btn', 'aria-label': '清除最早开始',
              onClick: () => setForm({ ...form, notBefore: '', notBeforeChoice: null }),
            }, '×') : null,
          )),
        ),
        h('p', { className: 'zw-fineprint' }, `工作空间时区：${zoneValid ? timeZone : `${timeZone}（无效）`}。跨日只按此 IANA 时区解释，历史时刻以 UTC 保存。`),
        preview,
        dueResolution?.missing || notBeforeResolution?.missing
          ? h('div', { className: 'zw-alert zw-alert-error' }, `当地时刻在 ${timeZone} 的夏令时跳变中不存在；请换一个钟点，或只保存日期。`)
          : null,
      ) : null,
      )
    }


    function GoalEditor({ goal, onClose, send, busy }) {
      const [form, setForm] = useState({
        title: goal?.title ?? '',
        criteria: goal?.success_criteria ?? '',
        targetDue: goal?.target_due ?? '',
        description: goal?.description ?? '',
      })
      const [error, setError] = useState('')
      const save = async () => {
        setError('')
        if (!form.title.trim()) return setError('请填写目标标题')
        const spec = {
          title: form.title.trim(),
          description: form.description.trim() || null,
          success_criteria: form.criteria.trim() || null,
          target_due: form.targetDue || null,
          hard_deadline: goal?.hard_deadline ?? null,
        }
        try {
          if (goal) await send('goal.update', { id: goal.id, goal: spec })
          else await send('goal.create', { goal: spec })
          onClose()
        } catch (failure) {
          setError(failure.message)
        }
      }
      return h(Modal, { title: goal ? '编辑目标' : '新建目标', onClose,
        footer: h(React.Fragment, null,
          h(Button, { onClick: onClose }, COPY.cancel),
          h(Button, { tone: 'primary', onClick: save, disabled: busy }, COPY.save)) },
        error ? h('div', { className: 'zw-alert zw-alert-error' }, error) : null,
        h(Field, { label: '目标', wide: true }, h('input', { value: form.title, autoFocus: true,
          onChange: event => setForm({ ...form, title: event.target.value }) })),
        h(Field, { label: '成功标准', wide: true }, h('textarea', { rows: 3, value: form.criteria,
          onChange: event => setForm({ ...form, criteria: event.target.value }) })),
        h(Field, { label: '期望完成' }, h('input', { type: 'date', value: form.targetDue,
          onChange: event => setForm({ ...form, targetDue: event.target.value }) })),
        h(Field, { label: '补充说明', wide: true }, h('textarea', { rows: 2, value: form.description,
          onChange: event => setForm({ ...form, description: event.target.value }) })),
      )
    }

    function ProjectEditor({ project, goals, goalId, onClose, send, busy }) {
      const [form, setForm] = useState({
        title: project?.title ?? '',
        scope: project?.scope ?? '',
        goal: project?.goal_id ?? goalId ?? '',
      })
      const [error, setError] = useState('')
      const save = async () => {
        setError('')
        const spec = { title: form.title.trim(), scope: form.scope.trim() || null, goal_id: form.goal || null }
        if (!spec.title) return setError('请填写项目标题')
        try {
          if (project) await send('project.update', { id: project.id, project: spec })
          else await send('project.create', { project: spec })
          onClose()
        } catch (failure) {
          setError(failure.message)
        }
      }
      return h(Modal, { title: project ? '编辑项目' : '新建项目', onClose,
        footer: h(React.Fragment, null,
          h(Button, { onClick: onClose }, COPY.cancel),
          h(Button, { tone: 'primary', onClick: save, disabled: busy }, COPY.save)) },
        error ? h('div', { className: 'zw-alert zw-alert-error' }, error) : null,
        h(Field, { label: '项目标题', wide: true }, h('input', { value: form.title, autoFocus: true,
          onChange: event => setForm({ ...form, title: event.target.value }) })),
        h(Field, { label: '范围', wide: true }, h('textarea', { rows: 3, value: form.scope,
          onChange: event => setForm({ ...form, scope: event.target.value }) })),
        h(Field, { label: '所属目标' }, h('select', { value: form.goal,
          onChange: event => setForm({ ...form, goal: event.target.value }) },
          h('option', { value: '' }, '无'), goals.map(goal => h('option', { key: goal.id, value: goal.id }, goal.title))),
        ),
      )
    }

    const CAPTURE_DRAFT_KEY = 'zodo.capture.draft.v1'

    function readCaptureDraft() {
      try {
        return window.localStorage.getItem(CAPTURE_DRAFT_KEY) ?? ''
      } catch {
        return ''
      }
    }

    function writeCaptureDraft(value) {
      try {
        if (value) window.localStorage.setItem(CAPTURE_DRAFT_KEY, value)
        else window.localStorage.removeItem(CAPTURE_DRAFT_KEY)
      } catch {
        /* storage can be unavailable; the textarea remains the draft */
      }
    }

    function CaptureEditor({ onClose, send, busy }) {
      const [restored] = useState(readCaptureDraft)
      const [text, setText] = useState(restored)
      const [error, setError] = useState('')
      const [draftKept, setDraftKept] = useState(Boolean(restored))
      const submit = async () => {
        if (!text.trim()) return setError('先写下要收集的内容')
        try {
          await send('capture.collect', { text })
          writeCaptureDraft('')
          onClose()
        } catch (failure) {
          writeCaptureDraft(text)
          setDraftKept(true)
          setError(`${failure.message}；草稿只保存在本机，尚未提交到 Core。`)
        }
      }
      return h(Modal, { title: '快速收集', subtitle: '原文先进入收集箱；不会自动变成任务。', onClose,
        footer: h(React.Fragment, null,
          h(Button, { onClick: onClose }, COPY.cancel),
          h(Button, { tone: 'primary', onClick: submit, disabled: busy, testid: 'capture-save' }, '保存原文')) },
        restored ? h('div', { className: 'zw-alert zw-alert-warn' }, '已恢复本机未提交草稿；它没有出现在 Core 收集箱里，直到保存成功。') : null,
        error ? h('div', { className: 'zw-alert zw-alert-error' }, error) : null,
        h('textarea', { className: 'zw-capture-input', rows: 5, autoFocus: true, value: text,
          placeholder: '例如：明天下午提醒我联系印刷店',
          'data-testid': 'capture-input',
          onChange: event => {
            const value = event.target.value
            setText(value)
            writeCaptureDraft(value)
            setDraftKept(Boolean(value))
          },
          onKeyDown: event => {
            if (event.key === 'Enter' && (event.metaKey || event.ctrlKey) && !event.nativeEvent.isComposing) submit()
          },
        }),
        draftKept ? h('p', { className: 'zw-fineprint', 'data-testid': 'capture-draft-state' }, '本机草稿：未提交（不是 Core 事实）。') : null,
      )
    }

    function InboxConvert({ entry, snapshot, onClose, send, busy }) {
      const [title, setTitle] = useState(entry.text.slice(0, 80))
      const [estimate, setEstimate] = useState(30)
      const [makeGoal, setMakeGoal] = useState(false)
      const [goalTitle, setGoalTitle] = useState('')
      const [makeProject, setMakeProject] = useState(false)
      const [projectTitle, setProjectTitle] = useState('')
      const [error, setError] = useState('')
      const submit = async () => {
        setError('')
        const tasks = title.trim() ? [{
          title: title.trim(),
          estimate_minutes: Number(estimate) > 0 ? Number(estimate) : null,
          priority: 2,
        }] : []
        if (!tasks.length && !makeGoal && !makeProject) return setError('至少创建一个目标对象')
        try {
          await send('capture.convert', {
            id: entry.id,
            tasks,
            goal: makeGoal && goalTitle.trim() ? { title: goalTitle.trim() } : null,
            project: makeProject && projectTitle.trim() ? { title: projectTitle.trim() } : null,
          })
          onClose()
        } catch (failure) {
          setError(failure.message)
        }
      }
      return h(Modal, { title: '处理收集内容', subtitle: '原文保留，明确提交后才生成正式工作对象。', onClose, wide: true,
        footer: h(React.Fragment, null,
          h(Button, { onClick: onClose }, COPY.cancel),
          h(Button, { tone: 'primary', onClick: submit, disabled: busy }, '提交转换')) },
        error ? h('div', { className: 'zw-alert zw-alert-error' }, error) : null,
        h('div', { className: 'zw-quote' }, entry.text),
        h(Field, { label: '任务标题', wide: true }, h('input', { value: title, autoFocus: true,
          onChange: event => setTitle(event.target.value) })),
        h(Field, { label: '预计分钟' }, h('input', { type: 'number', min: 1, value: estimate,
          onChange: event => setEstimate(event.target.value) })),
        h('label', { className: 'zw-check-line' },
          h('input', { type: 'checkbox', checked: makeGoal, onChange: event => setMakeGoal(event.target.checked) }),
          '同时创建目标',
        ),
        makeGoal ? h(Field, { label: '目标标题' }, h('input', { value: goalTitle,
          onChange: event => setGoalTitle(event.target.value) })) : null,
        h('label', { className: 'zw-check-line' },
          h('input', { type: 'checkbox', checked: makeProject, onChange: event => setMakeProject(event.target.checked) }),
          '同时创建项目',
        ),
        makeProject ? h(Field, { label: '项目标题' }, h('input', { value: projectTitle,
          onChange: event => setProjectTitle(event.target.value) })) : null,
      )
    }

    function ClarifyEditor({ entry, onClose, send, busy }) {
      const [question, setQuestion] = useState('这个事项的期望完成时间或限制是什么？')
      const [error, setError] = useState('')
      const submit = async () => {
        if (!question.trim()) return setError('请写下要澄清的问题')
        try {
          await send('capture.clarify', { id: entry.id, question: question.trim(), answer: null })
          onClose()
        } catch (failure) {
          setError(failure.message)
        }
      }
      return h(Modal, { title: '需要补充信息', subtitle: '澄清问题会与原文一起保留。', onClose,
        footer: h(React.Fragment, null,
          h(Button, { onClick: onClose }, COPY.cancel),
          h(Button, { tone: 'primary', onClick: submit, disabled: busy }, '保存问题')) },
        error ? h('div', { className: 'zw-alert zw-alert-error' }, error) : null,
        h(Field, { label: '问题', wide: true }, h('textarea', { rows: 3, value: question,
          onChange: event => setQuestion(event.target.value) })),
      )
    }

    // BEGIN APPOINTMENT DEFAULTS
    function appointmentDefaults(day, timeZone, displayZone, now = Date.now()) {
      let start
      let end
      if (day) {
        const first = resolveWallTime(day, '09:00', displayZone)
        const last = resolveWallTime(day, '10:00', displayZone)
        // Never guess on missing or repeated display-zone clocks.
        if (first.instants.length !== 1 || last.instants.length !== 1) return { start: '', end: '' }
        start = first.instants[0]
        end = last.instants[0]
      } else {
        const next = new Date(now + 60 * 60 * 1000)
        next.setMinutes(0, 0, 0)
        start = next.getTime()
        end = start + 60 * 60 * 1000
      }
      return { start: wallClockInput(start, timeZone), end: wallClockInput(end, timeZone) }
    }
    // END APPOINTMENT DEFAULTS

    function CommitmentEditor({ day, snapshot, onClose, send, busy }) {
      const timeZone = workspaceTimezone(snapshot)
      const displayZone = Intl.DateTimeFormat().resolvedOptions().timeZone
      const zoneValid = isKnownTimezone(timeZone)
      const [form, setForm] = useState(() => ({
        title: '固定预约',
        ...(zoneValid ? appointmentDefaults(day, timeZone, displayZone) : { start: '', end: '' }),
        startChoice: null, endChoice: null, locked: true,
      }))
      const resolutions = Object.fromEntries(['start', 'end'].map(key => [key,
        zoneValid && form[key] ? resolveWallTime(form[key].slice(0, 10), form[key].slice(11, 16), timeZone) : { instants: [] },
      ]))
      const submitting = useRef(false)
      const closeIfIdle = () => { if (!submitting.current && !busy) onClose() }
      const [error, setError] = useState('')
      const submit = async () => {
        if (submitting.current || busy) return
        if (!form.title.trim()) return setError('请填写标题')
        if (!zoneValid) return setError('请先修正工作空间 IANA 时区')
        for (const key of ['start', 'end']) {
          if (!resolutions[key].instants.length) return setError('开始或结束时间缺失或在此时区不存在，请重新选择')
          if (resolutions[key].instants.length > 1 && !resolutions[key].instants.includes(form[`${key}Choice`])) return setError('当地钟点重复，请明确选择 UTC 时刻')
        }
        const startInstant = form.startChoice ?? resolutions.start.instants[0]
        const endInstant = form.endChoice ?? resolutions.end.instants[0]
        if (endInstant <= startInstant) return setError('结束时间必须晚于开始时间')
        submitting.current = true
        try {
          await send('commitment.create', { commitment: {
            title: form.title.trim(),
            start: new Date(startInstant).toISOString(),
            end: new Date(endInstant).toISOString(),
            locked: form.locked,
          } })
          onClose()
        } catch (failure) {
          setError(failure.message)
        } finally {
          submitting.current = false
        }
      }
      return h(Modal, { title: '新建固定预约', onClose: closeIfIdle,
        footer: h(React.Fragment, null,
          h(Button, { onClick: closeIfIdle, disabled: busy }, COPY.cancel),
          h(Button, { tone: 'primary', onClick: submit, disabled: busy }, COPY.save)) },
        busy ? h('p', { className: 'zw-fineprint', role: 'status' }, '保存中，请等待完成后关闭。') : null,
        error ? h('div', { className: 'zw-alert zw-alert-error' }, error) : null,
        h('p', { className: 'zw-fineprint' }, `输入时区：${timeZone}。日程显示时区：${displayZone}。${day ? `由所选日 ${day} 的本机 09:00–10:00 转换为初值，可编辑；不代表该时段可预约。` : '确认时间后保存，不会自动创建。'}`),
        ['start', 'end'].map(key => resolutions[key].instants.length > 1 ? h(Field, { key, label: `${key === 'start' ? '开始' : '结束'}钟点重复，请选择 UTC 时刻` },
          h('select', { value: form[`${key}Choice`] ?? '', onChange: event => setForm({ ...form, [`${key}Choice`]: event.target.value === '' ? null : Number(event.target.value) }) },
            h('option', { value: '' }, '请选择'),
            resolutions[key].instants.map(instant => h('option', { key: instant, value: instant }, new Date(instant).toISOString())),
          )) : null),
        h(Field, { label: '标题', wide: true }, h('input', { value: form.title,
          onChange: event => setForm({ ...form, title: event.target.value }) })),
        h(Field, { label: '开始' }, h('input', { type: 'datetime-local', value: form.start,
          onChange: event => setForm({ ...form, start: event.target.value, startChoice: null }) })),
        h(Field, { label: '结束' }, h('input', { type: 'datetime-local', value: form.end,
          onChange: event => setForm({ ...form, end: event.target.value, endChoice: null }) })),
        h('label', { className: 'zw-check-line' },
          h('input', { type: 'checkbox', checked: form.locked, onChange: event => setForm({ ...form, locked: event.target.checked }) }),
          '锁定，不被自动移动',
        ),
      )
    }

    // Strict quick scheduling creates a candidate only; normal approval owns commit.
    // Hints are presentation only. Core diagnostics remain authoritative; never
    // reflect arbitrary diagnostic values (which may contain private titles).
    function scheduleRecovery(failure = {}) {
      const code = String(failure.code ?? '').toUpperCase()
      const message = String(failure.message ?? '').toLowerCase()
      if (code === 'UNSUPPORTED_OPERATION' || /unknown (operation|variant)|unsupported/.test(message)) return 'Core 不支持严格安排，请升级后重新生成预览；不会使用宽松接口。'
      if (['REVISION_CONFLICT', 'POLICY_CHANGED', 'APPROVAL_EXPIRED'].includes(code) || /task changed/.test(message)) return '数据、策略或批准依据已变化；刷新数据并核对保留的输入，再重新生成预览，不能重用旧批准。'
      if (code === 'PERMISSION_DENIED') return '当前权限不足；检查授权后再生成预览，不会绕过权限。'
      if (code === 'NOT_FOUND') return '所选任务或容量已不存在；刷新后重新选择，再生成预览。'
      if (/start in the future/.test(message)) return '开始时间已到或已过；选择未来时段，再生成预览。'
      if (/predecessor|dependency/.test(message)) return '前置工作未满足；先核实依赖完成情况或安排前置工作，再重新生成预览。'
      if (/unblocked leaf|closed or archived/.test(message)) return '请选择未关闭、未归档且未手动阻塞的叶任务；不会替你解除阻塞。'
      if (/task time constraints/.test(message)) return '所选时间违反最早开始或硬截止；调整时段，不会自动放宽任务限制。'
      if (/workspace availability|timezone/.test(message)) return '检查工作空间时区、硬工作窗口与周末规则，选择允许的时段后重新预览。'
      if (/overlap|external busy/.test(message)) return '所选时段与已有安排或忙碌区间冲突；改选时段，不会移动已有安排。'
      if (/capacity/.test(message)) return '检查所选容量块的时间范围和剩余预算；换一个已有容量块或缩短时段。'
      if (/remaining|estimate_minutes/.test(message)) return '检查任务剩余分钟及已有分配；补齐估时或缩短安排后重新预览。'
      return '未得到有效严格预览。输入已保留；请核实 Core 状态与提案记录，再手动重新生成预览。不会自动重试或提交。'
    }

    // BEGIN PURE FIRST-SCHEDULE GUIDANCE
    function manualScheduleTasks(snapshot) {
      const allTasks = snapshot.tasks ?? []
      const parents = new Set(allTasks.filter(task => !task.archived).map(task => task.parent_id))
      return allTasks.filter(task => !task.archived && !task.blocked_manual &&
        !['done', 'cancelled'].includes(task.status) && !parents.has(task.id))
    }
    function firstScheduleStep(snapshot, now, timeZone) {
      if (!Array.isArray(snapshot?.allocations) || snapshot.allocations.length ||
          !Array.isArray(snapshot.tasks) || !Array.isArray(snapshot.capacity_blocks) ||
          !Number.isFinite(now)) return null
      const tasks = manualScheduleTasks(snapshot)
      if (!tasks.some(task => (task.remaining_minutes ?? task.estimate_minutes ?? 0) > 0)) {
        const needsEstimate = tasks.find(task => task.remaining_minutes == null && task.estimate_minutes == null)
        return needsEstimate ? { kind: 'estimate', task: needsEstimate } : { kind: 'task' }
      }
      const next = snapshot.capacity_blocks.reduce((best, block) => {
        const start = typeof block.start === 'string' ? Date.parse(block.start) : NaN
        const end = typeof block.end === 'string' ? Date.parse(block.end) : NaN
        if (!Number.isFinite(start) || !Number.isFinite(end) || end <= Math.max(now, start) ||
            !(block.capacity_minutes > 0)) return best
        return Math.min(best, Math.max(start, now))
      }, Infinity)
      if (!Number.isFinite(next)) return { kind: 'capacity' }
      if (!isKnownTimezone(timeZone)) return { kind: 'timezone' }
      const day = wallClockInput(next, timeZone).slice(0, 10)
      return agendaDate(day) ? { kind: 'schedule', day } : { kind: 'timezone' }
    }
    // END PURE FIRST-SCHEDULE GUIDANCE

    function schedulePickerOptions(snapshot, form, timeZone) {
      const tasks = manualScheduleTasks(snapshot)
      const first = String(form.start ?? '').slice(0, 10)
      const last = String(form.end ?? '').slice(0, 10)
      const dateKnown = Boolean(agendaDate(first) && agendaDate(last) && first <= last && isKnownTimezone(timeZone))
      const capacities = (snapshot.capacity_blocks ?? []).filter(block => {
        const start = typeof block.start === 'string' ? Date.parse(block.start) : NaN
        const end = typeof block.end === 'string' ? Date.parse(block.end) : NaN
        if (!Number.isFinite(start) || !Number.isFinite(end) || end <= start) return false
        if (!dateKnown) return true // Do not clear a selection while the date is incomplete.
        // Compare workspace civil dates, not browser-local dates or UTC prefixes.
        // end - 1 preserves half-open midnight endings; multi-day blocks remain visible.
        return wallClockInput(start, timeZone).slice(0, 10) <= last &&
          wallClockInput(end - 1, timeZone).slice(0, 10) >= first
      })
      return { tasks, capacities, dateKnown }
    }

    function QuickSchedule({ day, snapshot, send, busy, onClose, setModal }) {
      const timeZone = workspaceTimezone(snapshot)
      const zoneValid = isKnownTimezone(timeZone)
      const pending = useRef(false)
      const [form, setForm] = useState({ task: '', capacity: '', start: `${day}T09:00`, end: `${day}T10:00`, startChoice: null, endChoice: null })
      const [error, setError] = useState('')
      const updateForm = next => {
        if (pending.current || busy) return
        const options = schedulePickerOptions(snapshot, next, timeZone)
        const datesChanged = next.start.slice(0, 10) !== form.start.slice(0, 10) || next.end.slice(0, 10) !== form.end.slice(0, 10)
        setForm(datesChanged && options.dateKnown && next.capacity && !options.capacities.some(block => block.id === next.capacity)
          ? { ...next, capacity: '' } : next)
      }
      const { tasks, capacities, dateKnown } = schedulePickerOptions(snapshot, form, timeZone)
      const selected = tasks.find(task => task.id === form.task)
      const capacity = capacities.find(block => block.id === form.capacity)
      const resolved = Object.fromEntries(['start', 'end'].map(key => [key, zoneValid && form[key]
        ? resolveWallTime(form[key].slice(0, 10), form[key].slice(11, 16), timeZone) : { instants: [] }]))
      const instant = key => resolved[key].instants.length === 1 ? resolved[key].instants[0]
        : resolved[key].instants.includes(form[`${key}Choice`]) ? form[`${key}Choice`] : null
      const start = instant('start'), end = instant('end')
      const close = () => { if (!pending.current && !busy) onClose() }
      const propose = async () => {
        if (pending.current || busy) return
        setError('')
        if (!selected || !capacity) return setError('请选择任务和已有容量块；不会自动创建容量。')
        if (!zoneValid || start == null || end == null) return setError('时间缺失、不存在或存在夏令时歧义；请修正并明确选择时刻。')
        if (end <= start) return setError('结束必须晚于开始。')
        if (!Number.isInteger(selected.version) || selected.version < 1) return setError('任务版本无效，请刷新后重试。')
        pending.current = true
        try {
          const result = await send('proposal.create', {
            intent: `安排任务「${selected.title}」到容量「${capacity.title}」：${form.start} 至 ${form.end}（${timeZone}）；UTC ${new Date(start).toISOString()} 至 ${new Date(end).toISOString()}`,
            operations: [{ operation: 'allocation.schedule', payload: { allocation: {
              task_id: selected.id, expected_task_version: selected.version, capacity_block_id: capacity.id,
              start: new Date(start).toISOString(), end: new Date(end).toISOString(),
            } } }],
          })
          const proposal = result.data?.proposal
          if (!proposal || proposal.status !== 'validated') {
            const hints = (proposal?.diagnostics ?? []).slice(0, 4).map(scheduleRecovery)
            setError(`Core 未返回有效的严格安排提案，未安排任何时间。${[...new Set(hints.length ? hints : [scheduleRecovery()])].join(' ')} 输入已保留，修改后请生成新预览；不会使用宽松接口。`)
            return
          }
          setModal({ type: 'plan', proposal, plan: null, grant: null })
        } catch (failure) {
          setError(`严格安排预览失败。${scheduleRecovery(failure)} 输入已保留。`)
        } finally { pending.current = false }
      }
      return h(Modal, { title: '安排任务时段', onClose: close, wide: true,
        footer: h(React.Fragment, null,
          h(Button, { onClick: close, disabled: busy }, COPY.cancel),
          h(Button, { onClick: propose, disabled: busy, testid: 'schedule-slot-preview' }, '生成严格预览')) },
        error ? h('div', { className: 'zw-alert zw-alert-error', role: 'alert' }, error) : null,
        busy ? h('p', { role: 'status' }, '正在生成预览，请等待后关闭。') : null,
        h('p', { className: 'zw-fineprint' }, `所选日期标签 ${day} 来自本机日程；下面所有日期与钟点按工作空间时区 ${timeZone} 解释，不自动猜测跨时区对应时刻。请核对 UTC 预览。`),
        h('fieldset', { disabled: busy, style: { border: 0, padding: 0, margin: 0, minWidth: 0 } },
        h('legend', { className: 'zw-fineprint' }, '修改输入后须重新生成预览；保存中输入受保护'),
        h(Field, { label: '任务' }, h('select', { value: form.task, onChange: event => updateForm({ ...form, task: event.target.value }), 'data-testid': 'schedule-slot-task' },
          h('option', { value: '' }, '选择任务'), tasks.map(task => h('option', { key: task.id, value: task.id }, task.title)))),
        h(Field, { label: '已有容量块' }, h('select', { value: form.capacity, onChange: event => updateForm({ ...form, capacity: event.target.value }), 'data-testid': 'schedule-slot-capacity' },
          h('option', { value: '' }, '选择容量块'), capacities.map(block => h('option', { key: block.id, value: block.id }, `${block.title} · ${zoneValid ? wallClockText(Date.parse(block.start), timeZone) : block.start} 至 ${zoneValid ? wallClockText(Date.parse(block.end), timeZone) : block.end}`)))),
        !tasks.length ? h('p', { role: 'status' }, '没有可选的开放叶任务；请先创建叶任务，或在任务详情检查归档、完成和手动阻塞状态。') : null,
        !capacities.length ? h('p', { role: 'status' }, `所选工作空间日期范围没有候选容量块。请调整日期，或关闭此表单先创建容量，再回来选择。`) : null,
        h('p', { className: 'zw-fineprint', 'data-testid': 'schedule-picker-scope' }, dateKnown
          ? `容量候选按输入的开始至结束日期（${timeZone}）筛选，跨午夜容量保留；日期变化后不相交的选择会清空。候选不代表预算、依赖或冲突检查已通过。`
          : '日期或时区尚未完整，暂不按日期缩小容量候选；请补齐后选择。'),
        ['start', 'end'].map(key => h(Field, { key, label: key === 'start' ? '开始（工作空间时区）' : '结束（工作空间时区）' },
          h('input', { type: 'datetime-local', value: form[key], onChange: event => updateForm({ ...form, [key]: event.target.value, [`${key}Choice`]: null }) }),
          resolved[key].instants.length > 1 ? h('select', { value: form[`${key}Choice`] ?? '', onChange: event => updateForm({ ...form, [`${key}Choice`]: event.target.value === '' ? null : Number(event.target.value) }) },
            h('option', { value: '' }, '钟点重复，请选择 UTC 时刻'), resolved[key].instants.map(value => h('option', { key: value, value }, new Date(value).toISOString()))) : null,
        )),
        ),
        h('p', { 'data-testid': 'schedule-slot-utc' }, `UTC：${start == null ? '待确认' : new Date(start).toISOString()} 至 ${end == null ? '待确认' : new Date(end).toISOString()}`),
        h('p', { className: 'zw-fineprint' }, '生成预览可能保存提案元数据，但不会分配时间。只有检查后批准并收到 Core 提交成功回执才保存安排。严格检查任务、依赖、容量和冲突；不移动已有安排、不开始专注、不写外部日历。忙碌快照可能过期，不能保证实时外部可用性。'),
      )
    }

    function CapacityEditor({ onClose, send, busy }) {
      const date = `${addDays(todayIso(), 1)}T14:00`
      const [form, setForm] = useState({
        title: '专注时间',
        start: date,
        end: `${addDays(todayIso(), 1)}T16:00`,
        minutes: 120,
      })
      const [error, setError] = useState('')
      const submit = async () => {
        if (!form.title.trim()) return setError('请填写容量名称')
        try {
          await send('capacity.create', { capacity: {
            title: form.title.trim(),
            start: toIso(form.start),
            end: toIso(form.end),
            capacity_minutes: Number(form.minutes),
            locked: false,
          } })
          onClose()
        } catch (failure) {
          setError(failure.message)
        }
      }
      return h(Modal, { title: '增加可用容量', subtitle: '容量块是工作容器，任务分配会在其中扣除。', onClose,
        footer: h(React.Fragment, null,
          h(Button, { onClick: onClose }, COPY.cancel),
          h(Button, { tone: 'primary', onClick: submit, disabled: busy }, COPY.save)) },
        error ? h('div', { className: 'zw-alert zw-alert-error' }, error) : null,
        h(Field, { label: '名称', wide: true }, h('input', { value: form.title,
          onChange: event => setForm({ ...form, title: event.target.value }) })),
        h(Field, { label: '开始' }, h('input', { type: 'datetime-local', value: form.start,
          onChange: event => setForm({ ...form, start: event.target.value }) })),
        h(Field, { label: '结束' }, h('input', { type: 'datetime-local', value: form.end,
          onChange: event => setForm({ ...form, end: event.target.value }) })),
        h(Field, { label: '可用分钟' }, h('input', { type: 'number', min: 1, value: form.minutes,
          onChange: event => setForm({ ...form, minutes: event.target.value }) })),
      )
    }

    function ProgressEditor({ task, onClose, send, busy }) {
      const [minutes, setMinutes] = useState(15)
      const [remaining, setRemaining] = useState(task.remaining_minutes ?? '')
      const [note, setNote] = useState('')
      const [error, setError] = useState('')
      const submit = async () => {
        if (!(Number(minutes) > 0)) return setError('投入分钟必须大于 0')
        try {
          await send('progress.record', {
            task_id: task.id,
            minutes: Number(minutes),
            remaining_minutes: remaining === '' ? null : Number(remaining),
            note: note.trim() || null,
          })
          onClose()
        } catch (failure) {
          setError(failure.message)
        }
      }
      return h(Modal, { title: '记录本次进度', subtitle: '这是执行事实，不会自动把任务标记完成。', onClose,
        footer: h(React.Fragment, null,
          h(Button, { onClick: onClose }, COPY.cancel),
          h(Button, { tone: 'primary', onClick: submit, disabled: busy }, '记录')) },
        error ? h('div', { className: 'zw-alert zw-alert-error' }, error) : null,
        h(Field, { label: '本次投入分钟' }, h('input', { type: 'number', min: 1, value: minutes, autoFocus: true,
          onChange: event => setMinutes(event.target.value) })),
        h(Field, { label: '更新剩余分钟（可选）' }, h('input', { type: 'number', min: 0, value: remaining,
          onChange: event => setRemaining(event.target.value) })),
        h(Field, { label: '备注（可选）', wide: true }, h('input', { value: note,
          onChange: event => setNote(event.target.value) })),
      )
    }

    function DependencyEditor({ task, snapshot, onClose, send, busy }) {
      const [predecessor, setPredecessor] = useState('')
      const [error, setError] = useState('')
      const submit = async () => {
        if (!predecessor) return setError('选择一个前置任务')
        try {
          await send('dependency.add', { predecessor_id: predecessor, successor_id: task.id })
          onClose()
        } catch (failure) {
          setError(failure.message)
        }
      }
      return h(Modal, { title: '添加依赖', subtitle: '前置任务完成后，当前任务才可被安排。', onClose,
        footer: h(React.Fragment, null,
          h(Button, { onClick: onClose }, COPY.cancel),
          h(Button, { tone: 'primary', onClick: submit, disabled: busy }, '添加')) },
        error ? h('div', { className: 'zw-alert zw-alert-error' }, error) : null,
        h(Field, { label: '前置任务' }, h('select', { value: predecessor,
          onChange: event => setPredecessor(event.target.value) },
          h('option', { value: '' }, '选择…'),
          (snapshot.tasks ?? []).filter(candidate => candidate.id !== task.id).map(candidate =>
            h('option', { key: candidate.id, value: candidate.id }, candidate.title)),
        )),
      )
    }

    function ReminderEditor({ task, onClose, send, busy }) {
      const [fireAt, setFireAt] = useState(localDateTimeInput(new Date(Date.now() + 60 * 60 * 1000)))
      const [message, setMessage] = useState(task ? `提醒：${task.title}` : '提醒')
      const [error, setError] = useState('')
      const submit = async () => {
        if (!message.trim()) return setError('请填写提醒内容')
        try {
          await send('reminder.create', {
            task_id: task?.id ?? null,
            message: message.trim(),
            fire_at: toIso(fireAt),
            dedupe_key: newId('rem'),
          })
          onClose()
        } catch (failure) {
          setError(failure.message)
        }
      }
      return h(Modal, { title: '设置提醒意图', subtitle: 'Core 持久化提醒；实际系统通知由发布平台单独验证。', onClose,
        footer: h(React.Fragment, null,
          h(Button, { onClick: onClose }, COPY.cancel),
          h(Button, { tone: 'primary', onClick: submit, disabled: busy }, COPY.save)) },
        error ? h('div', { className: 'zw-alert zw-alert-error' }, error) : null,
        h(Field, { label: '提醒时间' }, h('input', { type: 'datetime-local', value: fireAt, autoFocus: true,
          onChange: event => setFireAt(event.target.value) })),
        h(Field, { label: '提醒内容', wide: true }, h('input', { value: message,
          onChange: event => setMessage(event.target.value) })),
      )
    }

    function BusyImportEditor({ onClose, send, busy }) {
      const [text, setText] = useState(JSON.stringify({
        provider: 'manual',
        display_name: '手动忙碌快照',
        external_id: 'manual-busy',
        events: [{
          external_id: 'busy-1',
          title: '外部会议',
          start: new Date(Date.now() + 2 * 60 * 60 * 1000).toISOString(),
          end: new Date(Date.now() + 3 * 60 * 60 * 1000).toISOString(),
        }],
      }, null, 2))
      const [error, setError] = useState('')
      const submit = async () => {
        try {
          const parsed = JSON.parse(text)
          await send('calendar.import_busy', {
            source_id: null,
            provider: parsed.provider,
            display_name: parsed.display_name,
            external_id: parsed.external_id,
            events: parsed.events,
            replace_source: true,
          })
          onClose()
        } catch (failure) {
          setError(failure.message)
        }
      }
      return h(Modal, { title: '导入忙碌快照', subtitle: '外部事件只读，用于避免把任务排进已知忙碌时间。', onClose, wide: true,
        footer: h(React.Fragment, null,
          h(Button, { onClick: onClose }, COPY.cancel),
          h(Button, { tone: 'primary', onClick: submit, disabled: busy }, '导入')) },
        error ? h('div', { className: 'zw-alert zw-alert-error' }, error) : null,
        h(Field, { label: 'JSON 快照', wide: true }, h('textarea', { rows: 12, value: text,
          onChange: event => setText(event.target.value) })),
      )
    }

    function RecurrenceEditor({ onClose, send, busy }) {
      const [form, setForm] = useState({
        title: '每周回顾',
        start: todayIso(),
        frequency: 'weekly',
        interval: 1,
        weekdays: [1],
        estimate: 30,
        through: addDays(todayIso(), 42),
        endDate: '',
        maxOcc: '',
        afterDays: '',
        chain: false,
      })
      const [error, setError] = useState('')
      const toggleWeekday = day => setForm({
        ...form,
        weekdays: form.weekdays.includes(day) ? form.weekdays.filter(value => value !== day) : [...form.weekdays, day].sort(),
      })
      const submit = async () => {
        if (!form.title.trim()) return setError('请填写循环标题')
        try {
          await send('recurrence.create', {
            rule: {
              title: form.title.trim(),
              timezone: 'Asia/Shanghai',
              start_date: form.start,
              frequency: form.frequency,
              interval: Number(form.interval),
              weekdays: form.frequency === 'weekly' ? form.weekdays : [],
              estimate_minutes: Number(form.estimate) || null,
              priority: 2,
              end_date: form.endDate || null,
              max_occurrences: form.maxOcc === '' ? null : Number(form.maxOcc),
              after_completion_days: form.afterDays === '' ? null : Number(form.afterDays),
              depends_on_previous: form.chain,
            },
            through: form.through,
          })
          onClose()
        } catch (failure) {
          setError(failure.message)
        }
      }
      return h(Modal, { title: '新建循环工作', subtitle: '基础循环先生成有界实例；本次例外不改系列身份。', onClose, wide: true,
        footer: h(React.Fragment, null,
          h(Button, { onClick: onClose }, COPY.cancel),
          h(Button, { tone: 'primary', onClick: submit, disabled: busy }, '生成实例')) },
        error ? h('div', { className: 'zw-alert zw-alert-error' }, error) : null,
        h(Field, { label: '标题', wide: true }, h('input', { value: form.title,
          onChange: event => setForm({ ...form, title: event.target.value }) })),
        h(Field, { label: '开始日期' }, h('input', { type: 'date', value: form.start,
          onChange: event => setForm({ ...form, start: event.target.value }) })),
        h(Field, { label: '展开到' }, h('input', { type: 'date', value: form.through,
          onChange: event => setForm({ ...form, through: event.target.value }) })),
        h(Field, { label: '频率' }, h('select', { value: form.frequency,
          onChange: event => setForm({ ...form, frequency: event.target.value }) },
          h('option', { value: 'daily' }, '每天'), h('option', { value: 'weekly' }, '每周'), h('option', { value: 'monthly' }, '每月'))),
        h(Field, { label: '间隔' }, h('input', { type: 'number', min: 1, max: 52, value: form.interval,
          onChange: event => setForm({ ...form, interval: event.target.value }) })),
        h(Field, { label: '预计分钟' }, h('input', { type: 'number', min: 1, value: form.estimate,
          onChange: event => setForm({ ...form, estimate: event.target.value }) })),
        h(Field, { label: '\u7ed3\u675f\u65e5\u671f' }, h('input', { type: 'date', value: form.endDate, 'data-testid': 'recurrence-end-date',
          onChange: event => setForm({ ...form, endDate: event.target.value }) })),
        h(Field, { label: '\u6700\u5927\u6b21\u6570' }, h('input', { type: 'number', min: 1, max: 366, value: form.maxOcc, 'data-testid': 'recurrence-max-occ',
          onChange: event => setForm({ ...form, maxOcc: event.target.value }) })),
        h(Field, { label: '\u5b8c\u6210\u540e\u95f4\u9694\u5929' }, h('input', { type: 'number', min: 1, max: 365, value: form.afterDays, 'data-testid': 'recurrence-after-days',
          onChange: event => setForm({ ...form, afterDays: event.target.value }) })),
        form.frequency === 'weekly' ? h('div', { className: 'zw-weekdays' },
          ['一', '二', '三', '四', '五', '六', '日'].map((label, index) => {
            const day = index + 1
            return h(Pill, { key: label, active: form.weekdays.includes(day), onClick: () => toggleWeekday(day) }, label)
          }),
        ) : null,
      )
    }

    function TimeDetail({ kind, row, snapshot, onClose, send, busy, setModal }) {
      const title = kind === 'allocation' ? taskTitle(snapshot, row.task_id) : row.title
      const remove = async () => {
        try {
          await send(kind === 'commitment' ? 'commitment.remove' : kind === 'capacity' ? 'capacity.remove' : 'allocation.remove', { id: row.id })
          onClose()
        } catch {
          /* global error surface */
        }
      }
      const toggleLock = async () => {
        try {
          await send('allocation.set_locked', { id: row.id, locked: !row.locked })
          onClose()
        } catch {
          /* global error surface */
        }
      }
      return h(Modal, { title: '时间对象详情', subtitle: title, onClose,
        footer: h(React.Fragment, null,
          h(Button, { onClick: onClose }, COPY.close),
          kind === 'allocation' ? h(Button, {
            onClick: () => setModal({
              type: 'time',
              task: (snapshot.tasks ?? []).find(candidate => candidate.id === row.task_id) ?? null,
              tasks: snapshot.tasks ?? [],
            }),
            disabled: busy,
            testid: 'time-detail-task-time',
          }, '任务时间') : null,
          kind === 'allocation' ? h(Button, { onClick: toggleLock, disabled: busy }, row.locked ? '解除锁定' : '锁定分配') : null,
          h(Button, { tone: 'danger', onClick: remove, disabled: busy }, '删除'),
        ) },
        h('dl', { className: 'zw-kv' },
          h('dt', null, '类型'), h('dd', null, kind === 'commitment' ? '固定预约' : kind === 'capacity' ? '容量块' : '任务分配'),
          h('dt', null, '开始'), h('dd', null, dateTimeFor(row.start)),
          h('dt', null, '结束'), h('dd', null, dateTimeFor(row.end)),
          h('dt', null, '说明'), h('dd', null, kind === 'capacity' ? `${row.capacity_minutes} 分钟容量` : kind === 'allocation' ? `${row.minutes} 分钟` : row.title),
          kind === 'allocation' ? h(React.Fragment, null, h('dt', null, '锁定'), h('dd', null, row.locked ? '已锁定' : '可调整')) : null,
        ),
        h('p', { className: 'zw-fineprint' }, '删除只影响未来安排；已经开始的分配不会被计划器自动移动。'),
      )
    }

    function DuplicateSuggestions({ onClose, send, busy, refresh }) {
      const [groups, setGroups] = useState([])
      const [loading, setLoading] = useState(true)
      const [error, setError] = useState('')
      const load = async () => {
        setLoading(true)
        setError('')
        try {
          const result = await query('tasks.duplicates', {})
          setGroups(result.data?.groups ?? [])
        } catch (failure) {
          setError(failure.message)
        } finally {
          setLoading(false)
        }
      }
      useEffect(() => { void load() }, [])
      const merge = async (group, survivorId) => {
        try {
          await send('task.merge', {
            survivor_id: survivorId,
            duplicate_ids: group.tasks.filter(task => task.id !== survivorId).map(task => task.id),
          })
          await refresh()
          await load()
        } catch (failure) {
          setError(failure.message)
        }
      }
      return h(Modal, { title: '重复任务建议', subtitle: '按规范化标题分组；合并会保留依赖、笔记和审计，历史任务不会被删除。', onClose, wide: true,
        footer: h(Button, { onClick: onClose }, COPY.close) },
        error ? h('div', { className: 'zw-alert zw-alert-error' }, error) : null,
        loading ? h(Empty, null, '正在检查重复标题…') : groups.length ? h('div', null,
          groups.map(group => h(Section, { key: group.normalized, title: `${group.count} 个近似标题` },
            group.tasks.map(task => h('div', { className: 'zw-list-row', key: task.id },
              h('span', null, h('strong', null, task.title), h('small', { className: 'zw-muted' }, ` · ${task.id}`)),
              h(Button, { onClick: () => merge(group, task.id), disabled: busy, testid: `merge-${task.id}` }, '保留此项并合并'),
            )),
          )),
        ) : h(Empty, null, '没有发现重复标题。'),
        h('p', { className: 'zw-fineprint' }, '带分配的重复任务会被拒绝合并，避免把历史分配静默移走。'),
      )
    }

    function MeetingProposal({ onClose, send, busy, setView, report }) {
      const [form, setForm] = useState({
        title: '外部会议',
        date: todayIso(),
        start: '10:00',
        end: '11:00',
        externalId: `meeting-${newId('ext')}`,
      })
      const [error, setError] = useState('')
      const submit = async () => {
        setError('')
        if (!form.title.trim()) return setError('请填写会议标题')
        if (!form.date || !form.start || !form.end) return setError('请填写日期、开始和结束时间')
        const start = toIso(`${form.date}T${form.start}`)
        const end = toIso(`${form.date}T${form.end}`)
        if (new Date(end) <= new Date(start)) return setError('结束时间必须晚于开始时间')
        try {
          await send('proposal.create', {
            intent: `外部会议安排提案：${form.title.trim()}`,
            operations: [{
              operation: 'commitment.create',
              payload: {
                commitment: {
                  title: form.title.trim(),
                  start,
                  end,
                  external_id: form.externalId.trim() || null,
                },
              },
            }],
            assumptions: ['外部日历写入尚未完成；批准只创建本地固定预约并保留 external_id'],
            questions: [],
          })
          report('会议提案已生成，需要人工审批；外部日历尚未写入。')
          setView('proposals')
          onClose()
        } catch (failure) {
          setError(failure.message)
        }
      }
      return h(Modal, { title: '提议外部会议', subtitle: '生成待审批的固定预约提案；external_id 用于后续对账。', onClose,
        footer: h(React.Fragment, null,
          h(Button, { onClick: onClose }, COPY.cancel),
          h(Button, { tone: 'primary', onClick: submit, disabled: busy, testid: 'meeting-propose-submit' }, '生成提案')) },
        error ? h('div', { className: 'zw-alert zw-alert-error' }, error) : null,
        h(Field, { label: '会议标题', wide: true }, h('input', { value: form.title, autoFocus: true,
          onChange: event => setForm({ ...form, title: event.target.value }) })),
        h('div', { className: 'zw-grid2' },
          h(Field, { label: '日期' }, h('input', { type: 'date', value: form.date,
            onChange: event => setForm({ ...form, date: event.target.value }) })),
          h(Field, { label: '外部 ID（对账）' }, h('input', { value: form.externalId,
            onChange: event => setForm({ ...form, externalId: event.target.value }) })),
          h(Field, { label: '开始' }, h('input', { type: 'time', value: form.start,
            onChange: event => setForm({ ...form, start: event.target.value }) })),
          h(Field, { label: '结束' }, h('input', { type: 'time', value: form.end,
            onChange: event => setForm({ ...form, end: event.target.value }) })),
        ),
        h('p', { className: 'zw-fineprint' }, '这是提案，不是外部日历写入；批准只创建本地事实。重复 external_id 的幂等/失败行为仍待外部日历集成验证。'),
      )
    }

    function CapacitySuggestions({ onClose, send, busy }) {
      const [candidates, setCandidates] = useState([])
      const [loading, setLoading] = useState(true)
      const [error, setError] = useState('')
      const load = async () => {
        setLoading(true)
        setError('')
        try {
          const result = await query('capacity.candidates', {
            start: todayIso(),
            end: addDays(todayIso(), 6),
            min_minutes: 30,
            max_candidates: 12,
          })
          setCandidates(result.data?.candidates ?? [])
        } catch (failure) {
          setError(failure.message)
        } finally {
          setLoading(false)
        }
      }
      useEffect(() => { void load() }, [])
      const create = async candidate => {
        try {
          await send('capacity.create', {
            capacity: {
              title: `弹性容量 ${candidate.minutes} 分钟`,
              start: candidate.start,
              end: candidate.end,
              capacity_minutes: candidate.minutes,
              locked: false,
            },
          })
          setCandidates(current => current.filter(row => row.start !== candidate.start || row.end !== candidate.end))
        } catch (failure) {
          setError(failure.message)
        }
      }
      return h(Modal, { title: '建议容量窗口', subtitle: '只读建议；点击后才创建容量块，不会自动移动固定预约或锁定分配。', onClose, wide: true,
        footer: h(React.Fragment, null,
          h(Button, { onClick: onClose }, COPY.close),
          h(Button, { onClick: load, disabled: loading || busy, testid: 'capacity-candidates-refresh' }, '重新计算')) },
        error ? h('div', { className: 'zw-alert zw-alert-error' }, error) : null,
        loading ? h(Empty, null, '正在计算空闲窗口…') : candidates.length ? h('div', { className: 'zw-candidate-list' },
          candidates.map(candidate => h('div', { className: 'zw-list-row', key: `${candidate.start}-${candidate.end}` },
            h('span', null, `${dateTimeFor(candidate.start)} – ${timeFor(candidate.end)} · ${candidate.minutes} 分钟`),
            h(Button, { onClick: () => create(candidate), disabled: busy, testid: `capacity-create-${candidate.start}` }, '创建容量'),
          )),
        ) : h(Empty, null, '未来 7 天没有满足条件的空闲窗口。'),
        h('p', { className: 'zw-fineprint' }, '默认窗口 09:00–18:00（工作空间时区），排除已有承诺、容量、分配和忙碌日历；创建仍需要你的明确点击。'),
      )
    }

    function ProposalsView({ snapshot, setModal, busy, runs }) {
      const proposals = (snapshot.proposals ?? []).slice().sort((left, right) => {
        if (left.created_at !== right.created_at) return String(right.created_at).localeCompare(String(left.created_at))
        return (right.base_revision ?? 0) - (left.base_revision ?? 0)
      })
      const pending = proposals.filter(proposal => proposal.status === 'validated' || proposal.status === 'draft' || proposal.status === 'stale')
      const currentRevision = snapshot.workspace?.revision
      const isStale = proposal => currentRevision != null && proposal.base_revision !== currentRevision
      const apply = proposal => {
        const plan = proposal.preview?.plan?.plan ?? proposal.preview?.plan ?? null
        setModal({ type: 'plan', proposal, plan, grant: null })
      }
      return h('div', { className: 'zw-page' },
        h('header', { className: 'zw-page-head' },
          h('div', null, h('div', { className: 'zw-kicker' }, '候选与批准'), h('h1', null, '提案')),
        ),
        h('p', { className: 'zw-fineprint' }, '这里列出人工计划、外部 Agent 和后续模型提交的候选。预览不写正式事实；只有确认后才会提交。'),
        pending.length ? h('div', { className: 'zw-proposal-list' },
          pending.map(proposal => h('article', { className: 'zw-card', key: proposal.id },
            h('header', { className: 'zw-card-head' },
              h('div', null, h(Badge, { tone: proposal.status === 'validated' ? 'accent' : 'warn' }, proposal.status), isStale(proposal) ? h(Badge, { tone: 'warn', testid: `proposal-stale-${proposal.id}` }, '过期版本') : null, h('h3', null, proposal.intent)),
              h('span', { className: 'zw-muted zw-mono' }, proposal.id.slice(0, 14)),
            ),
            h('div', { className: 'zw-card-meta' },
              h('span', null, `依据 r${proposal.base_revision}`),
              isStale(proposal) ? h('span', null, `当前 r${currentRevision}`) : null,
              h('span', null, `策略 p${proposal.policy_revision}`),
              h('span', null, `${proposal.actor_kind} · ${proposal.actor_id}`),
            ),
            proposal.diagnostics?.length ? h('div', { className: 'zw-alert zw-alert-warn' },
              proposal.diagnostics.map(item => item.message).join(' · ')) : null,
            h('div', { className: 'zw-action-row' },
              h(Button, { tone: 'primary', onClick: () => apply(proposal), disabled: busy || proposal.status === 'draft' }, '检查并批准'),
              h(Button, {
                onClick: () => setModal({
                  type: 'data-reference',
                  proposal,
                  run: (runs ?? []).find(candidate => candidate.output?.proposal_id === proposal.id) ?? null,
                }),
                disabled: busy,
                testid: `proposal-data-reference-${proposal.id}`,
              }, '资料与外发'),
            ),
          )),
        ) : h(Empty, null, '没有待确认的提案。'),
        proposals.length ? h(Section, { title: '最近历史' },
          proposals.filter(proposal => !pending.includes(proposal)).slice(0, 12).map(proposal =>
            h('div', { className: 'zw-list-row', key: proposal.id },
              h('span', null, proposal.intent),
              h(Badge, { tone: proposal.status === 'applied' ? 'accent' : 'neutral' }, proposal.status),
            )),
        ) : null,
      )
    }

    function PlanModal({ proposal, plan, snapshotRevision, onClose, onApprove, busy, error, grant }) {
      const [localError, setLocalError] = useState('')
      const [blocked, setBlocked] = useState('')
      const attempt = useRef({ pending: false, outcome: '' })
      const closeIfIdle = () => { if (!attempt.current.pending && !busy) onClose() }
      const approveOnce = async () => {
        if (attempt.current.pending || attempt.current.outcome || busy || stale || grant) return
        attempt.current.pending = true
        setLocalError('')
        try {
          await onApprove()
          attempt.current.outcome = 'complete'
        } catch (failure) {
          const regenerate = ['VALIDATION_FAILED', 'REVISION_CONFLICT', 'POLICY_CHANGED', 'APPROVAL_EXPIRED', 'APPROVAL_REQUIRED', 'PERMISSION_DENIED', 'NOT_FOUND'].includes(failure.code)
          const outcome = regenerate ? 'regenerate' : 'uncertain'
          attempt.current.outcome = outcome
          setBlocked(outcome)
          setLocalError(regenerate
            ? 'Core 拒绝了本次操作。请核对提案状态、时间、版本和权限；修正后重新生成预览并批准，不能重试旧批准。'
            : '本次批准或提交结果不确定，可能已经生效。请关闭后刷新提案与日程核实结果；核实前不要重新提交或创建重复安排。')
        } finally { attempt.current.pending = false }
      }
      const stale = proposal.base_revision !== snapshotRevision
      const unplaced = plan?.unplaced ?? []
      const assignments = plan?.assignments ?? []
      const diff = proposal.preview?.diff ?? proposal.preview ?? {}
      const changes = diff.changed ?? []
      const added = diff.added ?? []
      const removed = diff.removed ?? []
      return h(Modal, {
        title: plan ? '检查计划建议' : '检查提案',
        subtitle: '这是尚未应用的候选；批准只是发起提交，收到 Core 成功回执后才算已保存。',
        onClose: closeIfIdle,
        wide: true,
        footer: h(React.Fragment, null,
          h(Button, { onClick: closeIfIdle, disabled: busy || attempt.current.pending }, blocked || grant ? '关闭并核实状态' : '放弃预览'),
          h(Button, {
            tone: 'primary',
            onClick: approveOnce,
            disabled: busy || stale || Boolean(grant) || Boolean(blocked) || attempt.current.pending,
            testid: 'plan-apply',
          }, stale ? '计划依据已变化' : blocked === 'regenerate' ? '需要重新生成预览' : blocked || grant ? '先核实提交状态' : COPY.approve),
        ),
      },
      h('p', { className: 'zw-fineprint', 'data-testid': 'plan-intent' }, proposal.intent),
      grant && !attempt.current.pending && !blocked ? h('p', { className: 'zw-alert zw-alert-warn', role: 'alert' }, '已有批准记录，但不能仅凭旧批准判断提交结果。请关闭并刷新提案与日程核实，不直接重试。') : null,
      (error || localError) ? h('div', { className: 'zw-alert zw-alert-error' }, localError || error) : null,
      stale ? h('div', { className: 'zw-alert zw-alert-warn' },
        '计划依据已变化，请刷新后重新检查。旧批准不能用于新内容。') : null,
      h('div', { className: 'zw-plan-summary' },
        h('div', null, h('strong', null, plan ? `${assignments.length} 个时间段` : `${added.length} 项新增`), h('span', null, plan ? '候选安排（未保存）' : '提案影响')),
        h('div', null, h('strong', null, plan ? `${assignments.reduce((sum, row) => sum + (row.minutes || 0), 0)} 分钟` : `${changes.length} 项修改`), h('span', null, plan ? '分配' : '变更')),
        h('div', null, h('strong', null, plan ? `${unplaced.reduce((sum, row) => sum + (row.minutes || 0), 0)} 分钟` : `${removed.length} 项移除`), h('span', null, plan ? '未排入' : '移除')),
        h('div', null, h('strong', null, `r${proposal.base_revision}`), h('span', null, '依据版本')),
      ),
      h(Section, { title: '确认前摘要' },
        h('div', { 'data-testid': 'plan-change-summary' },
          h('strong', null, '将改变什么'),
          h('p', null, `本候选拟新增 ${added.length} 项、修改 ${changes.length} 项、移除 ${removed.length} 项；逐项内容见下方差异。移除是提案的一部分，不会在预览时执行。`),
        ),
        h('div', { 'data-testid': 'plan-fixed-summary' },
          h('strong', null, '什么保持不变'),
          h('p', null, '打开、检查或放弃此预览均不应用这些变更。差异之外的事实不由本摘要推断；是否允许提交仍由 Core 校验。'),
          plan ? h('p', { className: 'zw-fineprint' }, '排程保护规则：固定预约、锁定和已开始的分配不应被自动移动。请核对实际差异；本摘要不是额外批准。') : null,
        ),
        h('div', { 'data-testid': 'plan-gap-summary' },
          h('strong', null, '哪些尚未安排'),
          h('p', null, plan ? (unplaced.length
            ? `Core 报告 ${unplaced.length} 项未排入，共 ${unplaced.reduce((sum, row) => sum + (row.minutes || 0), 0)} 分钟；原因见下方，不能仅凭总量判断是容量不足。`
            : '本候选未报告未排入项；这不代表所有任务均已完成，也不代表外部日历完整或当前可执行。')
            : '这是一般提案，未提供排程缺口结论。'),
        ),
      ),
      assignments.length ? h('div', { className: 'zw-table' },
        h('div', { className: 'zw-table-head' }, h('span', null, '时间'), h('span', null, '任务'), h('span', null, '分钟')),
        assignments.map((row, index) => h('div', { className: 'zw-table-row', key: `${row.task_id}-${index}` },
          h('span', null, `${dateTimeFor(row.start)} – ${timeFor(row.end)}`),
          h('span', null, taskTitle(stateSnapshotRef.current, row.task_id)),
          h('span', null, row.minutes),
        )),
      ) : plan ? h(Empty, null, '本候选没有新增任务分配；请结合未排入原因、诊断和差异检查，不能据此断定没有容量。') : null,
      unplaced.length ? h(Section, { title: '未排入与原因' },
        unplaced.map((row, index) => h('div', { className: 'zw-list-row', key: index },
          h('span', null, taskTitle(stateSnapshotRef.current, row.task_id)),
          h(Badge, { tone: 'warn' }, `${row.minutes} 分钟 · ${row.reason}`),
        )),
      ) : null,
      (plan?.diagnostics ?? []).length ? h(Section, { title: '诊断' },
        plan.diagnostics.map((item, index) => h('div', { className: 'zw-diagnostic', key: index },
          h(Badge, { tone: item.severity === 'hard' ? 'danger' : 'warn' }, item.code),
          h('span', null, item.message),
        )),
      ) : null,
      h(Section, { title: '差异' },
        (added.length + changes.length + removed.length) === 0 ? h(Empty, null, '只有诊断，没有正式变更。') :
          h('div', { className: 'zw-list' },
            added.map((row, index) => h('div', { className: 'zw-list-row', key: `a-${index}` },
              h(Badge, { tone: 'accent' }, '新增'), h('span', null, `${row.kind} · ${row.summary || row.id}`))),
            changes.map((row, index) => h('div', { className: 'zw-list-row', key: `c-${index}` },
              h(Badge, { tone: 'warn' }, '修改'), h('span', null, `${row.kind} · ${row.summary || row.id}`))),
            removed.map((row, index) => h('div', { className: 'zw-list-row', key: `r-${index}` },
              h(Badge, { tone: 'danger' }, '移除'), h('span', null, `${row.kind} · ${row.summary || row.id}`))),
          ),
      ),
      h('p', { className: 'zw-fineprint' }, '批准绑定当前提案内容、数据版本和策略版本；过期或内容变化后必须重新检查。'),
      )
    }

    function entityFactRows(snapshot, ref) {
      if (ref?.kind === 'goal') {
        const goal = (snapshot?.goals ?? []).find(row => row.id === ref.id)
        if (!goal) return { kindLabel: '目标', title: '（当前快照中找不到该对象）', dataClass: '未知', fields: [] }
        return {
          kindLabel: '目标',
          title: goal.title,
          dataClass: '工作空间事实（Core 未对目标单独分级；运行按 workspace 敏感度路由）',
          fields: [['title', goal.title], ['success_criteria', goal.success_criteria], ['target_due', goal.target_due]],
        }
      }
      if (ref?.kind === 'project') {
        const project = (snapshot?.projects ?? []).find(row => row.id === ref.id)
        if (!project) return { kindLabel: '项目', title: '（当前快照中找不到该对象）', dataClass: '未知', fields: [] }
        return {
          kindLabel: '项目',
          title: project.title,
          dataClass: '工作空间事实（Core 未对项目单独分级）',
          fields: [['title', project.title], ['scope', project.scope], ['goal_id', project.goal_id]],
        }
      }
      if (ref?.kind === 'task') {
        const task = (snapshot?.tasks ?? []).find(row => row.id === ref.id)
        if (!task) return { kindLabel: '任务', title: '（当前快照中找不到该对象）', dataClass: '未知', fields: [] }
        return {
          kindLabel: '任务',
          title: task.title,
          dataClass: '工作空间事实（Core 未对任务单独分级）',
          fields: [['title', task.title], ['description', task.description], ['target_due', task.target_due], ['hard_deadline', task.hard_deadline]],
        }
      }
      if (ref?.kind === 'note') {
        const note = (snapshot?.notes ?? []).find(row => row.id === ref.id)
        if (!note) return { kindLabel: '笔记', title: '（当前快照中找不到该对象）', dataClass: '未知', fields: [] }
        return {
          kindLabel: '笔记',
          title: note.title,
          dataClass: DATA_CLASS_LABELS[note.data_class] ?? note.data_class ?? '未知',
          fields: [['title', note.title], ['body', note.body], ['linked_tasks', note.linked_tasks]],
        }
      }
      return { kindLabel: ref?.kind ?? '未知类型', title: ref?.id ?? '', dataClass: '未知（该类型没有分级信息）', fields: [] }
    }

    function proposalCandidateRows(proposal) {
      const rows = []
      for (const operation of proposal?.operations ?? []) {
        const payload = operation?.payload ?? {}
        const name = operation?.operation ?? 'unknown'
        if (payload.id != null) rows.push({ operation: name, field: 'id', value: payload.id })
        if (payload.expected_version != null) rows.push({ operation: name, field: 'expected_version', value: payload.expected_version })
        const nested = payload.task ?? payload.goal ?? payload.project ?? payload.note ?? payload.saved_query ?? payload.automation ?? null
        if (nested && typeof nested === 'object') {
          for (const [field, value] of Object.entries(nested)) rows.push({ operation: name, field, value })
        } else if (Array.isArray(payload.assignments)) {
          rows.push({ operation: name, field: 'assignments', value: `${payload.assignments.length} 个分配` })
          rows.push({ operation: name, field: 'replace_task_ids', value: (payload.replace_task_ids ?? []).join(', ') || '（无）' })
        } else {
          rows.push({ operation: name, field: 'payload', value: payload })
        }
      }
      return rows
    }

    function runInputRows(run) {
      const input = run?.input
      if (!input || typeof input !== 'object') return []
      return Object.entries(input).filter(([, value]) => value != null).map(([field, value]) => ({ field, value }))
    }

    function runCheckpointRows(run) {
      const checkpoint = run?.checkpoint
      if (!checkpoint || typeof checkpoint !== 'object') return []
      const rows = []
      for (const field of ['model', 'provider', 'route', 'elapsed_ms']) {
        if (checkpoint[field] != null) rows.push({ field, value: checkpoint[field] })
      }
      if (Array.isArray(checkpoint.tasks)) rows.push({ field: 'tasks', value: `${checkpoint.tasks.length} 个候选任务标题（未提交）` })
      if (Array.isArray(checkpoint.assumptions)) rows.push({ field: 'assumptions', value: `${checkpoint.assumptions.length} 条模型假设` })
      if (Array.isArray(checkpoint.questions)) rows.push({ field: 'questions', value: `${checkpoint.questions.length} 个模型问题` })
      return rows
    }

    // L11: 资料引用与外发预览。Read-only projection of facts Core already exposes on the proposal
    // and the durable run. It never calls a new endpoint, never renders credentials and never
    // claims that a summary is redaction.
    function DataReferenceView({ proposal, run, snapshot, onClose }) {
      const policy = snapshot?.workspace?.policy ?? {}
      const reads = proposal?.reads ?? []
      const readFacts = reads.map(ref => ({ ref, ...entityFactRows(snapshot, ref) }))
      const goal = run?.input?.goal_id ? (snapshot?.goals ?? []).find(row => row.id === run.input.goal_id) : null
      const project = run?.input?.project_id ? (snapshot?.projects ?? []).find(row => row.id === run.input.project_id) : null
      const provider = run?.output?.provider ?? run?.checkpoint?.provider ?? null
      const model = run?.output?.model ?? run?.checkpoint?.model ?? null
      const route = run?.output?.route ?? run?.checkpoint?.route ?? null
      const isClarify = run?.intent === 'capture.clarify'
      const clarifyQuestions = isClarify ? (run?.output?.questions ?? run?.checkpoint?.questions ?? []) : []
      const allowedContext = [['goals', '目标内容'], ['projects', '项目背景'], ['captures', '收集原文']]
        .filter(([key]) => policy.model_access?.[key]).map(([, label]) => label).join('、') || '尚未授权资料'
      const destination = provider || model || route
        ? [provider, model, route ? `路由 ${route}` : null].filter(Boolean).join(' · ')
        : '尚无已记录的调用目的地，不能据此断言没有外发'
      const valueCell = (value, limit = 80) => {
        const cell = truncateText(value, limit)
        return h(React.Fragment, null,
          h('span', { className: 'zw-ref-value' }, cell.text || '（空）'),
          cell.truncated ? h('em', { className: 'zw-muted' }, '（已截断）') : null)
      }
      const fieldRows = (rows, dataClass) => h('div', { className: 'zw-ref-table' },
        h('div', { className: 'zw-ref-row zw-ref-row-head' },
          h('span', null, '字段'), h('span', null, '数据类别'), h('span', null, '截断值')),
        rows.map((row, index) => h('div', { className: 'zw-ref-row', key: `${row.field}-${index}` },
          h('span', { className: 'zw-mono' }, row.field),
          h('span', null, dataClass),
          valueCell(row.value),
        )),
      )
      const candidateRows = proposalCandidateRows(proposal)
      const inputRows = runInputRows(run)
      const checkpointRows = runCheckpointRows(run)
      return h(Modal, {
        title: '资料引用与外发预览',
        subtitle: proposal ? proposal.intent : (run ? `AI 运行 · ${run.intent}` : '只读预览'),
        onClose,
        wide: true,
        footer: h(Button, { onClick: onClose }, COPY.close),
      },
      h('div', { className: 'zw-alert zw-alert-warn' },
        '只读预览，不会向任何模型或宿主发送内容；也不能在这个对话框里修改引用集合。摘要和标题不是自动脱敏：标题、说明、成功标准等原文会按允许字段进入运行上下文。'),
      h(Section, { title: '来源与目的地' },
        h('dl', { className: 'zw-kv' },
          h('dt', null, '来源通道'),
          h('dd', null, proposal
            ? (SOURCE_KIND_LABELS[proposal.source?.kind] ?? proposal.source?.kind ?? '未知')
            : `Core AI 运行（${run?.intent ?? 'unknown'}）`),
          h('dt', null, '提交主体'),
          h('dd', { className: 'zw-mono' }, proposal ? `${proposal.actor_id} · ${proposal.actor_kind}` : 'ai_planner · delegated_agent'),
          proposal ? h('dt', null, '提案状态') : null,
          proposal ? h('dd', null, `${proposal.status} · 依据 r${proposal.base_revision} · 策略 p${proposal.policy_revision}`) : null,
          h('dt', null, '目的地 / 提供方'), h('dd', null, destination),
          h('dt', null, '当前资料授权'), h('dd', null, allowedContext),
          h('dt', null, '历史发送范围'), h('dd', null, '当前权限不等于历史发送记录；以该运行的实际派发记录为准。'),
          h('dt', null, '凭据边界'), h('dd', null, 'DSH 模型的凭据由 DSH 持有；自定义连接引用 Core 环境变量；浏览器不展示密钥。'),
        ),
      ),
      proposal && candidateRows.length ? h(Section, { title: '提案候选（尚未提交）' },
        h('p', { className: 'zw-fineprint' }, `提案 ${proposal.id} 包含 ${(proposal.operations ?? []).length} 个操作；以下字段是候选内容，只有人工批准并提交后才会写入正式事实。`),
        h('div', { className: 'zw-ref-table' },
          h('div', { className: 'zw-ref-row zw-ref-row-head' },
            h('span', null, '操作 / 字段'), h('span', null, '类别'), h('span', null, '截断值')),
          candidateRows.slice(0, 80).map((row, index) => h('div', { className: 'zw-ref-row', key: `${row.operation}-${row.field}-${index}` },
            h('span', { className: 'zw-mono' }, `${row.operation} · ${row.field}`),
            h('span', null, '候选'),
            valueCell(row.value),
          )),
        ),
      ) : null,
      run ? h(Section, { title: 'AI 运行记录' },
        h('dl', { className: 'zw-kv' },
          h('dt', null, '运行'), h('dd', { className: 'zw-mono' }, `${run.id} · ${run.state}`),
          h('dt', null, '创建'), h('dd', null, dateTimeFor(run.created_at)),
          proposal ? h('dt', null, '关联提案') : null,
          proposal ? h('dd', { className: 'zw-mono' }, proposal.id) : null,
        ),
        inputRows.length ? h('div', { style: { marginTop: '10px' } },
          h('h4', { style: { margin: '0 0 4px', fontSize: '12.5px' } }, 'run.input（运行入参）'),
          h('div', { className: 'zw-ref-table' }, inputRows.map((row, index) => h('div', { className: 'zw-ref-row zw-ref-row-head', key: `${row.field}-${index}` },
            h('span', { className: 'zw-mono' }, row.field), h('span', null, '入参'), valueCell(row.value),
          ))),
        ) : null,
        checkpointRows.length ? h('div', { style: { marginTop: '10px' } },
          h('h4', { style: { margin: '0 0 4px', fontSize: '12.5px' } }, 'run.checkpoint（模型已产生的草稿事实）'),
          h('div', { className: 'zw-ref-table' }, checkpointRows.map((row, index) => h('div', { className: 'zw-ref-row', key: `${row.field}-${index}` },
            h('span', { className: 'zw-mono' }, row.field), h('span', null, '运行记录'), valueCell(row.value),
          ))),
        ) : null,
        isClarify ? h('div', { style: { marginTop: '10px' } },
          h('h4', { style: { margin: '0 0 4px', fontSize: '12.5px' } }, 'capture.clarify 输出（只读，不写事实）'),
          h('div', { className: 'zw-ref-table' },
            h('div', { className: 'zw-ref-row zw-ref-row-head' },
              h('span', null, '字段'), h('span', null, '类别'), h('span', null, '截断值')),
            h('div', { className: 'zw-ref-row' },
              h('span', { className: 'zw-mono' }, 'suggested_kind'),
              h('span', null, '建议'),
              valueCell({ task: '任务', note: '笔记', goal: '目标', keep: '保持原样' }[run.output?.suggested_kind] ?? run.output?.suggested_kind ?? '未返回'),
            ),
            h('div', { className: 'zw-ref-row' },
              h('span', { className: 'zw-mono' }, 'facts_written'),
              h('span', null, '事实写入'),
              valueCell(run.output?.facts_written === false ? 'false（没有写入任何事实）' : String(run.output?.facts_written ?? '未知')),
            ),
          ),
          clarifyQuestions.length ? h('div', { style: { marginTop: '8px' } },
            clarifyQuestions.map((text, index) => h('div', { className: 'zw-list-row', key: index },
              h('span', { className: 'zw-mono' }, `questions[${index}]`),
              h('span', { className: 'zw-ref-value' }, valueCell(text, 120)),
            )),
          ) : h('p', { className: 'zw-fineprint' }, '模型没有返回澄清问题；原文可以继续手工处理。'),
        ) : null,
        run.error ? h('div', { className: 'zw-alert zw-alert-error' }, truncateText(run.error.message ?? run.error, 240).text) : null,
        (run.checkpoint?.tasks ?? []).length ? h('div', { style: { marginTop: '10px' } },
          h('h4', { style: { margin: '0 0 4px', fontSize: '12.5px' } }, '模型返回的候选任务标题（已截断）'),
          (run.checkpoint.tasks ?? []).slice(0, 20).map((task, index) => h('div', { className: 'zw-list-row', key: index },
            h('span', { className: 'zw-mono' }, `tasks[${index}].title`),
            h('span', { className: 'zw-ref-value' }, valueCell(task?.title)),
          )),
        ) : null,
      ) : null,
      h(Section, { title: '引用事实（proposal.reads）' },
        readFacts.length ? readFacts.map((fact, index) => h('div', { key: `${fact.ref.kind}-${fact.ref.id}-${index}`, className: 'zw-ref-entity' },
          h('div', { className: 'zw-card-meta' },
            h(Badge, { tone: 'neutral' }, `${fact.kindLabel} · ${fact.ref.kind}`),
            h('span', { className: 'zw-mono' }, fact.ref.id),
            h(Badge, { tone: 'warn' }, `数据类别：${fact.dataClass}`),
          ),
          h('strong', null, fact.title),
          fact.fields.length ? fieldRows(fact.fields, fact.dataClass) : h('p', { className: 'zw-fineprint' }, '该对象没有可展示的允许字段。'),
        )) : h(Empty, null, proposal
          ? '该提案没有声明 reads；人工/Agent 提交的候选只经 Core 校验，没有模型外发引用。'
          : '该运行没有声明读取对象。'),
      ),
      run && isClarify ? h(Section, { title: '当前授权下的上下文预览' },
        h('p', { className: 'zw-fineprint' }, '按当前快照与当前权限预览，不是历史请求还原。收集澄清只使用获准原文，不读取笔记、附件、任务或目标。'),
        policy.model_access?.captures ? h('p', null, valueCell(run.input?.text, 160))
          : h('p', { className: 'zw-muted' }, '收集原文当前未授权，不会为新的收集澄清请求派发模型。'),
        h('p', { className: 'zw-fineprint' }, '模型只返回澄清问题与建议类型，不自动转换、批准或提交。'),
      ) : run && goal ? h(Section, { title: '当前授权下的上下文预览' },
        h('p', { className: 'zw-fineprint' }, '按当前目标和权限显示可用于新请求的字段；不代表该历史运行实际发送了这些内容。'),
        policy.model_access?.goals ? h('dl', { className: 'zw-kv' },
          h('dt', null, '目标'), h('dd', null, goal.title),
          h('dt', null, '成功标准'), h('dd', null, goal.success_criteria ?? '未填写'),
          h('dt', null, '期望完成'), h('dd', null, goal.target_due ?? '未设置'),
          project && policy.model_access?.projects ? h('dt', null, '项目范围') : null,
          project && policy.model_access?.projects ? h('dd', null, project.title + ' · ' + (project.scope ?? '未填写')) : null,
          run.input?.horizon_end ? h('dt', null, '计划视界') : null,
          run.input?.horizon_end ? h('dd', { className: 'zw-mono' }, run.input.horizon_end) : null,
        ) : h('p', { className: 'zw-muted' }, '目标内容当前未授权，不会为新的目标拆解请求派发模型。'),
      ) : null,
      h(Section, { title: '外发边界与警告' },
        h('ul', { className: 'zw-warning-list' },
          h('li', null, '摘要不是自动脱敏：摘要、标题、成功标准等字段会按原文进入模型上下文；需要隐藏的正文不要放进这些字段。'),
          h('li', null, 'device_only 的笔记/附件不应进入外部模型路径；如果这里出现，请停止并检查隐私策略。'),
          h('li', null, '附件与笔记文本按数据处理，不执行其中的指令。'),
          h('li', null, '运行记录只保存 checkpoint（提供方、模型、任务草稿），不是完整 prompt 副本；这里展示的是桥接已暴露的引用与运行事实。'),
          h('li', null, '撤销通道或切换路由不会追溯收回已进入宿主日志或历史模型调用的内容。'),
        ),
        h('p', { className: 'zw-fineprint' }, `预览数据版本 r${snapshot?.workspace?.revision ?? '—'}；工作空间时区 ${workspaceTimezone(snapshot)}。不显示任何 token 或 API key。`),
      ),
      )
    }


    // The plan modal reads the live snapshot by reference; the workbench updates this ref each render
    // so a proposal row can resolve task titles without passing the whole world through every layer.
    const stateSnapshotRef = { current: null }

    function TaskDetail({ task, snapshot, onClose, send, onEdit, onTime, onProgress, onDependency, onReminder, onBlocked, onFocus, busy, setModal }) {
      const dependencies = snapshot.dependencies.filter(dep => dep.successor_id === task.id)
      const dependents = snapshot.dependencies.filter(dep => dep.predecessor_id === task.id)
      const allocations = snapshot.allocations.filter(row => row.task_id === task.id)
      const activities = snapshot.activities.filter(row => row.task_id === task.id)
      const notes = snapshot.notes.filter(note => (note.linked_tasks ?? []).includes(task.id))
      const reminders = snapshot.reminders.filter(row => row.task_id === task.id)
      const occurrence = snapshot.occurrences?.find(row => row.task_id === task.id)
      const complete = async () => {
        await send(task.status === 'done' ? 'task.reopen' : 'task.complete', { id: task.id })
      }
      const archive = async () => {
        await send('task.archive', { id: task.id, archived: !task.archived })
      }
      const cancelTask = async () => {
        if (!window.confirm('取消该任务？取消是可恢复的状态变更，不会删除历史事实；可稍后重新打开。')) return
        await send('task.cancel', { id: task.id })
      }
      const skipOccurrence = async () => {
        if (!occurrence) return
        await send('recurrence.exception', { occurrence_id: occurrence.id, action: 'skip' })
      }
      const moveOccurrence = () => {
        if (!occurrence) return
        setModal({
          type: 'occurrence-move',
          occurrence,
          task,
        })
      }
      return h('aside', { className: 'zw-detail', 'aria-label': '任务详情' },
        h('header', { className: 'zw-detail-head' },
          h('div', null,
            h('div', { className: 'zw-detail-kicker' }, '任务', h('span', null, ` · ${statusLabel(task.status)}`)),
            h('h2', null, task.title),
          ),
          h('button', { className: 'zw-icon-btn', onClick: onClose, 'aria-label': COPY.close }, '×'),
        ),
        task.archived ? h(Badge, { tone: 'warn' }, '已归档') : null,
        h('p', { className: 'zw-detail-meta' },
          task.target_due ? `期望完成 ${task.target_due}` : '无期望完成',
          task.hard_deadline ? ` · 必须 ${dateTimeFor(task.hard_deadline)}` : '',
        ),
        h('div', { className: 'zw-stat-strip' },
          h('div', null, h('span', null, '预计'), h('strong', null, `${task.estimate_minutes ?? '—'} 分`)),
          h('div', null, h('span', null, '剩余'), h('strong', null, `${task.remaining_minutes ?? '—'} 分`)),
          h('div', null, h('span', null, '实际'), h('strong', null, `${task.actual_minutes ?? 0} 分`)),
          h('div', null, h('span', null, '优先级'), h('strong', null, task.priority)),
        ),
        h('div', { className: 'zw-action-row' },
          h(Button, { tone: 'primary', onClick: complete, disabled: busy }, task.status === 'done' ? '重新打开' : '标记完成'),
          h(Button, { onClick: onProgress, disabled: busy || task.status === 'done' }, '记录进度'),
          h(Button, { onClick: onFocus, disabled: busy || task.status === 'done' }, '开始专注'),
          h(Button, { onClick: onEdit, disabled: busy }, '编辑'),
          h(Button, { onClick: onTime, disabled: busy, testid: 'task-time' }, '时间'),
          h(Button, { onClick: archive, disabled: busy }, task.archived ? '恢复' : '归档'),
          h(Button, { onClick: cancelTask, disabled: busy || task.status === 'cancelled' }, '取消任务'),
        ),
        h(Button, { onClick: onDependency, disabled: busy }, '添加依赖'),
        h(Button, { onClick: onReminder, disabled: busy }, '设置提醒'),
        h(Button, { onClick: onBlocked, disabled: busy }, task.blocked_manual ? '解除阻塞' : '标记阻塞'),
        task.series_id ? h(Section, { title: '循环实例' },
          h('p', { className: 'zw-fineprint' }, `系列 ${task.series_id} · 发生键 ${task.occurrence_key ?? '—'}`),
          h('div', { className: 'zw-action-row' },
            h(Button, { onClick: skipOccurrence, disabled: busy || !occurrence }, '本次跳过'),
            h(Button, { onClick: moveOccurrence, disabled: busy || !occurrence }, '本次改期'),
            h(Button, { onClick: () => setModal({ type: 'recurrence-scope', task }), disabled: busy, testid: 'recurrence-scope' }, '循环作用范围'),
          ),
        ) : null,
        h(Section, { title: '依赖关系' },
          dependencies.length ? dependencies.map(dep => h('div', { className: 'zw-list-row', key: dep.id },
            h('span', null, `前置 · ${taskTitle(snapshot, dep.predecessor_id)}`),
            h('span', { className: 'zw-muted' }, '完成前置后才能安排'),
          )) : h(Empty, null, '没有前置依赖。'),
          dependents.length ? h('div', { className: 'zw-fineprint' }, `影响 ${dependents.length} 个后续任务`) : null,
        ),
        h(Section, { title: '已安排时间' },
          allocations.length ? allocations.map(allocation => h('div', { className: 'zw-list-row', key: allocation.id },
            h('span', null, `${dateTimeFor(allocation.start)} – ${timeFor(allocation.end)}`),
            h(Badge, { tone: allocation.locked ? 'warn' : 'accent' }, allocation.locked ? '锁定' : '计划'),
          )) : h(Empty, null, '尚未分配时间。'),
        ),
        h(Section, { title: '执行记录' },
          activities.length ? activities.map(activity => h('div', { className: 'zw-list-row', key: activity.id },
            h('span', null, `${dateTimeFor(activity.occurred_at)} · ${activity.minutes} 分钟`),
            h('span', { className: 'zw-muted' }, activity.kind),
          )) : h(Empty, null, '还没有执行事实。'),
        ),
        reminders.length ? h(Section, { title: '提醒意图' },
          reminders.map(reminder => h('div', { className: 'zw-list-row', key: reminder.id },
            h('span', null, `${dateTimeFor(reminder.fire_at)} · ${reminder.message}`),
            h('span', { className: 'zw-muted' }, reminder.status === 'delivered' && reminder.delivered_at
              ? `送达 ${dateTimeFor(reminder.delivered_at)}`
              : reminder.status === 'failed'
                ? `失败：${reminder.failure ?? '未知原因'}`
                : reminder.status === 'cancelled' ? '已取消' : '等待送达'),
            h(Badge, { tone: reminder.status === 'pending' ? 'accent' : 'neutral' }, reminder.status),
          )),
        ) : null,
        notes.length ? h(Section, { title: '关联资料' },
          notes.map(note => h('div', { className: 'zw-list-row', key: note.id }, h('span', null, note.title))),
        ) : null,
        h('p', { className: 'zw-fineprint' }, '完成、专注结束和目标达成分别记录；专注结束不会自动完成任务。'),
      )
    }

    function OccurrenceMove({ occurrence, task, onClose, send, busy }) {
      const [date, setDate] = useState(localDateInput(occurrence.current_date))
      const [error, setError] = useState('')
      const submit = async () => {
        try {
          await send('recurrence.exception', { occurrence_id: occurrence.id, action: 'move', new_date: date })
          onClose()
        } catch (failure) {
          setError(failure.message)
        }
      }
      return h(Modal, { title: '本次改期', subtitle: '只改变这一次的发生，不改变系列发生键。', onClose,
        footer: h(React.Fragment, null,
          h(Button, { onClick: onClose }, COPY.cancel),
          h(Button, { tone: 'primary', onClick: submit, disabled: busy }, COPY.save)) },
        error ? h('div', { className: 'zw-alert zw-alert-error' }, error) : null,
        h(Field, { label: '新的日期', wide: true }, h('input', { type: 'date', value: date,
          onChange: event => setDate(event.target.value) })),
        h('p', { className: 'zw-fineprint' }, task.title),
      )
    }

    // L06: a recurring task can change only this occurrence, or every generated instance from today
    // onward. The dialog reports the exact affected range before writing; the "this occurrence"
    // scope reuses the existing exception actions instead of pretending to edit the series.
    function RecurrenceScopeDialog({ task, snapshot, onClose, send, busy, setModal }) {
      const rule = (snapshot.recurrence_rules ?? []).find(item => item.id === task.series_id) ?? null
      const occurrence = (snapshot.occurrences ?? []).find(item => item.task_id === task.id) ?? null
      const [scope, setScope] = useState('single')
      const [form, setForm] = useState({
        title: rule?.title ?? task.title,
        estimate: rule?.estimate_minutes ?? task.estimate_minutes ?? '',
        priority: rule?.priority ?? task.priority ?? '',
      })
      const [error, setError] = useState('')
      const timeZone = workspaceTimezone(snapshot)
      const instant = snapshot?.now ? new Date(snapshot.now).getTime() : Date.now()
      const parts = zonePartsAt(Number.isNaN(instant) ? Date.now() : instant, timeZone)
      const today = `${parts.year}-${String(parts.month).padStart(2, '0')}-${String(parts.day).padStart(2, '0')}`
      const lockedTaskIds = new Set((snapshot.allocations ?? [])
        .filter(row => row.locked || row.state === 'started')
        .map(row => row.task_id))
      const closedTaskIds = new Set((snapshot.tasks ?? [])
        .filter(row => row.status === 'done' || row.status === 'cancelled')
        .map(row => row.id))
      const futureOccurrences = (snapshot.occurrences ?? []).filter(item => rule
        && item.rule_id === rule.id
        && item.status === 'generated'
        && item.current_date >= today
        && item.task_id)
      const affected = futureOccurrences.filter(item => !lockedTaskIds.has(item.task_id) && !closedTaskIds.has(item.task_id))
      const kept = futureOccurrences.length - affected.length
      const update = patch => setForm({ ...form, ...patch })
      const chooseScope = next => {
        setScope(next)
        setError('')
      }
      const submitWithScope = async scopeValue => {
        if (!rule) return setError('找不到这个系列（可能已被删除），只能处理本次实例。')
        const title = form.title.trim()
        if (!title) return setError('请填写系列标题')
        if (title.length > 500) return setError('系列标题不能超过 500 个字符')
        const estimate = form.estimate === '' || form.estimate === null ? null : Number(form.estimate)
        if (estimate !== null && (!Number.isInteger(estimate) || estimate < 15 || estimate > 1440)) {
          return setError('每次预计分钟必须是 15–1440 的整数')
        }
        const priority = form.priority === '' || form.priority === null ? null : Number(form.priority)
        if (priority !== null && (!Number.isInteger(priority) || priority < 0 || priority > 4)) {
          return setError('优先级必须是 0–4 的整数')
        }
        try {
          await send('recurrence.update', {
            rule_id: rule.id,
            scope: scopeValue,
            title,
            estimate_minutes: estimate,
            priority,
          })
          onClose()
        } catch (failure) {
          setError(failure.message)
        }
      }
      const skipThis = async () => {
        if (!occurrence) return setError('这个任务没有绑定的循环实例，无法只跳过本次。')
        try {
          await send('recurrence.exception', { occurrence_id: occurrence.id, action: 'skip' })
          onClose()
        } catch (failure) {
          setError(failure.message)
        }
      }
      const moveThis = () => {
        if (!occurrence) return setError('这个任务没有绑定的循环实例，无法只改本次。')
        setModal({ type: 'occurrence-move', occurrence, task })
      }
      const footer = h(React.Fragment, null,
        h(Button, { onClick: onClose }, scope === 'single' ? COPY.close : COPY.cancel),
        scope === 'future' ? h(Button, { tone: 'primary', onClick: () => submitWithScope('future'), disabled: busy, testid: 'recurrence-scope-submit' }, '\u5e94\u7528\u5230\u672a\u6765\u7cfb\u5217') : null,
        scope === 'series' ? h(Button, { tone: 'primary', onClick: () => submitWithScope('series'), disabled: busy, testid: 'recurrence-scope-series-submit' }, '\u5e94\u7528\u5230\u6574\u4e2a\u7cfb\u5217') : null,
      )
      return h(Modal, {
        title: '循环作用范围',
        subtitle: '先选择改动范围；历史、已发生和已锁定的实例不会被改写。',
        onClose,
        footer,
        wide: true,
      },
      error ? h('div', { className: 'zw-alert zw-alert-error', role: 'alert' }, error) : null,
      h('div', { className: 'zw-action-row' },
        h(Pill, { active: scope === 'single', onClick: () => chooseScope('single'), testid: 'recurrence-scope-single' }, '仅本次'),
        rule ? h(Pill, { active: scope === 'future', onClick: () => chooseScope('future'), testid: 'recurrence-scope-future' }, '未来系列') : null,
        rule ? h(Pill, { active: scope === 'series', onClick: () => chooseScope('series'), testid: 'recurrence-scope-series' }, '\u6574\u4e2a\u7cfb\u5217') : null,
      ),
      occurrence
        ? h('p', { className: 'zw-fineprint' }, `本次实例 ${occurrence.current_date} · 发生键 ${occurrence.occurrence_key}${occurrence.status === 'moved' ? ' · 已改期' : ''}`)
        : h('p', { className: 'zw-fineprint' }, '这个任务没有绑定的循环实例；仅本次的改期/跳过不可用。'),
      scope === 'single' ? h(Section, { title: '仅本次' },
        h('p', { className: 'zw-fineprint' }, '只改变这一次的发生；系列身份、发生键和其他实例保持不变。'),
        h('div', { className: 'zw-action-row' },
          h(Button, { onClick: moveThis, disabled: busy || !occurrence, testid: 'recurrence-scope-move' }, '本次改期'),
          h(Button, { onClick: skipThis, disabled: busy || !occurrence, testid: 'recurrence-scope-skip' }, '本次跳过'),
        ),
      ) : h(Section, { title: '未来系列' },
        h('p', { className: 'zw-fineprint' }, `将更新 ${affected.length} 个生成于今天（${today}，${timeZone}）及以后的实例${kept ? `；另有 ${kept} 个已开始/锁定/已完成的实例保持不变` : ''}。`),
        h(Field, { label: '系列标题', wide: true, hint: '会写入规则和每个被更新的未来实例。' },
          h('input', { value: form.title, 'data-testid': 'recurrence-scope-title',
            onChange: event => update({ title: event.target.value }) })),
        h(Field, { label: '每次预计分钟', hint: '15–1440；留空表示保留规则当前值。' },
          h('input', { type: 'number', min: 15, max: 1440, value: form.estimate, 'data-testid': 'recurrence-scope-estimate',
            onChange: event => update({ estimate: event.target.value }) })),
        h(Field, { label: '优先级', hint: '0–4；留空表示保留规则当前值。' },
          h('input', { type: 'number', min: 0, max: 4, value: form.priority, 'data-testid': 'recurrence-scope-priority',
            onChange: event => update({ priority: event.target.value }) })),
        h('p', { className: 'zw-fineprint' }, rule?.time_of_day
          ? `系列时间点保持为 ${rule.time_of_day}；这里不修改系列开始日期、频率、间隔和发生率。全系列编辑尚未实现。`
          : '这里不修改系列开始日期、频率、间隔和发生率。全系列编辑尚未实现。'),
      ),
      )
    }

    function BlockedEditor({ task, onClose, send, busy }) {
      const [blocked, setBlocked] = useState(!task.blocked_manual)
      const [reason, setReason] = useState(task.blocked_reason ?? '')
      const submit = async () => {
        await send('task.set_blocked', { id: task.id, blocked, reason: blocked ? (reason.trim() || null) : null })
        onClose()
      }
      return h(Modal, { title: blocked ? '标记阻塞' : '解除阻塞', onClose,
        footer: h(React.Fragment, null,
          h(Button, { onClick: onClose }, COPY.cancel),
          h(Button, { tone: 'primary', onClick: submit, disabled: busy }, COPY.save)) },
        blocked ? h(Field, { label: '原因（可选）', wide: true }, h('textarea', { rows: 3, value: reason,
          onChange: event => setReason(event.target.value) })) : null,
      )
    }

    function RunsView({ runs, loading, busy, refresh, cancel, onProposal, snapshot, setModal }) {
      const stateTone = state => state === 'succeeded' ? 'accent'
        : state === 'failed' ? 'danger'
          : state === 'cancelled' ? 'warn'
            : 'neutral'
      const proposalStatus = id => (snapshot?.proposals ?? []).find(proposal => proposal.id === id)?.status
      return h('div', { className: 'zw-page' },
        h('header', { className: 'zw-page-head' },
          h('div', null, h('div', { className: 'zw-kicker' }, '持久化 AI 工作流'), h('h1', null, 'AI 运行')),
          h('div', { className: 'zw-head-actions' },
            h(Button, { onClick: refresh, disabled: busy || loading }, COPY.refresh),
          ),
        ),
        h('p', { className: 'zw-fineprint' }, '运行在工作线程中执行，模型调用不占用事实事务；成功只产出提案，不会自动批准或提交。'),
        runs.length ? h('div', { className: 'zw-run-list' },
          runs.map(run => h('article', { className: 'zw-card', key: run.id },
            h('header', { className: 'zw-card-head' },
              h('div', null,
                h(Badge, { tone: stateTone(run.state) }, run.state),
                h('h3', null, run.intent),
              ),
              h('span', { className: 'zw-mono zw-muted' }, run.id.slice(0, 16)),
            ),
            h('div', { className: 'zw-card-meta' },
              h('span', null, dateTimeFor(run.created_at)),
              run.output?.provider ? h('span', null, `${run.output.provider} · ${run.output.route}`) : null,
              run.output?.task_count ? h('span', null, `${run.output.task_count} 个任务`) : null,
              run.checkpoint?.elapsed_ms ? h('span', null, `${run.checkpoint.elapsed_ms}ms`) : null,
            ),
            run.error ? h('div', { className: 'zw-alert zw-alert-error' }, run.error.message ?? JSON.stringify(run.error)) : null,
            run.state === 'queued' || run.state === 'running' ? h('div', { className: 'zw-ai-progress' },
              h('span', { className: 'zw-spinner' }),
              h('span', null, run.state === 'queued' ? '等待工作线程…' : '模型/校验进行中…'),
              h(Button, { onClick: () => cancel(run.id), disabled: busy }, '取消'),
            ) : null,
            run.output?.proposal_id ? h('div', { className: 'zw-action-row' },
              ['applied', 'rejected', 'expired'].includes(proposalStatus(run.output.proposal_id))
                ? h(Badge, { tone: proposalStatus(run.output.proposal_id) === 'applied' ? 'accent' : 'neutral' },
                  proposalStatus(run.output.proposal_id) === 'applied' ? '提案已应用' : `提案 ${proposalStatus(run.output.proposal_id)}`)
                : h(Button, { tone: 'primary', onClick: () => onProposal(run.output.proposal_id) }, '检查提案'),
            ) : null,
            h('div', { className: 'zw-action-row' },
              h(Button, {
                onClick: () => setModal({
                  type: 'data-reference',
                  run,
                  proposal: (snapshot?.proposals ?? []).find(proposal => proposal.id === run.output?.proposal_id) ?? null,
                }),
                testid: `run-data-reference-${run.id}`,
              }, '资料与外发'),
            ),
            run.checkpoint?.questions?.length ? h('p', { className: 'zw-fineprint' }, `模型问题 ${run.checkpoint.questions.length} 个；不会自动执行。`) : null,
          )),
        ) : h(Empty, null, loading ? '正在读取运行…' : '还没有 AI 运行。可在目标卡片上启动拆解。'),
      )
    }

    const MODEL_REASONS = {
      model_not_selected: '尚未选择模型',
      host_unavailable: 'DSH 宿主调用通道尚未就绪',
      host_model_unavailable: '选定的 DSH 模型当前不可用，请刷新后重新选择',
      custom_credentials_missing: 'Core 未检测到指定的密钥环境变量',
      configured: '配置检查通过；尚未验证实际推理',
      goals_not_allowed: '请在隐私设置中允许目标内容',
      captures_not_allowed: '请在隐私设置中允许收集原文',
      device_only_requires_custom_loopback: '禁止外发的内容只能交给已验证的本机端点',
    }
    const modelReason = state => MODEL_REASONS[state?.reason] ?? state?.reason ?? '等待读取状态'

    function ModelsView({ snapshot, busy, report, onPrivacy }) {
      const [config, setConfig] = useState(null)
      const [draft, setDraft] = useState(null)
      const [effective, setEffective] = useState(null)
      const [catalog, setCatalog] = useState(null)
      const [error, setError] = useState('')
      const [catalogError, setCatalogError] = useState('')
      const [loading, setLoading] = useState(true)
      const [catalogLoading, setCatalogLoading] = useState(false)
      const [saving, setSaving] = useState(false)
      const [dirty, setDirty] = useState(false)
      const [stale, setStale] = useState(false)
      const dirtyRef = useRef(false)
      const baseline = useRef(null)
      const alive = useRef(true)
      const sourceDrafts = useRef({ dsh: { kind: 'dsh', provider: null, model: null },
        custom: { kind: 'custom', base_url: '', model: '', api_key_env: null } })
      useEffect(() => { alive.current = true; return () => { alive.current = false } }, [])
      const apply = (result, discard = false) => {
        setConfig(result.config)
        setEffective(result.effective)
        if (discard || !dirtyRef.current) {
          setDraft(result.config)
          baseline.current = JSON.stringify(result.config)
          sourceDrafts.current[result.config.source.kind] = result.config.source
          dirtyRef.current = false; setDirty(false); setStale(false)
        } else setStale(JSON.stringify(result.config) !== baseline.current)
      }
      const loadModel = async (discard = false) => {
        setLoading(true); setError('')
        try { const result = await getModelConfig(); if (alive.current) apply(result, discard) }
        catch (failure) { if (alive.current) { setError(failure.message); setEffective(null) } }
        finally { if (alive.current) setLoading(false) }
      }
      const loadCatalog = async () => {
        setCatalogLoading(true); setCatalogError('')
        try { const result = await getHarnessModels(); if (alive.current) setCatalog(result) }
        catch (failure) { if (alive.current) setCatalogError(failure.message) }
        finally { if (alive.current) setCatalogLoading(false) }
      }
      useEffect(() => { void loadModel() }, [snapshot.workspace.policy_revision])
      useEffect(() => { void loadCatalog() }, [])
      const change = source => {
        sourceDrafts.current[source.kind] = source
        dirtyRef.current = true; setDirty(true); setDraft({ version: 2, source }); setError('')
      }
      const save = async () => {
        if (!draft || stale) return
        setSaving(true); setError('')
        try {
          const current = await getModelConfig()
          if (!alive.current) return
          if (JSON.stringify(current.config) !== baseline.current) {
            setConfig(current.config); setEffective(current.effective); setStale(true)
            setError('模型配置已在其他界面更新；草稿已保留，未覆盖服务器。请放弃草稿并重新载入。')
            return
          }
          const result = await updateModel(draft)
          if (!alive.current) return
          apply(result, true)
          report('模型已保存；隐私授权未改变。')
        } catch (failure) { if (alive.current) setError(failure.message) }
        finally { if (alive.current) setSaving(false) }
      }
      const groups = catalog?.groups ?? []
      const options = groups.flatMap(group => (group.models ?? []).map(model => ({ group, model,
        value: JSON.stringify({ provider: group.provider, model: model.id }) })))
      const source = draft?.source
      const choice = source?.kind === 'dsh' && source.provider && source.model
        ? JSON.stringify({ provider: source.provider, model: source.model }) : ''
      const matching = options.find(row => row.value === choice)
      const chosenName = state => !state?.source?.model ? '尚未选择' : state.source.kind === 'dsh'
        ? 'DSH · ' + state.source.provider + ' / ' + state.source.model : '自定义 · ' + state.source.model
      const disabled = busy || loading || saving || !effective
      const canSave = dirty && !stale && source && (source.kind === 'dsh'
        ? Boolean(matching?.group.supported && matching?.model.supported)
        : Boolean(source.base_url.trim() && source.model.trim()))
      const customField = (key, label, placeholder) => h(Field, { label }, h('input', {
        value: source[key] ?? '', placeholder, disabled, autoComplete: 'off',
        onChange: event => change({ ...source, [key]: key === 'api_key_env' ? event.target.value || null : event.target.value }),
      }))
      return h('div', { className: 'zw-page zw-ai-settings' },
        h('header', { className: 'zw-page-head' },
          h('div', null, h('div', { className: 'zw-kicker' }, '选择 Zodo 内置 AI 使用的模型'), h('h1', null, '模型')),
          h('div', { className: 'zw-head-actions' },
            dirty ? h(Button, { onClick: () => { void loadModel(true) }, disabled: loading || saving, testid: 'model-discard' }, '放弃草稿并重新载入') : null,
            h(Button, { onClick: () => { void loadModel(); void loadCatalog() }, disabled: loading || saving || catalogLoading, testid: 'model-refresh' }, '刷新'))),
        h('p', { className: 'zw-fineprint' }, '只选一个模型来源，不改变 DSH 当前聊天模型。哪些内容可交给模型，由独立的隐私设置控制。'),
        error ? h('div', { className: 'zw-alert zw-alert-error', role: 'alert' }, error) : null,
        h(Section, { title: '模型来源' },
          loading && !config ? h('p', { className: 'zw-muted', role: 'status' }, '正在读取已保存的模型配置…') : null,
          h('fieldset', { className: 'zw-source-options', disabled },
            h('legend', { className: 'zw-source-legend' }, '使用已有配置，或连接自己的模型'),
            [['dsh', '使用 DSH 中的模型', '直接复用 DSH 的模型与登录认证，不用再填写地址或密钥。'],
              ['custom', '自定义模型', '填写 OpenAI 兼容 API 地址与模型 ID。本机服务也使用这一配置。']]
              .map(([kind, title, description]) => h('label', { key: kind, className: 'zw-source-option' + (source?.kind === kind ? ' zw-source-selected' : '') },
                h('input', { type: 'radio', name: 'zodo-model-source', checked: source?.kind === kind,
                  'data-testid': 'model-source-' + kind, onChange: () => change(sourceDrafts.current[kind]) }),
                h('span', null, h('strong', null, title), h('span', null, description))))),
          source?.kind === 'dsh' ? h(React.Fragment, null,
            catalogError ? h('div', { className: 'zw-alert zw-alert-error', role: 'alert', 'data-testid': 'model-catalog-error' },
              '无法读取 DSH 模型：' + catalogError + '。可点击刷新重试；这不会影响自定义连接。') : null,
            h(Field, { label: 'DSH 模型' }, h('select', { 'aria-label': 'DSH 模型', 'data-testid': 'model-dsh-select', value: choice,
              disabled: disabled || catalogLoading || Boolean(catalogError), onChange: event => {
                const selected = event.target.value ? JSON.parse(event.target.value) : { provider: null, model: null }
                change({ kind: 'dsh', ...selected })
              } }, h('option', { value: '' }, catalogLoading ? '读取 DSH 模型…' : '请选择一个 DSH 模型'),
              choice && !matching ? h('option', { value: choice, disabled: true }, source.provider + ' / ' + source.model + '（目录中不可用）') : null,
              groups.map(group => h('optgroup', { key: group.provider, label: group.name || group.provider },
                (group.models ?? []).map(model => h('option', { key: model.id, value: JSON.stringify({ provider: group.provider, model: model.id }),
                  disabled: !group.supported || !model.supported }, model.name || model.id)))))),
            !catalogLoading && !catalogError && !options.length ? h('p', { className: 'zw-fineprint' }, 'DSH 暂无可选模型，请先在 DSH 中配置，或选择自定义模型。') : null,
            (catalog?.failures ?? []).length ? h('p', { className: 'zw-fineprint' }, '部分提供商目录读取失败，请刷新重试；未成功读取的模型不会被自动替代。') : null,
            h('p', { className: 'zw-fineprint' }, 'DSH 负责实际调用及其认证，包括由 DSH 支持的订阅登录模型；Zodo 只保存你的明确选择，不复制凭据、不附带聊天历史。')) : null,
          source?.kind === 'custom' ? h(React.Fragment, null,
            h('div', { className: 'zw-grid2' }, customField('base_url', 'API 地址', 'https://api.example.com/v1 或 http://127.0.0.1:11434/v1'),
              customField('model', '模型 ID', '填写服务支持的模型 ID')),
            h('details', { className: 'zw-ai-advanced', 'data-testid': 'model-advanced' }, h('summary', null, '认证设置（需要密钥时）'),
              customField('api_key_env', '密钥环境变量名（可选）', '例如 MY_MODEL_API_KEY'),
              h('p', { className: 'zw-fineprint' }, '这里只填变量名，不填 API Key。密钥从 Core 启动环境读取；本机无鉴权服务可留空。修改环境变量后需重启 Core。')),
            h('p', { className: 'zw-fineprint' }, '模型请求将发往这个 API 地址；本机地址也不会自动获得额外资料权限。不使用其他连接作为失败回退。')) : null,
          h('div', { className: 'zw-action-row' }, h(Button, { tone: 'primary', onClick: save, disabled: disabled || !canSave, testid: 'model-save' }, '保存模型'),
            dirty ? h('span', { className: 'zw-fineprint' }, '更改尚未保存') : null),
          stale ? h('div', { className: 'zw-alert zw-alert-warn', role: 'alert', 'data-testid': 'model-stale' }, '检测到服务器配置变化，草稿已保留；本次不发送保存请求。请放弃草稿并重新载入。') : null),
        h(Section, { title: '当前已保存', testid: 'model-effective-status' },
          h('p', { className: 'zw-model-selected-name' }, chosenName(config)),
          h('p', null, modelReason(effective?.selected)),
          h('dl', { className: 'zw-kv' }, h('dt', null, '目标拆解'), h('dd', null, modelReason(effective?.operations?.decompose)),
            h('dt', null, '收集澄清'), h('dd', null, modelReason(effective?.operations?.clarify))),
          h('p', { className: 'zw-fineprint' }, '状态来自配置和授权检查，不代表服务可达或实际推理成功。模型选择不会自动放宽隐私权限。'),
          h(Button, { onClick: onPrivacy }, '设置可发送的内容')),
      )
    }

    function HabitEditor({ habit, onClose, send, busy }) {
      const [form, setForm] = useState({
        title: habit?.title ?? '',
        start: habit?.start_date ?? todayIso(),
        end: habit?.end_date ?? '',
        target: habit?.weekly_target ?? 3,
        weekdays: habit?.scheduled_weekdays ?? [1, 3, 5],
        time: habit?.time_of_day ? String(habit.time_of_day).slice(0, 5) : '',
      })
      const [error, setError] = useState('')
      const toggle = day => setForm({
        ...form,
        weekdays: form.weekdays.includes(day) ? form.weekdays.filter(value => value !== day) : [...form.weekdays, day].sort(),
      })
      const submit = async () => {
        setError('')
        if (!form.title.trim()) return setError('请填写习惯标题')
        const spec = {
          title: form.title.trim(),
          timezone: 'Asia/Shanghai',
          start_date: form.start,
          end_date: form.end || null,
          weekly_target: Number(form.target) || 1,
          scheduled_weekdays: form.weekdays,
          time_of_day: form.time ? `${form.time}:00` : null,
        }
        try {
          if (habit) await send('habit.update', { id: habit.id, habit: spec })
          else await send('habit.create', { habit: spec })
          onClose()
        } catch (failure) {
          setError(failure.message)
        }
      }
      return h(Modal, { title: habit ? '编辑习惯' : '新建习惯', subtitle: '习惯记录是执行事实，周目标不会自动变成任务完成。', onClose,
        footer: h(React.Fragment, null,
          h(Button, { onClick: onClose }, COPY.cancel),
          h(Button, { tone: 'primary', onClick: submit, disabled: busy }, COPY.save)) },
        error ? h('div', { className: 'zw-alert zw-alert-error' }, error) : null,
        h(Field, { label: '习惯', wide: true }, h('input', { value: form.title, autoFocus: true,
          onChange: event => setForm({ ...form, title: event.target.value }) })),
        h('div', { className: 'zw-grid2' },
          h(Field, { label: '每周目标次数' }, h('input', { type: 'number', min: 1, max: 100, value: form.target,
            onChange: event => setForm({ ...form, target: event.target.value }) })),
          h(Field, { label: '提醒时间（可选）' }, h('input', { type: 'time', value: form.time,
            onChange: event => setForm({ ...form, time: event.target.value }) })),
          h(Field, { label: '开始日期' }, h('input', { type: 'date', value: form.start,
            onChange: event => setForm({ ...form, start: event.target.value }) })),
          h(Field, { label: '结束日期（可选）' }, h('input', { type: 'date', value: form.end,
            onChange: event => setForm({ ...form, end: event.target.value }) })),
        ),
        h('div', { className: 'zw-weekdays' },
          ['一', '二', '三', '四', '五', '六', '日'].map((label, index) => h(Pill, {
            key: label, active: form.weekdays.includes(index + 1), onClick: () => toggle(index + 1),
          }, label))),
        h('p', { className: 'zw-fineprint' }, '周目标是判断依据，不把每一次记录变成任务；一次例外可在日志中跳过。'),
      )
    }

    function HabitLogEditor({ habit, onClose, send, busy }) {
      const [date, setDate] = useState(todayIso())
      const [minutes, setMinutes] = useState(15)
      const [note, setNote] = useState('')
      const [error, setError] = useState('')
      const submit = async () => {
        try {
          await send('habit.log', {
            habit_id: habit.id,
            occurred_at: new Date(`${date}T12:00:00`).toISOString(),
            minutes: Number(minutes),
            note: note.trim() || null,
            dedupe_key: newId('habitlog'),
          })
          onClose()
        } catch (failure) {
          setError(failure.message)
        }
      }
      return h(Modal, { title: '记录习惯', subtitle: habit.title, onClose,
        footer: h(React.Fragment, null,
          h(Button, { onClick: onClose }, COPY.cancel),
          h(Button, { tone: 'primary', onClick: submit, disabled: busy }, '记录')) },
        error ? h('div', { className: 'zw-alert zw-alert-error' }, error) : null,
        h(Field, { label: '日期' }, h('input', { type: 'date', value: date, autoFocus: true,
          onChange: event => setDate(event.target.value) })),
        h(Field, { label: '本次分钟' }, h('input', { type: 'number', min: 1, max: 1440, value: minutes,
          onChange: event => setMinutes(event.target.value) })),
        h(Field, { label: '备注（可选）', wide: true }, h('input', { value: note,
          onChange: event => setNote(event.target.value) })),
      )
    }

    function HabitDetail({ habit, snapshot, onClose, send, busy, setModal }) {
      const weekStart = startOfWeekIso(todayIso())
      const weekEnd = addDays(weekStart, 6)
      const logs = (snapshot.activities ?? [])
        .filter(activity => activity.habit_id === habit.id)
        .slice()
        .sort((left, right) => String(right.occurred_at).localeCompare(String(left.occurred_at)))
      const weekLogs = logs.filter(row => row.occurred_at.slice(0, 10) >= weekStart && row.occurred_at.slice(0, 10) <= weekEnd)
      const habitDays = [...new Set(logs.filter(row => row.kind === 'habit').map(row => row.occurred_at.slice(0, 10)))].sort()
      const daySet = new Set(habitDays)
      const longestStreak = (() => { let best = 0, run = 0, previous = null
        for (const day of habitDays) {
          run = previous && addDays(previous, 1) === day ? run + 1 : 1
          previous = day; best = Math.max(best, run)
        }
        return best
      })()
      let currentStreak = 0
      let cursor = daySet.has(todayIso()) ? todayIso() : addDays(todayIso(), -1)
      while (daySet.has(cursor)) { currentStreak += 1; cursor = addDays(cursor, -1) }
      const skipToday = async () => {
        try { await send('habit.skip', { habit_id: habit.id, date: todayIso(), note: null }); onClose() } catch { /* global error */ }
      }
      const undoLatest = async () => {
        try { await send('habit.undo', { habit_id: habit.id }); onClose() } catch { /* global error */ }
      }
      return h(Modal, { title: '习惯详情', subtitle: habit.title, onClose, wide: true,
        footer: h(React.Fragment, null,
          h(Button, { onClick: onClose }, COPY.close),
          h(Button, { onClick: skipToday, disabled: busy || habitDays.includes(todayIso()), testid: 'habit-skip-today' }, '跳过今天'),
          h(Button, { onClick: undoLatest, disabled: busy || logs.length === 0, testid: 'habit-undo' }, '撤销最近'),
          h(Button, { onClick: () => setModal({ type: 'habit', habit }), disabled: busy }, '编辑规则'),
          h(Button, { tone: 'primary', onClick: () => setModal({ type: 'habit-log', habit }), disabled: busy }, '记录一次')) },
        h('div', { className: 'zw-big-stats' },
          h('div', null, h('strong', null, `${weekLogs.length}/${habit.weekly_target}`), h('span', null, '本周目标')),
          h('div', null, h('strong', null, weekLogs.reduce((sum, row) => sum + (row.minutes || 0), 0)), h('span', null, '本周分钟')),
          h('div', null, h('strong', null, logs.length), h('span', null, '累计记录')),
          h('div', null, h('strong', null, habit.time_of_day ? String(habit.time_of_day).slice(0, 5) : '—'), h('span', null, '偏好时刻')),
          h('div', null, h('strong', null, `${currentStreak} 天`), h('span', null, '当前连续')),
          h('div', null, h('strong', null, `${longestStreak} 天`), h('span', null, '最长连续')),
        ),
        h(Section, { title: '最近执行事实' },
          logs.length ? logs.slice(0, 30).map(row => h('div', { className: 'zw-list-row', key: row.id },
            h('span', null, row.kind === 'habit_skip' ? `${row.occurred_at.slice(0, 10)} · 跳过` : `${dateTimeFor(row.occurred_at)} · ${row.minutes} 分钟`),
            h('span', { className: 'zw-muted' }, row.note || row.source?.kind || ''),
          )) : h(Empty, null, '还没有记录。'),
        ),
        h('p', { className: 'zw-fineprint' }, `开始 ${habit.start_date}${habit.end_date ? ` · 结束 ${habit.end_date}` : ''} · 周目标 ${habit.weekly_target} 次。`),
      )
    }

    function HabitsView({ snapshot, send, busy, setModal }) {
      const weekStart = startOfWeekIso(todayIso())
      const weekEnd = addDays(weekStart, 6)
      const habits = snapshot.habits ?? []
      return h('div', { className: 'zw-page' },
        h('header', { className: 'zw-page-head' },
          h('div', null, h('div', { className: 'zw-kicker' }, '重复事务与一次例外'), h('h1', null, '习惯')),
          h(Button, { tone: 'primary', onClick: () => setModal({ type: 'habit' }), disabled: busy }, '新建习惯'),
        ),
        h('p', { className: 'zw-fineprint' }, `本周 ${weekStart} – ${weekEnd}。记录是可重建的事实；周目标不是完成度自动推断。`),
        habits.length ? h('div', { className: 'zw-goals-grid' },
          habits.map(habit => {
            const logs = (snapshot.activities ?? []).filter(activity =>
              activity.kind === 'habit' && activity.habit_id === habit.id
              && activity.occurred_at.slice(0, 10) >= weekStart
              && activity.occurred_at.slice(0, 10) <= weekEnd)
            const minutes = logs.reduce((sum, row) => sum + (row.minutes || 0), 0)
            const ratio = habit.weekly_target ? Math.min(100, Math.round((logs.length / habit.weekly_target) * 100)) : 0
            const ended = habit.end_date && habit.end_date < todayIso()
            return h('article', { className: 'zw-card', key: habit.id },
              h('header', { className: 'zw-card-head' },
                h('div', null,
                  h(Badge, { tone: ended ? 'neutral' : 'accent' }, ended ? '已结束' : '进行中'),
                  h('h3', null, habit.title),
                ),
                h('button', { className: 'zw-icon-btn', onClick: () => setModal({ type: 'habit', habit }) }, '…'),
              ),
              h('div', { className: 'zw-card-meta' },
                h('span', null, `周目标 ${logs.length}/${habit.weekly_target}`),
                h('span', null, `${minutes} 分钟`),
                h('span', null, habit.time_of_day ? `偏好 ${String(habit.time_of_day).slice(0, 5)}` : '无固定时刻'),
              ),
              h('div', { className: 'zw-progress' }, h('div', { className: 'zw-progress-fill', style: { width: `${ratio}%` } })),
              h('div', { className: 'zw-card-meta' }, h('span', null, habit.scheduled_weekdays?.map(day => `周${day}`).join(' · ') || '任意日')),
              h('div', { className: 'zw-action-row' },
                h(Button, { tone: 'primary', onClick: () => setModal({ type: 'habit-log', habit }), disabled: busy || ended }, '记录一次'),
                h(Button, { onClick: () => setModal({ type: 'habit-detail', habit }), disabled: busy }, '详情'),
                h(Button, { onClick: () => setModal({ type: 'habit', habit }), disabled: busy }, '编辑'),
              ),
            )
          }),
        ) : h(Empty, null, '还没有习惯。习惯适合重复事务，不适合一次性任务。'),
      )
    }

    function SavedViewEditor({ view, initialFilter, onClose, send, busy }) {
      const initial = view?.filter ?? initialFilter ?? {}
      const [form, setForm] = useState({
        name: view?.name ?? '',
        text: initial.text ?? '',
        status: initial.status ?? '',
        tag: initial.tag ?? '',
        overdue: Boolean(initial.overdue),
      })
      const [error, setError] = useState('')
      const submit = async () => {
        setError('')
        if (!form.name.trim()) return setError('请填写视图名称')
        const filter = {}
        if (form.text.trim()) filter.text = form.text.trim()
        if (form.status) filter.status = form.status
        if (form.tag.trim()) filter.tag = form.tag.trim()
        if (form.overdue) filter.overdue = true
        try {
          if (view) await send('saved_query.update', { id: view.id, saved_query: { name: form.name.trim(), filter } })
          else await send('saved_query.create', { saved_query: { name: form.name.trim(), filter } })
          onClose()
        } catch (failure) {
          setError(failure.message)
        }
      }
      return h(Modal, { title: view ? '编辑保存视图' : '新建保存视图', subtitle: '保存的是过滤器，不是结果快照；结果随事实更新。', onClose,
        footer: h(React.Fragment, null,
          h(Button, { onClick: onClose }, COPY.cancel),
          h(Button, { tone: 'primary', onClick: submit, disabled: busy }, COPY.save)) },
        error ? h('div', { className: 'zw-alert zw-alert-error' }, error) : null,
        h(Field, { label: '名称', wide: true }, h('input', { value: form.name, autoFocus: true,
          onChange: event => setForm({ ...form, name: event.target.value }) })),
        h(Field, { label: '文本包含', wide: true }, h('input', { value: form.text,
          onChange: event => setForm({ ...form, text: event.target.value }) })),
        h(Field, { label: '状态' }, h('select', { value: form.status,
          onChange: event => setForm({ ...form, status: event.target.value }) },
          h('option', { value: '' }, '任意'), h('option', { value: 'open' }, '待处理'), h('option', { value: 'in_progress' }, '进行中'), h('option', { value: 'done' }, '已完成'))),
        h(Field, { label: '标签' }, h('input', { value: form.tag,
          onChange: event => setForm({ ...form, tag: event.target.value }) })),
        h('label', { className: 'zw-check-line' },
          h('input', { type: 'checkbox', checked: form.overdue, onChange: event => setForm({ ...form, overdue: event.target.checked }) }),
          '只看逾期',
        ),
      )
    }

    function SavedViewsView({ snapshot, send, busy, setModal, onApply }) {
      const views = snapshot.saved_queries ?? []
      const knownFilterKeys = new Set(['text', 'status', 'tag', 'overdue', 'project_id', 'limit', 'include_archived', 'only_leaf'])
      const unresolvedKeys = filter => Object.keys(filter ?? {}).filter(key => !knownFilterKeys.has(key))
      const remove = async id => {
        try { await send('saved_query.delete', { id }) } catch { /* global error */ }
      }
      return h('div', { className: 'zw-page' },
        h('header', { className: 'zw-page-head' },
          h('div', null, h('div', { className: 'zw-kicker' }, '条件构建'), h('h1', null, '保存视图')),
          h(Button, { tone: 'primary', onClick: () => setModal({ type: 'saved-view' }), disabled: busy }, '新建视图'),
        ),
        h('p', { className: 'zw-fineprint' }, '视图保存过滤条件而不是结果；运行时会按当前事实重新求值。'),
        views.length ? h('div', { className: 'zw-goals-grid' },
          views.map(view => h('article', { className: 'zw-card', key: view.id },
            h('header', { className: 'zw-card-head' },
              h('div', null, h('h3', null, view.name), h('span', { className: 'zw-muted' }, '条件')),
              h('button', { className: 'zw-icon-btn', onClick: () => setModal({ type: 'saved-view', view }) }, '…'),
            ),
            unresolvedKeys(view.filter).length ? h('p', { className: 'zw-alert zw-alert-warn', 'data-testid': `saved-view-unresolved-${view.id}` }, `未解析条件：${unresolvedKeys(view.filter).join(', ')}；应用时会被忽略，不会静默当作已支持`) : null,
            h('pre', { className: 'zw-filter-code' }, JSON.stringify(view.filter, null, 2)),
            h('div', { className: 'zw-action-row' },
              h(Button, { tone: 'primary', onClick: () => onApply(view), disabled: busy }, '应用'),
              h(Button, { onClick: () => setModal({ type: 'saved-view', view }), disabled: busy }, '编辑'),
              h(Button, { tone: 'danger', onClick: () => remove(view.id), disabled: busy }, '删除'),
            ),
          )),
        ) : h(Empty, null, '还没有保存视图。先在任务页筛选，再保存条件。'),
      )
    }

    function MilestoneEditor({ project, milestone, onClose, send, busy }) {
      const [form, setForm] = useState({
        title: milestone?.title ?? '',
        due: milestone?.due ?? '',
        done: milestone?.done ?? false,
      })
      const [error, setError] = useState('')
      const submit = async () => {
        if (!form.title.trim()) return setError('请填写里程碑标题')
        try {
          const spec = { title: form.title.trim(), due: form.due || null, done: form.done }
          if (milestone) await send('milestone.update', { project_id: project.id, milestone_id: milestone.id, milestone: spec })
          else await send('milestone.add', { project_id: project.id, milestone: spec })
          onClose()
        } catch (failure) {
          setError(failure.message)
        }
      }
      return h(Modal, { title: milestone ? '编辑里程碑' : '添加里程碑', subtitle: project.title, onClose,
        footer: h(React.Fragment, null,
          h(Button, { onClick: onClose }, COPY.cancel),
          h(Button, { tone: 'primary', onClick: submit, disabled: busy }, COPY.save)) },
        error ? h('div', { className: 'zw-alert zw-alert-error' }, error) : null,
        h(Field, { label: '标题', wide: true }, h('input', { value: form.title, autoFocus: true,
          onChange: event => setForm({ ...form, title: event.target.value }) })),
        h(Field, { label: '截止日期' }, h('input', { type: 'date', value: form.due,
          onChange: event => setForm({ ...form, due: event.target.value }) })),
        h('label', { className: 'zw-check-line' },
          h('input', { type: 'checkbox', checked: form.done, onChange: event => setForm({ ...form, done: event.target.checked }) }),
          '已完成',
        ),
        h('p', { className: 'zw-fineprint' }, '里程碑完成独立于任务完成率。'),
      )
    }

    function AutomationEditor({ onClose, send, busy }) {
      const [form, setForm] = useState({ name: '', taskTitle: '自动创建的任务', estimate: 30, budget: 10, auto: false, interval: 3600 })
      const [error, setError] = useState('')
      const submit = async () => {
        setError('')
        if (!form.name.trim() || !form.taskTitle.trim()) return setError('请填写规则名称和动作标题')
        try {
          await send('automation.create', {
            automation: {
              name: form.name.trim(),
              trigger: form.auto ? 'schedule' : 'manual',
              conditions: form.auto ? { interval_seconds: Math.max(60, Number(form.interval) || 3600) } : {},
              actions: [{
                kind: 'task.create',
                task: {
                  title: form.taskTitle.trim(),
                  estimate_minutes: Number(form.estimate) || 30,
                  priority: 2,
                },
              }],
              enabled: true,
              budget: Number(form.budget) || 10,
            },
          })
          onClose()
        } catch (failure) {
          setError(failure.message)
        }
      }
      return h(Modal, { title: '新建自动化规则', subtitle: '调度器每 5 秒检查一次；最短间隔 60 秒，预算用尽自动停用。', onClose,
        footer: h(React.Fragment, null,
          h(Button, { onClick: onClose }, COPY.cancel),
          h(Button, { tone: 'primary', onClick: submit, disabled: busy }, COPY.save)) },
        error ? h('div', { className: 'zw-alert zw-alert-error' }, error) : null,
        h(Field, { label: '规则名称', wide: true }, h('input', { value: form.name, autoFocus: true,
          onChange: event => setForm({ ...form, name: event.target.value }) })),
        h(Field, { label: '动作：创建任务标题', wide: true }, h('input', { value: form.taskTitle,
          onChange: event => setForm({ ...form, taskTitle: event.target.value }) })),
        h('div', { className: 'zw-grid2' },
          h(Field, { label: '预计分钟' }, h('input', { type: 'number', min: 1, value: form.estimate,
            onChange: event => setForm({ ...form, estimate: event.target.value }) })),
          h(Field, { label: '最多运行次数' }, h('input', { type: 'number', min: 1, max: 100, value: form.budget,
            onChange: event => setForm({ ...form, budget: event.target.value }) })),
        ),
        h('label', { className: 'zw-check-line' },
          h('input', { type: 'checkbox', checked: form.auto,
            onChange: event => setForm({ ...form, auto: event.target.checked }) }),
          '按时间自动触发（否则仅手动运行）',
        ),
        form.auto ? h(Field, { label: '最小间隔（秒，≥60）' }, h('input', { type: 'number', min: 60, value: form.interval,
          onChange: event => setForm({ ...form, interval: event.target.value }) })) : null,
        h('p', { className: 'zw-fineprint' }, '每次运行都有预算和间隔约束；失败会记录事件，不会假装成功。'),
      )
    }

    function AutomationView({ snapshot, send, busy, setModal, report }) {
      const rules = snapshot.automation_rules ?? []
      const toggle = async rule => {
        try { await send('automation.toggle', { id: rule.id, enabled: !rule.enabled }) } catch { /* global */ }
      }
      const run = async rule => {
        try { await send('automation.run', { id: rule.id }); report('自动化已运行并产生可审计回执。') } catch { /* global */ }
      }
      const remove = async rule => {
        try { await send('automation.delete', { id: rule.id }) } catch { /* global */ }
      }
      return h('div', { className: 'zw-page' },
        h('header', { className: 'zw-page-head' },
          h('div', null, h('div', { className: 'zw-kicker' }, '有界触发与动作'), h('h1', null, '自动化')),
          h(Button, { tone: 'primary', onClick: () => setModal({ type: 'automation' }), disabled: busy }, '新建规则'),
        ),
        h('p', { className: 'zw-fineprint' }, '当前 A4 只实现人工触发的有界任务创建；条件和预算会被记录，自动调度/循环防护的完整矩阵仍待补齐。'),
        rules.length ? h('div', { className: 'zw-goals-grid' },
          rules.map(rule => h('article', { className: 'zw-card', key: rule.id },
            h('header', { className: 'zw-card-head' },
              h('div', null, h(Badge, { tone: rule.enabled ? 'accent' : 'neutral' }, rule.enabled ? '启用' : '停用'), h('h3', null, rule.name)),
              h('span', { className: 'zw-mono zw-muted' }, rule.id.slice(0, 12)),
            ),
            h('div', { className: 'zw-card-meta' },
              h('span', null, `触发 ${rule.trigger}`),
              h('span', null, `运行 ${rule.runs}/${rule.budget}`),
              h('span', null, rule.last_run_at ? `上次 ${dateTimeFor(rule.last_run_at)}` : '尚未运行'),
            ),
            h('pre', { className: 'zw-filter-code' }, JSON.stringify(rule.actions, null, 2)),
            h('div', { className: 'zw-action-row' },
              h(Button, { tone: 'primary', onClick: () => run(rule), disabled: busy || !rule.enabled || rule.runs >= rule.budget }, '手动运行'),
              h(Button, { onClick: () => toggle(rule), disabled: busy }, rule.enabled ? '停用' : '启用'),
              h(Button, { tone: 'danger', onClick: () => remove(rule), disabled: busy }, '删除'),
            ),
          )),
        ) : h(Empty, null, '还没有自动化规则。规则不是事实，运行产生的任务才是事实。'),
      )
    }

    function SyncView({ snapshot, report, refresh }) {
      const [busy, setBusy] = useState(false)
      const [error, setError] = useState('')
      const [result, setResult] = useState(null)
      const [deviceId, setDeviceId] = useState('')
      const importRef = useRef(null)
      const download = async () => {
        setBusy(true)
        setError('')
        try {
          const bundle = await syncExport()
          setDeviceId(bundle.device_id ?? '')
          const blob = new Blob([JSON.stringify(bundle, null, 2)], { type: 'application/json' })
          const url = URL.createObjectURL(blob)
          const anchor = document.createElement('a')
          anchor.href = url
          anchor.download = `zodo-sync-${todayIso()}.json`
          anchor.click()
          URL.revokeObjectURL(url)
          report('同步包已导出；需要人工在另一设备导入。')
        } catch (failure) {
          setError(failure.message)
        } finally {
          setBusy(false)
        }
      }
      const importBundle = async file => {
        if (!file) return
        setBusy(true)
        setError('')
        try {
          const bundle = JSON.parse(await file.text())
          const imported = await syncImport(bundle)
          setResult(imported)
          setDeviceId(imported.device_id ?? '')
          await refresh()
          report(imported.added ? `已合并 ${imported.added} 个实体。` : '同步包与本地事实一致。')
        } catch (failure) {
          setError(failure.message)
        } finally {
          setBusy(false)
          if (importRef.current) importRef.current.value = ''
        }
      }
      return h('div', { className: 'zw-page' },
        h('header', { className: 'zw-page-head' },
          h('div', null, h('div', { className: 'zw-kicker' }, '设备包与冲突'), h('h1', null, '同步')),
          h('div', { className: 'zw-head-actions' },
            h(Button, { onClick: download, disabled: busy, testid: 'sync-export' }, '导出同步包'),
            h(Button, { onClick: () => importRef.current?.click(), disabled: busy, testid: 'sync-import' }, '导入同步包'),
          ),
        ),
        h('input', {
          type: 'file', accept: 'application/json,.json', style: { display: 'none' }, ref: importRef,
          onChange: event => importBundle(event.target.files?.[0]),
        }),
        error ? h('div', { className: 'zw-alert zw-alert-error' }, error) : null,
        h('div', { className: 'zw-alert zw-alert-warn' }, '当前 A6 是离线包合并：只新增两边不存在的实体；同一实体内容不同则整体拒绝并列出冲突，不做静默覆盖。删除/加密传输/自动发现尚未实现。'),
        h(Section, { title: '本地设备' },
          h('dl', { className: 'zw-kv' },
            h('dt', null, '设备 ID'), h('dd', { className: 'zw-mono' }, deviceId || '导出/导入后显示'),
            h('dt', null, '数据版本'), h('dd', null, `r${snapshot.workspace.revision}`),
            h('dt', null, '工作空间'), h('dd', null, snapshot.workspace.id),
          ),
        ),
        result ? h(Section, { title: '最近导入结果' },
          h('pre', { className: 'zw-filter-code' }, JSON.stringify(result, null, 2)),
        ) : null,
        h('p', { className: 'zw-fineprint' }, '冲突时 Core 不写入任何部分；请先导出冲突双方的人工决定，再导入无冲突包。'),
      )
    }

    function DataView({ snapshot, report, refresh, send }) {
      const [backups, setBackups] = useState([])
      const [csvPreview, setCsvPreview] = useState(null)
      const [csvFile, setCsvFile] = useState(null)
      const [busy, setBusy] = useState(false)
      const [error, setError] = useState('')
      const [lastBackup, setLastBackup] = useState(null)
      const [pendingImport, setPendingImport] = useState(null)
      const importRef = useRef(null)
      const csvImportRef = useRef(null)
      const parseCsv = async file => {
        if (!file) return
        setError('')
        try {
          const text = await file.text()
          const lines = text.split(/\r?\n/).filter(line => line.trim() !== '')
          if (lines.length < 2) throw new Error('CSV 需要表头和至少一行数据')
          const headers = lines[0].split(',').map(value => value.trim())
          const required = ['title']
          for (const name of required) if (!headers.includes(name)) throw new Error(`CSV 缺少 ${name} 列`)
          const rows = lines.slice(1).map((line, index) => {
            const values = line.split(',').map(value => value.trim())
            const row = Object.fromEntries(headers.map((header, position) => [header, values[position] ?? '']))
            if (!row.title) throw new Error(`CSV 第 ${index + 2} 行缺少标题`)
            return {
              title: row.title,
              estimate_minutes: row.estimate_minutes ? Number(row.estimate_minutes) : null,
              priority: row.priority ? Number(row.priority) : 2,
              tags: row.tags ? row.tags.split('|').map(tag => tag.trim()).filter(Boolean) : [],
            }
          })
          setCsvFile(file)
          setCsvPreview({ rows, fileName: file.name })
        } catch (failure) {
          setError(failure.message)
        }
      }
      const confirmCsvImport = async () => {
        if (!csvPreview) return
        setBusy(true)
        setError('')
        try {
          for (const row of csvPreview.rows) {
            await command('task.create', { task: {
              title: row.title,
              estimate_minutes: row.estimate_minutes || null,
              priority: row.priority,
              tags: row.tags,
            } }, undefined)
          }
          report(`CSV 已导入 ${csvPreview.rows.length} 个任务；不含凭据。`)
          setCsvPreview(null)
          setCsvFile(null)
          await refresh()
        } catch (failure) {
          setError(failure.message)
        } finally {
          setBusy(false)
          if (csvImportRef.current) csvImportRef.current.value = ''
        }
      }
      const loadBackups = async () => {
        try {
          const result = await listBackups()
          setBackups(result.backups ?? [])
        } catch (failure) {
          setError(failure.message)
        }
      }
      useEffect(() => { void loadBackups() }, [])
      const downloadExport = async () => {
        setBusy(true)
        setError('')
        try {
          const data = await exportData()
          const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' })
          const url = URL.createObjectURL(blob)
          const anchor = document.createElement('a')
          anchor.href = url
          anchor.download = `zodo-export-${todayIso()}.json`
          anchor.click()
          URL.revokeObjectURL(url)
          report('导出已生成；包含当前工作空间事实，不含任何 token 或 API key。')
        } catch (failure) {
          setError(failure.message)
        } finally {
          setBusy(false)
        }
      }
      const downloadCsv = async () => {
        setBusy(true)
        setError('')
        try {
          const response = await fetch(`${BRIDGE}/data/export.csv`, { credentials: 'same-origin', headers: bridgeToken ? { 'X-Zodo-UI-Session': bridgeToken } : {} })
          if (!response.ok) throw new Error(`CSV 导出失败 (${response.status})`)
          const blob = await response.blob()
          const url = URL.createObjectURL(blob)
          const anchor = document.createElement('a')
          anchor.href = url
          anchor.download = `zodo-tasks-${todayIso()}.csv`
          anchor.click()
          URL.revokeObjectURL(url)
          report('任务 CSV 已导出；不含 token 或模型凭据。')
        } catch (failure) {
          setError(failure.message)
        } finally {
          setBusy(false)
        }
      }
      const backup = async () => {
        setBusy(true)
        setError('')
        try {
          const result = await createBackup()
          setLastBackup(result)
          report('SQLite 备份已创建。')
          await loadBackups()
        } catch (failure) {
          setError(failure.message)
        } finally {
          setBusy(false)
        }
      }
      const importBundle = async file => {
        if (!file) return
        setBusy(true)
        setError('')
        try {
          const bundle = JSON.parse(await file.text())
          const preflight = await bridgeJson('/data/import/preflight', {
            method: 'POST',
            body: JSON.stringify({ bundle }),
          })
          setPendingImport({ bundle, preflight })
        } catch (failure) {
          setError(failure.message)
        } finally {
          setBusy(false)
          if (importRef.current) importRef.current.value = ''
        }
      }
      const confirmImport = async () => {
        if (!pendingImport) return
        setBusy(true)
        setError('')
        try {
          await bridgeJson('/data/import', {
            method: 'POST',
            body: JSON.stringify({ bundle: pendingImport.bundle }),
          })
          report('JSON 包已导入空工作空间；token/凭据没有导入。')
          setPendingImport(null)
          await refresh()
        } catch (failure) {
          setError(failure.message)
        } finally {
          setBusy(false)
        }
      }
      return h('div', { className: 'zw-page' },
        h('header', { className: 'zw-page-head' },
          h('div', null, h('div', { className: 'zw-kicker' }, '导出、备份与恢复'), h('h1', null, '数据')),
          h('div', { className: 'zw-head-actions' },
            h(Button, { onClick: downloadExport, disabled: busy, testid: 'data-export' }, '导出 JSON'),
            h(Button, { onClick: downloadCsv, disabled: busy, testid: 'data-export-csv' }, '导出任务 CSV'),
            h(Button, { onClick: () => importRef.current?.click(), disabled: busy, testid: 'data-import' }, '导入 JSON'),
            h(Button, { onClick: () => csvImportRef.current?.click(), disabled: busy, testid: 'data-import-csv' }, '导入 CSV'),
            h(Button, { tone: 'primary', onClick: backup, disabled: busy, testid: 'data-backup' }, '立即备份'),
          ),
        ),
        h('input', {
          type: 'file',
          accept: 'application/json,.json',
          style: { display: 'none' },
          ref: importRef,
          onChange: event => importBundle(event.target.files?.[0]),
        }),
        h('input', {
          type: 'file',
          accept: 'text/csv,.csv',
          style: { display: 'none' },
          ref: csvImportRef,
          onChange: event => parseCsv(event.target.files?.[0]),
        }),
        error ? h('div', { className: 'zw-alert zw-alert-error' }, error) : null,
        csvPreview ? h(Modal, {
          title: 'CSV 导入预览',
          subtitle: `${csvPreview.fileName} · ${csvPreview.rows.length} 行；确认后逐条创建任务，不含凭据。`,
          onClose: () => { setCsvPreview(null); setCsvFile(null) },
          wide: true,
          footer: h(React.Fragment, null,
            h(Button, { onClick: () => { setCsvPreview(null); setCsvFile(null) } }, '取消'),
            h(Button, { tone: 'primary', onClick: confirmCsvImport, disabled: busy, testid: 'data-csv-confirm' }, '确认导入'),
          ),
        },
        h('pre', { className: 'zw-filter-code' }, JSON.stringify(csvPreview.rows, null, 2)),
        ) : null,
        pendingImport ? h(Modal, {
          title: '导入预检',
          subtitle: '只读预检；确认后才会写入。冲突时 Core 整体拒绝，不会静默覆盖。',
          onClose: () => setPendingImport(null),
          wide: true,
          footer: h(React.Fragment, null,
            h(Button, { onClick: () => setPendingImport(null) }, '取消'),
            h(Button, { tone: 'primary', onClick: confirmImport, disabled: busy, testid: 'data-import-confirm' }, '确认导入'),
          ),
        },
        h('pre', { className: 'zw-filter-code' }, JSON.stringify(pendingImport.preflight, null, 2)),
        h('p', { className: 'zw-fineprint' }, '预检不包含 token、Cookie 或模型凭据；导入只会新增缺失实体。'),
        ) : null,
        h('div', { className: 'zw-alert zw-alert-warn' }, '导出包含私密事实，只保存在你选择的本地位置；备份与导出都不包含 Core token、Cookie、模型 API key。'),
        h(Section, { title: '当前事实规模' },
          h('div', { className: 'zw-big-stats' },
            h('div', null, h('strong', null, snapshot.tasks?.length ?? 0), h('span', null, '任务')),
            h('div', null, h('strong', null, snapshot.habits?.length ?? 0), h('span', null, '习惯')),
            h('div', null, h('strong', null, snapshot.activities?.length ?? 0), h('span', null, '执行记录')),
            h('div', null, h('strong', null, snapshot.proposals?.length ?? 0), h('span', null, '提案')),
          ),
          h('p', { className: 'zw-fineprint' }, `数据版本 r${snapshot.workspace.revision}；Core 的 SQLite 是权威事实源。`),
        ),
        h(Section, { title: 'SQLite 备份' },
          lastBackup ? h('div', { className: 'zw-alert zw-alert-warn' },
            `已创建 ${lastBackup.file} · ${lastBackup.size_bytes} bytes · sha256 ${lastBackup.sha256?.slice(0, 16)}…`) : null,
          backups.length ? h('div', { className: 'zw-table' },
            h('div', { className: 'zw-table-head' }, h('span', null, '文件'), h('span', null, '大小'), h('span', null, 'sha256')),
            backups.map(item => h('div', { className: 'zw-table-row', key: item.file },
              h('span', null, item.file), h('span', null, `${item.size_bytes} B`), h('span', { className: 'zw-mono' }, item.sha256?.slice(0, 18)),
            )),
          ) : h(Empty, null, '还没有备份。备份在 `.runtime/<profile>/backups/` 下创建。'),
          h('p', { className: 'zw-fineprint' }, '恢复必须离线进行：停止 Core，校验备份兼容性后替换 SQLite；运行时不会热恢复，也不会复活旧 token。'),
        ),
        h(Section, { title: '恢复边界' },
          h('p', null, 'A4 提供可校验备份和 JSON 导出；离线恢复命令与兼容性预检在 A6 多设备/恢复阶段完成。当前页面不提供会覆盖事实的热恢复按钮。'),
          h('p', { className: 'zw-fineprint' }, '备份文件由 Core 创建并计算 SHA-256；浏览器只接收文件名/大小/哈希，不接收私有正文。'),
        ),
      )
    }

    function SettingsView({ snapshot, send, busy, report }) {
      const [form, setForm] = useState({
        timezone: snapshot.workspace.settings.timezone,
        weekend: snapshot.workspace.settings.weekend_policy,
        defaultEstimate: snapshot.workspace.settings.default_estimate_minutes,
      })
      const [error, setError] = useState('')
      const save = async () => {
        setError('')
        try {
          await send('workspace.update', {
            settings: {
              ...snapshot.workspace.settings,
              timezone: form.timezone,
              weekend_policy: form.weekend,
              default_estimate_minutes: Number(form.defaultEstimate) || 30,
            },
            policy: null,
          })
          report('设置已保存')
        } catch (failure) {
          setError(failure.message)
        }
      }
      return h('div', { className: 'zw-page' },
        h('header', { className: 'zw-page-head' },
          h('div', null, h('div', { className: 'zw-kicker' }, 'Zodo 局部设置'), h('h1', null, '设置')),
          h(Button, { tone: 'primary', onClick: save, disabled: busy }, COPY.save),
        ),
        error ? h('div', { className: 'zw-alert zw-alert-error' }, error) : null,
        h(Section, { title: '时间与工作窗口' },
          h('div', { className: 'zw-grid2' },
            h(Field, { label: 'IANA 时区' }, h('input', { value: form.timezone,
              onChange: event => setForm({ ...form, timezone: event.target.value }) })),
            h(Field, { label: '周末策略' }, h('select', { value: form.weekend,
              onChange: event => setForm({ ...form, weekend: event.target.value }) },
              h('option', { value: 'ask' }, '每次询问'), h('option', { value: 'allow' }, '允许'), h('option', { value: 'deny' }, '禁止'))),
            h(Field, { label: '默认估时（分钟）' }, h('input', { type: 'number', value: form.defaultEstimate,
              onChange: event => setForm({ ...form, defaultEstimate: event.target.value }) })),
            h('div', { 'data-testid': 'settings-daily-limit' },
              h('span', null, `每日上限（保留值）：${snapshot.workspace.settings.daily_limit_minutes == null ? '未设置' : `${snapshot.workspace.settings.daily_limit_minutes} 分钟`}`),
              h('p', { className: 'zw-fineprint' }, '尚未启用，不参与自动排程或指定时段安排。保存其他设置不会更改此保留值。'),
            ),
          ),
          h('div', { className: 'zw-workwindows' },
            (snapshot.workspace.settings.work_windows ?? []).map((window, index) =>
              h('span', { className: 'zw-workwindow', key: index },
                `周${window.weekday} ${String(Math.floor(window.start_minute / 60)).padStart(2, '0')}:${String(window.start_minute % 60).padStart(2, '0')}–${String(Math.floor(window.end_minute / 60)).padStart(2, '0')}:${String(window.end_minute % 60).padStart(2, '0')}`,
              )),
          ),
          h('p', { className: 'zw-fineprint' }, '工作窗口是规划偏好；容量块决定当前可用计划容器。硬约束不会被静默放宽。'),
        ),
        h(Section, { title: '数据与运行位置' },
          h('dl', { className: 'zw-kv' },
            h('dt', null, '工作空间'), h('dd', null, snapshot.workspace.id),
            h('dt', null, '时区'), h('dd', null, snapshot.workspace.settings.timezone),
            h('dt', null, '数据版本'), h('dd', null, `r${snapshot.workspace.revision}`),
            h('dt', null, '策略版本'), h('dd', null, `p${snapshot.workspace.policy_revision}`),
            h('dt', null, '核心位置'), h('dd', null, '由 Host Bridge 显示，浏览器不直接访问 SQLite'),
          ),
        ),
        h(Section, { title: '时间连接与日历源' },
          snapshot.calendar_sources?.length
            ? h('div', { className: 'zw-list' }, snapshot.calendar_sources.map(source =>
                h('div', { className: 'zw-list-row', key: source.id, 'data-testid': `settings-calendar-${source.id}` },
                  h('span', null, source.display_name),
                  h('span', { className: 'zw-muted' }, `${source.provider} · ${source.writable ? '可写' : '只读'} · 同步 ${calendarFreshness(source) === '未知' ? '未知' : dateTimeFor(source.last_sync_at)}`),
                )))
            : h(Empty, null, '尚未连接外部日历；忙碌快照仍可从日程页手动导入。'),
          h('p', { className: 'zw-fineprint' }, '时间连接只显示本地日历源快照；浏览器不直接访问外部账号，也不支持自动写入。'),
        ),
        h(Section, { title: '无模型手工路径' },
          h('p', null, '收集、任务/目标/项目编辑、计划预览与提交、进度和回顾都可在无模型时完成。模型不可用时草稿不会丢失。'),
        ),
      )
    }

    function PrivacyView({ snapshot, send, busy, report, onModels }) {
      const policy = snapshot.workspace.policy
      const valid = policy.model_access && ['goals', 'projects', 'captures'].every(key => typeof policy.model_access[key] === 'boolean')
      const [access, setAccess] = useState({ ...policy.model_access })
      const [diagnostics, setDiagnostics] = useState(policy.diagnostic_capture)
      const [dirty, setDirty] = useState(false)
      const [stale, setStale] = useState(false)
      const [saving, setSaving] = useState(false)
      const [error, setError] = useState('')
      const [message, setMessage] = useState('')
      const dirtyRef = useRef(false)
      const baseline = useRef(JSON.stringify(policy))
      const reset = (clearFeedback = true) => {
        setAccess({ ...policy.model_access }); setDiagnostics(policy.diagnostic_capture)
        baseline.current = JSON.stringify(policy); dirtyRef.current = false
        setDirty(false); setStale(false)
        if (clearFeedback) { setError(''); setMessage('') }
      }
      useEffect(() => {
        if (!dirtyRef.current) reset(false)
        else if (JSON.stringify(policy) !== baseline.current) setStale(true)
      }, [JSON.stringify(policy)])
      const changed = () => { dirtyRef.current = true; setDirty(true); setMessage(''); setError('') }
      const save = async () => {
        if (!valid || stale) return
        if (JSON.stringify(policy) !== baseline.current) { setStale(true); return }
        setSaving(true); setError(''); setMessage('')
        const next = { model_access: access, diagnostic_capture: diagnostics }
        try {
          await send('workspace.update', { settings: null, policy: next })
          baseline.current = JSON.stringify(next); dirtyRef.current = false; setDirty(false)
          setMessage('隐私设置已保存；模型选择未改变。'); report('隐私设置已保存；模型选择未改变。')
        } catch (failure) { setError(failure.message) }
        finally { setSaving(false) }
      }
      const disabled = busy || saving || !valid
      const labels = [
        ['goals', '目标内容', '允许目标拆解使用目标标题、成功标准、截止日期和计划视界。关闭后，不会为目标拆解调用模型。'],
        ['projects', '项目背景', '允许附带关联项目的名称和范围。关闭后仍可拆解目标，但请求中会省略项目背景。'],
        ['captures', '收集原文', '允许澄清功能使用你主动收集的原文。关闭后，不会为收集澄清调用模型。'],
      ]
      return h('div', { className: 'zw-page zw-ai-settings' },
        h('header', { className: 'zw-page-head' },
          h('div', null, h('div', { className: 'zw-kicker' }, '控制 Zodo 内置 AI 的资料范围'), h('h1', null, '隐私')),
          h(Button, { tone: 'primary', onClick: save, disabled: disabled || !dirty || stale, testid: 'privacy-save' }, '保存隐私设置')),
        h('p', { className: 'zw-fineprint' }, '这些开关控制 Zodo 的目标拆解与收集澄清，同时适用于 DSH 模型和自定义模型。远端模型会把获准内容发送给其提供商；本机地址也不自动获得额外权限。'),
        !valid ? h('div', { className: 'zw-alert zw-alert-error', role: 'alert' }, 'Core 隐私接口无效，请检查插件与 Core 构建是否一致。') : null,
        error ? h('div', { className: 'zw-alert zw-alert-error', role: 'alert' }, error) : null,
        h('div', { 'data-testid': 'privacy-status', role: 'status' },
          stale ? h('div', { className: 'zw-alert zw-alert-warn' }, '隐私设置已在其他界面变化；未覆盖服务器。请重新载入后再修改。')
            : message ? h('p', { className: 'zw-fineprint' }, message)
            : dirty ? h('p', { className: 'zw-fineprint' }, '更改尚未保存，当前请求仍使用已保存的权限。') : null),
        h(Section, { title: '允许模型使用的资料' },
          h('div', { className: 'zw-privacy-options' }, labels.map(([key, title, description]) => h('label', { className: 'zw-source-option', key },
            h('input', { type: 'checkbox', checked: access[key] === true, disabled, 'data-testid': 'privacy-' + key,
              onChange: event => { setAccess({ ...access, [key]: event.target.checked }); changed() } }),
            h('span', null, h('strong', null, title), h('span', null, description))))),
          h('p', { className: 'zw-fineprint' }, '默认全部关闭。请只开启你愿意交给所选模型的内容；这些开关不是自动脱敏，不要把禁止外发的内容复制进已授权字段。')),
        h(Section, { title: '始终保留的边界' },
          h('p', null, '不会自动附加笔记正文、附件或 DSH 聊天历史。笔记与附件自身的数据等级不会因为模型选择或这些开关而改写。'),
          h('p', { className: 'zw-fineprint' }, '明确标记为禁止外发的内容，不交给无法验证目的地的模型。模型只能提出候选，不能自行审批或提交。')),
        h(Section, { title: '其他资料通道' },
          h('p', { className: 'zw-fineprint' }, '这三个开关不控制 DSH 聊天中主动请求的工具读取、文件读取、导出或手动粘贴。那些通道由 DSH 权限、内容等级与各自的确认流程控制，不应视为已被这些开关阻止。')),
        h('details', { className: 'zw-ai-advanced', 'data-testid': 'privacy-advanced' }, h('summary', null, '高级诊断'),
          h('label', { className: 'zw-check-line' }, h('input', { type: 'checkbox', checked: diagnostics, disabled, 'data-testid': 'privacy-diagnostics',
            onChange: event => { setDiagnostics(event.target.checked); changed() } }), '启用原文诊断'),
          h('p', { className: 'zw-fineprint' }, '诊断开关不是历史日志清除开关。运行记录、导出和备份仍应按敏感资料保管。')),
        h('div', { className: 'zw-action-row' }, dirty || stale ? h(Button, { onClick: reset, disabled: busy || saving, testid: 'privacy-reset' }, '放弃更改并重新载入') : null,
          h(Button, { onClick: onModels }, '选择模型')),
      )
    }

    function AgentsView({ bridge, snapshot }) {
      const [busy, setBusy] = useState(false)
      const [message, setMessage] = useState('')
      const [error, setError] = useState('')
      // null means this page has not observed a revocation list yet. The bridge exposes the revoke
      // action but no auth-state query, so the UI must not pretend to know the current state.
      const [knownRevoked, setKnownRevoked] = useState(null)
      useEffect(() => {
        let active = true
        getRevocations()
          .then(result => {
            if (!active) return
            const list = Array.isArray(result?.revoked_channels) ? result.revoked_channels : []
            setKnownRevoked(list)
          })
          .catch(() => {})
        return () => { active = false }
      }, [])
      const [rulesChannel, setRulesChannel] = useState(null)
      const [rulesBusy, setRulesBusy] = useState(false)
      const [rulesMessage, setRulesMessage] = useState('')
      const [rulesError, setRulesError] = useState('')
      const applyRevokeResult = (label, revoked, result) => {
        const list = Array.isArray(result?.revoked_channels) ? result.revoked_channels : null
        if (list) setKnownRevoked(list)
        setMessage(`${label} 通道已${revoked ? '撤销' : '恢复'}；撤销后其后续请求由 Core 拒绝。当前撤销通道：${(list ?? []).join(', ') || '无'}`)
        return list
      }
      const toggle = async (channel, revoked) => {
        setBusy(true)
        setError('')
        setMessage('')
        try {
          const result = await revokeChannel(channel, revoked)
          applyRevokeResult(channel, revoked, result)
        } catch (failure) {
          setError(failure.message)
        } finally {
          setBusy(false)
        }
      }
      const toggleFromRules = async (rule, revoked) => {
        setRulesBusy(true)
        setRulesError('')
        setRulesMessage('')
        try {
          const result = await revokeChannel(rule.alias, revoked)
          const list = applyRevokeResult(rule.label, revoked, result)
          setRulesMessage(`${rule.channel} ${revoked
            ? '已撤销：该通道后续请求会被 Core 拒绝。'
            : '已恢复：原 token 重新通过鉴权。'}${list ? ` Core 返回的撤销通道：${list.join(', ') || '无'}` : ''}`)
        } catch (failure) {
          setRulesError(failure.message)
        } finally {
          setRulesBusy(false)
        }
      }
      const revokedTone = channel => knownRevoked == null ? 'neutral' : knownRevoked.includes(channel) ? 'danger' : 'accent'
      const revokedLabel = channel => knownRevoked == null ? '未知' : knownRevoked.includes(channel) ? '已撤销' : '未撤销'
      const rules = rulesChannel ? channelRule(rulesChannel) : null
      const rulesRevoked = rules && knownRevoked != null ? knownRevoked.includes(rules.channel) : null
      const closeRules = () => {
        setRulesChannel(null)
        setRulesMessage('')
        setRulesError('')
      }
      return h('div', { className: 'zw-page' },
        h('header', { className: 'zw-page-head' },
          h('div', null, h('div', { className: 'zw-kicker' }, '可信调用'), h('h1', null, 'Agent 与授权')),
        ),
        error ? h('div', { className: 'zw-alert zw-alert-error' }, error) : null,
        message ? h('div', { className: 'zw-alert zw-alert-warn' }, message) : null,
        h(Section, { title: '通道身份' },
          h('p', { className: 'zw-fineprint' }, '每一行都是 Core 运行时签发的通道主体；点开一行查看读写范围、暂停规则与撤权动作。'),
          h('div', { className: 'zw-table' },
            h('div', { className: 'zw-table-head zw-channel-head' },
              h('span', null, '通道'), h('span', null, '默认能力'), h('span', null, '授权状态'), h('span', null, '说明')),
            CHANNEL_RULES.map(rule => h('button', {
              type: 'button',
              key: rule.channel,
              className: 'zw-table-row zw-channel-row',
              onClick: () => { setRulesChannel(rule.channel); setRulesMessage(''); setRulesError('') },
              'data-testid': `channel-${rule.channel}`,
            },
            h('span', null, rule.label),
            h('span', null, rule.scopes.map(scope => SCOPE_SHORT_LABELS[scope] ?? scope).join(' / ')),
            h('span', null, h(Badge, { tone: revokedTone(rule.channel) }, revokedLabel(rule.channel))),
            h('span', { className: 'zw-muted' }, rule.summary),
            )),
          ),
          knownRevoked == null ? h('p', { className: 'zw-fineprint' }, '正在读取 Core 的授权状态…') : null,
        ),
        h(Section, { title: '通道撤权（快速操作）' },
          h('p', { className: 'zw-fineprint' }, '撤权是 Core 运行时层的强制拒绝，并持久化到 auth.json；恢复后原 token 才能继续使用。撤销 UI 自身会切断当前管理面，需谨慎。'),
          h('div', { className: 'zw-action-row' },
            h(Button, { onClick: () => toggle('agent', true), disabled: busy, testid: 'revoke-agent' }, '撤销 Agent'),
            h(Button, { onClick: () => toggle('agent', false), disabled: busy }, '恢复 Agent'),
            h(Button, { onClick: () => toggle('mcp', true), disabled: busy, testid: 'revoke-mcp' }, '撤销 MCP'),
            h(Button, { onClick: () => toggle('mcp', false), disabled: busy }, '恢复 MCP'),
          ),
        ),
        h(Section, { title: '当前连接' },
          h('dl', { className: 'zw-kv' },
            h('dt', null, 'Core 协议'), h('dd', null, bridge?.core?.protocol ?? '未知'),
            h('dt', null, 'Core 实例'), h('dd', { className: 'zw-mono' }, bridge?.core?.instance_id ?? '未知'),
            h('dt', null, '数据版本'), h('dd', null, `r${snapshot.workspace.revision}`),
          ),
          h('p', { className: 'zw-fineprint' }, '浏览器从未收到 Core UI/Agent token。Agent 工具执行许可不等于业务批准。'),
        ),
        rules ? h(Modal, {
          title: `撤权与暂停规则 · ${rules.label}`,
          subtitle: `通道 ${rules.channel} · 主体 ${rules.principal}`,
          onClose: closeRules,
          footer: h(React.Fragment, null,
            h(Button, { onClick: closeRules }, COPY.close),
            rules.selfManaged ? null : h(Button, {
              onClick: () => toggleFromRules(rules, false),
              disabled: rulesBusy,
              testid: `restore-rules-${rules.channel}`,
            }, '恢复该通道'),
            rules.selfManaged ? null : h(Button, {
              tone: 'danger',
              onClick: () => toggleFromRules(rules, true),
              disabled: rulesBusy,
              testid: `revoke-rules-${rules.channel}`,
            }, `撤销 ${rules.label}`),
          ),
        },
        h('div', { className: 'zw-alert zw-alert-warn' },
          '撤权是 Core 运行时层的强制拒绝并持久化到 auth.json；它只阻止该通道的未来请求，不会回滚或删除已经提交的事实。'),
        h(Section, { title: '主体与授权范围' },
          h('dl', { className: 'zw-kv' },
            h('dt', null, '通道'), h('dd', { className: 'zw-mono' }, rules.channel),
            h('dt', null, '主体'), h('dd', { className: 'zw-mono' }, rules.principal),
            h('dt', null, '主体类型'), h('dd', null, rules.kind),
            h('dt', null, '当前撤销状态'), h('dd', null,
              rulesRevoked == null ? '未知（桥接没有授权状态查询接口）' : rulesRevoked ? '已撤销：后续请求会被 Core 拒绝' : '未撤销：后续请求仍会被接受'),
            h('dt', null, 'Core 协议 / 实例'), h('dd', { className: 'zw-mono' }, `${bridge?.core?.protocol ?? '未知'} · ${bridge?.core?.instance_id ?? '未知'}`),
            h('dt', null, '数据版本'), h('dd', null, `r${snapshot.workspace.revision}`),
          ),
        ),
        h(Section, { title: '读写范围（Core 运行时签发）' },
          h('div', { className: 'zw-scope-badges' }, rules.scopes.map(scope =>
            h(Badge, { key: scope, tone: 'neutral' }, `${SCOPE_LABELS[scope] ?? scope}（${scope}）`))),
          h('p', { className: 'zw-fineprint' }, '范围来自 Core 运行时为该通道签发的能力集；浏览器不能扩大或修改。'),
        ),
        h(Section, { title: '未来效应与 Agent 运行' },
          h('p', null, rulesRevoked == null
            ? '无法确认当前暂停状态：Host 桥接只提供撤销操作，没有读取授权状态的接口；执行一次撤销/恢复后这里会显示 Core 返回的完整撤销清单。'
            : rulesRevoked
              ? '已撤销：该通道后续鉴权会被 Core 拒绝，因此该通道的未来读取、写入、提议与执行（包括它发起的 Agent 运行提交）都会暂停。'
              : '未撤销：该通道的未来请求仍会被 Core 接受；是否允许某个动作仍由能力集与提案/批准规则决定。'),
          rules.selfManaged ? h('div', { className: 'zw-alert zw-alert-error' },
            '当前浏览器会话本身使用这个通道；自助撤销会切断所有 UI 命令，浏览器无法再恢复它。需要撤销时请改用本机 CLI 管理。') : null,
          rules.channel === 'harness_agent' || rules.channel === 'mcp_stdio'
            ? h('p', { className: 'zw-fineprint' },
              '已排队或正在运行的 Core AI 运行由内部主体 ai_planner（内部通道 ai）执行，不受该通道撤销影响；撤销不会自动取消这些运行，也不会阻止它们在 Core 内继续产生候选提案。')
            : null,
          h('p', { className: 'zw-fineprint' }, '由该通道发起、尚未提交的 Agent 动作不会再被接受；已签发的 Core 批准凭据由 Core 独立校验，不随通道撤权自动失效。'),
        ),
        h(Section, { title: '已发生事实与恢复' },
          h('p', null, '撤销不会删除、回滚或改写已提交的任务、提案、审计记录与执行事实；已经发出的提醒等外部效果不会因撤权自动撤回。'),
          h('p', { className: 'zw-fineprint' }, '桥接没有暴露待执行 effects 列表；撤权不回滚 Core 已记录的 outbox 外部效果，只停止该通道的新请求。'),
          h('p', { className: 'zw-fineprint' }, '恢复后原 token 才能再次使用；重新读取、提交与恢复都会重新鉴权。'),
        ),
        rulesMessage ? h('div', { className: 'zw-alert zw-alert-warn' }, rulesMessage) : null,
        rulesError ? h('div', { className: 'zw-alert zw-alert-error' }, rulesError) : null,
        ) : null,
      )
    }

    function AuditView({ send, busy }) {
      const [rows, setRows] = useState([])
      const [error, setError] = useState('')
      const load = async () => {
        setError('')
        try {
          const result = await send.query('audit.list', { limit: 100 })
          setRows(result.data.audit ?? [])
        } catch (failure) {
          setError(failure.message)
        }
      }
      useEffect(() => { void load() }, [])
      return h('div', { className: 'zw-page' },
        h('header', { className: 'zw-page-head' },
          h('div', null, h('div', { className: 'zw-kicker' }, '审计'), h('h1', null, '活动与恢复')),
          h(Button, { onClick: load, disabled: busy }, COPY.refresh),
        ),
        error ? h('div', { className: 'zw-alert zw-alert-error' }, error) : null,
        rows.length ? h('div', { className: 'zw-audit-list' },
          rows.map(row => h('div', { className: 'zw-audit-row', key: row.seq },
            h('div', { className: 'zw-audit-main' },
              h('strong', null, row.operation),
              h('span', null, `${row.actor_id} · ${row.channel}`),
            ),
            h('div', { className: 'zw-audit-meta' },
              h(Badge, { tone: row.status === 'committed' ? 'accent' : 'neutral' }, row.status),
              h('span', null, `r${row.revision}`),
              h('span', null, dateTimeFor(row.created_at)),
            ),
          )),
        ) : h(Empty, null, '还没有审计记录。'),
        h('p', { className: 'zw-fineprint' }, '审计按主体、通道、操作与版本记录；不复制私密正文。'),
      )
    }

    function CommandPalette({ onClose, actions }) {
      const [query, setQuery] = useState('')
      const [activeIndex, setActiveIndex] = useState(0)
      const filtered = actions.filter(action => `${action.label} ${action.hint ?? ''}`.toLowerCase().includes(query.trim().toLowerCase()))
      const clampIndex = index => filtered.length ? Math.max(0, Math.min(filtered.length - 1, index)) : 0
      return h(Modal, { title: '命令面板', subtitle: 'Ctrl/⌘ + K；↑/↓ 选择，Enter 执行，Esc 关闭。', onClose,
        footer: h(Button, { onClick: onClose }, COPY.close) },
        h('input', { autoFocus: true, value: query, placeholder: '搜索页面或动作…', 'data-testid': 'command-input',
          role: 'combobox', 'aria-autocomplete': 'list', 'aria-expanded': true,
          onChange: event => { setQuery(event.target.value); setActiveIndex(0) },
          onKeyDown: event => {
            if (event.key === 'ArrowDown' && filtered.length) {
              event.preventDefault()
              setActiveIndex(clampIndex(activeIndex + 1))
            } else if (event.key === 'ArrowUp' && filtered.length) {
              event.preventDefault()
              setActiveIndex(clampIndex(activeIndex - 1))
            } else if (event.key === 'Enter' && filtered[activeIndex]) {
              event.preventDefault()
              filtered[activeIndex].run()
            } else if (event.key === 'Home' && filtered.length) {
              event.preventDefault(); setActiveIndex(0)
            } else if (event.key === 'End' && filtered.length) {
              event.preventDefault(); setActiveIndex(filtered.length - 1)
            }
          } }),
        h('div', { className: 'zw-command-list' },
          filtered.length ? filtered.map((action, index) => h('button', {
            key: action.id,
            type: 'button',
            className: `zw-command-item ${index === activeIndex ? 'zw-command-item-active' : ''}`,
            'data-testid': `command-${action.id}`,
            'data-active': index === activeIndex ? 'true' : 'false',
            'aria-current': index === activeIndex ? 'true' : 'false',
            onClick: () => action.run(),
            onMouseEnter: () => setActiveIndex(index),
            ref: node => { if (index === activeIndex && node) node.scrollIntoView({ block: 'nearest' }) },
          },
          h('span', { className: 'zw-command-glyph' }, action.glyph ?? '→'),
          h('span', null, h('strong', null, action.label), action.hint ? h('small', null, action.hint) : null),
          )) : h(Empty, null, '没有匹配的命令。'),
        ),
      )
    }

    function ZodoWorkbench() {
      const rootRef = useRef(null)
      const width = useContainerWidth(rootRef)
      const [snapshot, setSnapshot] = useState(null)
      const [bridge, setBridge] = useState(null)
      const [view, setView] = useState('today')
      const [selectedTaskId, setSelectedTaskId] = useState(null)
      const [modal, setModal] = useState(null)
      const [busy, setBusy] = useState(false)
      const [error, setError] = useState('')
      const [notice, setNotice] = useState('')
      const [loading, setLoading] = useState(true)
      const [clockTick, setClockTick] = useState(0)
      const [runs, setRuns] = useState([])
      const [runsLoading, setRunsLoading] = useState(false)
      const [taskFilter, setTaskFilter] = useState({ text: '', status: '', tag: '', overdue: false })
      const [selectedTaskIds, setSelectedTaskIds] = useState([])
      const [scheduleLength, setScheduleLength] = useState(7)
      const [scheduleStart, setScheduleStart] = useState(() => agendaShift(todayIso(), 0))
      const settledRuns = useRef(new Set())
      stateSnapshotRef.current = snapshot

      useEffect(() => {
        const timer = setInterval(() => setClockTick(value => value + 1), 30_000)
        return () => clearInterval(timer)
      }, [])

      useEffect(() => {
        const onKey = event => {
          if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') {
            event.preventDefault()
            setModal(current => current?.type === 'command' ? null : { type: 'command' })
          }
        }
        window.addEventListener('keydown', onKey)
        return () => window.removeEventListener('keydown', onKey)
      }, [])

      const commandActions = [
        { id: 'today', glyph: '◐', label: '打开今天', hint: 'Today', run: () => { setModal(null); setView('today') } },
        { id: 'tasks', glyph: '☰', label: '打开任务', hint: 'Tasks', run: () => { setModal(null); setView('tasks') } },
        { id: 'schedule', glyph: '▦', label: '打开日程', hint: 'Schedule', run: () => { setModal(null); setView('schedule') } },
        { id: 'proposals', glyph: '◇', label: '打开提案', hint: 'Proposals', run: () => { setModal(null); setView('proposals') } },
        { id: 'review', glyph: '✓', label: '打开回顾', hint: 'Review', run: () => { setModal(null); setView('review') } },
        { id: 'capture', glyph: '＋', label: '收集一句话', hint: '原文进入 Inbox', run: () => setModal({ type: 'capture' }) },
        { id: 'task-new', glyph: '□', label: '新建任务', hint: '直接创建正式任务', run: () => setModal({ type: 'task', task: null }) },
        { id: 'goal-new', glyph: '◇', label: '新建目标', hint: 'Goal', run: () => setModal({ type: 'goal' }) },
        { id: 'plan', glyph: '→', label: '生成计划预览', hint: '5 分钟后起规划 7 天；只补充、不移动已有分配；需批准', run: () => { setModal(null); void openPlan() } },
      ]

      useEffect(() => {
        if (!notice) return
        const timer = setTimeout(() => setNotice(''), 5000)
        return () => clearTimeout(timer)
      }, [notice])

      const load = useCallback(async () => {
        setError('')
        try {
          const session = await ensureSession()
          const health = await fetch(`${BRIDGE}/health`, { credentials: 'same-origin' })
            .then(response => response.json())
            .catch(() => null)
          const result = await query('workspace.snapshot', {})
          setBridge(health ?? { core: session?.core })
          setSnapshot(result.data)
          setLoading(false)
        } catch (failure) {
          setError(failure.message)
          setLoading(false)
        }
      }, [])

      useEffect(() => { void load() }, [load])

      const refresh = useCallback(async () => {
        try {
          const result = await query('workspace.snapshot', {})
          setSnapshot(result.data)
        } catch (failure) {
          setError(failure.message)
        }
      }, [])

      const loadRuns = useCallback(async (silent = false) => {
        if (!silent) setRunsLoading(true)
        try {
          const next = await listRuns()
          setRuns(next)
          let newlySettled = false
          for (const run of next) {
            if (run.state === 'succeeded' && !settledRuns.current.has(run.id)) {
              settledRuns.current.add(run.id)
              newlySettled = true
            }
            if (run.state === 'failed' && !settledRuns.current.has(run.id)) {
              settledRuns.current.add(run.id)
              newlySettled = true
            }
          }
          if (newlySettled) {
            setNotice('AI 运行已结束；请检查提案或错误。')
            await refresh()
          }
        } catch (failure) {
          if (!silent) setError(failure.message)
        } finally {
          if (!silent) setRunsLoading(false)
        }
      }, [refresh])

      useEffect(() => { void loadRuns(true) }, [loadRuns])

      const runsActive = runs.some(run => run.state === 'queued' || run.state === 'running')
      useEffect(() => {
        if (!runsActive) return
        const timer = setInterval(() => { void loadRuns(true) }, 2000)
        return () => clearInterval(timer)
      }, [runsActive, loadRuns])

      const baseRevision = snapshot?.workspace?.revision
      const send = useCallback(async (operation, payload) => {
        setBusy(true)
        setError('')
        try {
          const result = await command(operation, payload, snapshot?.workspace?.revision)
          setNotice(`${['proposal.create', 'plan.propose'].includes(operation) ? '提案已记录 · 尚未应用' : '已保存'} · r${result.revision} · ${result.operation_id.slice(0, 12)}`)
          if (operation !== 'proposal.approve') await refresh()
          return result
        } catch (failure) {
          if (failure.code === 'REVISION_CONFLICT') {
            setError('数据已改变，当前页面需要刷新；不会覆盖新状态。')
            await refresh()
          } else {
            const strictPreview = operation === 'proposal.create' && payload?.operations?.some(item => item.operation === 'allocation.schedule')
            setError(strictPreview ? '严格安排预览失败；请查看弹窗中的处理建议，输入已保留。' : failure.message)
          }
          throw failure
        } finally {
          setBusy(false)
        }
      }, [refresh, snapshot?.workspace?.revision])

      const reader = useMemo(() => ({ query }), [])

      const today = todayIso()
      const activeTasks = (snapshot?.tasks ?? []).filter(task => !task.archived && task.status !== 'done' && task.status !== 'cancelled')
      const nextTask = activeTasks.slice().sort((a, b) => {
        const hardA = a.hard_deadline ?? '9999'
        const hardB = b.hard_deadline ?? '9999'
        if (hardA !== hardB) return hardA.localeCompare(hardB)
        const dueA = a.target_due ?? '9999'
        const dueB = b.target_due ?? '9999'
        if (dueA !== dueB) return dueA.localeCompare(dueB)
        return b.priority - a.priority
      })[0]
      const selectedTask = findTask(snapshot, selectedTaskId)
      const todayEntries = agendaEntries(snapshot ?? {}, today)
      const todayCheckedAt = Date.now()
      const todayAction = todayNextAction(todayEntries, activeTasks, nextTask, todayCheckedAt)
      const todayAllocations = todayEntries.filter(entry => entry.kind === 'allocation').map(entry => entry.row)
      const outstanding = outstandingWork(snapshot, todayCheckedAt)
      const unplacedMinutes = outstanding.minutes

      const openTask = id => {
        setSelectedTaskId(id)
        setView('tasks')
      }

      const applySavedView = view => {
        const filter = view.filter ?? {}
        setTaskFilter({
          text: filter.text ?? '',
          status: filter.status ?? '',
          tag: filter.tag ?? '',
          overdue: Boolean(filter.overdue),
        })
        setView('tasks')
      }

      const toggleTaskSelection = id => {
        setSelectedTaskIds(current => current.includes(id)
          ? current.filter(value => value !== id)
          : [...current, id])
      }

      const bulkUpdate = async patch => {
        if (!selectedTaskIds.length) return
        try {
          const preview = await send('task.batch_update', {
            ids: selectedTaskIds,
            ...patch,
            preview: true,
          })
          const affected = preview.data?.affected ?? selectedTaskIds.length
          if (!window.confirm(`影响 ${affected} 项任务。确认应用这次批量修改？`)) return
          await send('task.batch_update', {
            ids: selectedTaskIds,
            ...patch,
            preview: false,
          })
          setSelectedTaskIds([])
        } catch (failure) {
          setError(failure.message)
        }
      }

      const openPlan = async (mode = 'additive') => {
        const moveUnlocked = mode === 'replan'
        setError('')
        try {
          // Leave an explicit review window; never shift a candidate after approval.
          const start = new Date(Date.now() + 5 * 60 * 1000)
          const end = new Date(start.getTime() + 7 * 24 * 60 * 60 * 1000)
          const result = await send('plan.propose', {
            task_ids: [],
            horizon_start: start.toISOString(),
            horizon_end: end.toISOString(),
            move_unlocked: moveUnlocked,
            intent: moveUnlocked
              ? '重新规划：允许重排范围内未来未锁定、未开始的分配；固定预约、锁定、已开始和过去事实保持不变。从生成时刻 5 分钟后起规划，保留 7 天范围供检查；预览需批准后提交，超过候选开始时间须重新生成。'
              : '生成计划：仅补充安排，不移动或移除已有分配；容量不足会报告缺口。从生成时刻 5 分钟后起规划，保留 7 天范围供检查；预览需批准后提交，超过候选开始时间须重新生成。',
          })
          setModal({ type: 'plan', proposal: result.data.proposal, plan: result.data.plan, grant: null })
        } catch (failure) {
          setError(failure.message)
        }
      }

      const approvePlan = async () => {
        const proposal = modal?.proposal
        if (!proposal) return
        let grantId = modal.grant
        if (!grantId) {
          const approved = await send('proposal.approve', { proposal_id: proposal.id })
          grantId = approved.data.approval.id
          setModal({ ...modal, grant: grantId })
        }
        const committed = await send('changes.commit', { proposal_id: proposal.id, approval_grant: grantId })
        setModal(null)
        setNotice(`计划已应用 · ${committed.data.change_set_id} · r${committed.revision}`)
        await refresh()
      }

      const startAiDecompose = async goal => {
        setError('')
        try {
          const horizon = goal.target_due ? `${goal.target_due}T23:59:59Z` : new Date(Date.now() + 30 * 24 * 60 * 60 * 1000).toISOString()
          await startRun('goal.decompose', { goal_id: goal.id, horizon_end: horizon })
          setView('runs')
          setNotice('AI 拆解已入队；模型只生成提案，需要你检查后批准。')
          await loadRuns()
        } catch (failure) {
          setError(failure.message)
        }
      }

      const cancelAiRun = async id => {
        try {
          await cancelRun(id)
          await loadRuns()
        } catch (failure) {
          setError(failure.message)
        }
      }

      const openRunProposal = async () => {
        await refresh()
        setView('proposals')
      }

      const focusStart = async task => {
        try {
          await send('focus.start', { task_id: task?.id ?? null })
          setNotice('专注已开始')
        } catch (failure) {
          setError(failure.message)
        }
      }

      const focusFinish = async () => {
        try {
          const result = await send('focus.finish', {})
          setNotice(`专注已结束 · 记录 ${result.data.activity_id ?? ''}`)
        } catch (failure) {
          setError(failure.message)
        }
      }

      const focusPause = async () => {
        try {
          await send('focus.pause', {})
          setNotice('专注已暂停；恢复后继续累计')
        } catch (failure) {
          setError(failure.message)
        }
      }

      const focusResume = async () => {
        try {
          await send('focus.resume', {})
          setNotice('专注已恢复')
        } catch (failure) {
          setError(failure.message)
        }
      }

      const focusCancel = async () => {
        if (!window.confirm('取消当前专注？已记录的实际投入会保留，但不会再计入本次专注。')) return
        try {
          await send('focus.cancel', {})
          setNotice('专注已取消')
        } catch (failure) {
          setError(failure.message)
        }
      }

      const deleteDependency = async id => {
        try {
          await send('dependency.remove', { id })
        } catch (failure) {
          setError(failure.message)
        }
      }

      const renderTaskList = (tasks, empty = '这里还没有任务。') => {
        if (!tasks.length) return h(Empty, null, empty)
        return h('div', { className: 'zw-task-list' },
          tasks.map(task => h(TaskRow, { key: task.id, snapshot, task, onSelect: openTask })))
      }

      const renderFirstSchedule = () => {
        const step = firstScheduleStep(snapshot, todayCheckedAt, workspaceTimezone(snapshot))
        if (!step) return null
        const actions = {
          task: ['先建一个可以安排的任务，并填入预计分钟；不用先建项目或连接模型。', '新建任务', () => setModal({ type: 'task', task: null })],
          estimate: ['已有任务。给任务填入预计分钟，才能为它安排时段。', '补充任务估时', () => setModal({ type: 'task', task: step.task })],
          capacity: ['已有带估时的任务。先标出一段你愿意用来做任务的时间，也就是「容量」。日历空白不是容量。', '添加可用时间', () => setModal({ type: 'capacity' })],
          timezone: ['已录入任务和容量，但无法识别工作空间时区。请先检查时区设置。', '查看工作空间设置', () => setView('settings')],
          schedule: ['已有带估时的任务和未来容量。选择任务、容量和具体时段，先检查预览，再批准保存；是否可安排以 Core 检查为准。', '打开时段安排', () => setModal({ type: 'quick-schedule', day: step.day })],
        }
        const [description, label, open] = actions[step.kind]
        return h(Section, { title: '安排第一项任务', testid: 'first-schedule-guide' },
          h('p', null, description),
          h(Button, { onClick: open, disabled: busy, testid: 'first-schedule-action' }, label),
        )
      }

      const renderToday = () => {
        const emptyWorkspace = !snapshot.tasks.length && !snapshot.goals.length && !snapshot.inbox.length && !snapshot.habits.length
        return h('div', { className: 'zw-page' },
        h('header', { className: 'zw-page-head' },
          h('div', null,
            h('div', { className: 'zw-kicker' }, new Intl.DateTimeFormat('zh-CN', { weekday: 'long', month: 'long', day: 'numeric' }).format(new Date())),
            h('h1', null, '今天'),
          ),
          h('div', { className: 'zw-head-actions' },
            h(Button, { onClick: () => setModal({ type: 'capture' }), disabled: busy }, '收集'),
            h(Button, { onClick: () => openPlan(), disabled: busy, tone: 'primary', title: '从现在 5 分钟后起规划 7 天，仅补充安排、不移动已有分配；先预览再批准' }, COPY.plan),
            h(Button, { onClick: () => openPlan('replan'), disabled: busy, title: '从现在 5 分钟后起规划 7 天，允许重排未来未锁定、未开始分配；固定预约、锁定、已开始和过去事实保持不变，需预览批准' }, '重新规划'),
          ),
        ),
        error ? h('div', { className: 'zw-alert zw-alert-error' }, error) : null,
        renderFirstSchedule(),
        emptyWorkspace ? h('p', { className: 'zw-fineprint' }, '也可以先用「收集」记录一句原文，再决定怎样处理。') : null,
        h('div', { className: 'zw-today-grid' },
          h(Section, { title: '承诺与容量', className: 'zw-span2' },
            h('div', { className: 'zw-agenda' },
              todayEntries.map(entry => {
                const { row, kind } = entry
                return h('div', { className: `zw-agenda-row ${kind === 'allocation' ? 'zw-allocation' : ''}`, key: `${kind}:${row.id}` },
                  h('span', { className: 'zw-time' }, `${timeFor(new Date(entry.visibleStart).toISOString())}–${entry.endsAtDayBoundary ? '24:00' : timeFor(new Date(entry.visibleEnd).toISOString())}`),
                  h('span', { className: 'zw-agenda-title' }, kind === 'busy' ? '外部日历忙碌' : kind === 'allocation' ? taskTitle(snapshot, row.task_id) : row.title),
                  h(Badge, { tone: kind === 'commitment' || kind === 'busy' ? 'fixed' : 'soft' }, kind === 'busy' ? '只读忙碌快照' : kind === 'commitment' ? '固定' : kind === 'capacity' ? `容量 ${row.capacity_minutes} 分（全段）` : '任务分配'),
                  entry.continuesBefore || entry.continuesAfter ? h('small', null, [entry.continuesBefore ? '从前日延续' : '', entry.continuesAfter ? '延续至次日' : ''].filter(Boolean).join(' · ')) : null,
                )
              }),
              todayEntries.length === 0 ? h(Empty, null, '今天暂无已知安排，空白不代表可预约。') : null,
              h('p', { className: 'zw-fineprint' }, '按本机时区显示。外部忙碌为只读快照；来源缺失、未知或过期时不能据此判断可预约，请在日程中检查日历新鲜度。'),
            ),
          ),
          h(Section, { title: '下一步' },
            h('p', { className: 'zw-fineprint', 'data-testid': 'today-action-asof' }, `截至 ${dateTimeFor(new Date(todayCheckedAt).toISOString())} · 本次页面计算，非实时专注状态`),
            todayAction ? h('div', { className: 'zw-next', onClick: () => openTask(todayAction.task.id), role: 'button', tabIndex: 0,
              onKeyDown: event => { if (event.target === event.currentTarget && (event.key === 'Enter' || event.key === ' ')) { event.preventDefault(); openTask(todayAction.task.id) } },
              'data-testid': 'today-next-task' },
              h('span', { className: 'zw-next-kicker' }, todayAction.kind === 'scheduled-now' ? '计划时段覆盖当前时间' : todayAction.kind === 'scheduled-next' ? '今天下一项计划安排' : '按截止日期与优先级排序的建议'),
              h('strong', null, todayAction.task.title),
              todayAction.entry ? h('span', { className: 'zw-muted' }, `${dateTimeFor(todayAction.entry.row.start)}–${dateTimeFor(todayAction.entry.row.end)}`) : null,
              h('span', { className: 'zw-muted' }, `${todayAction.task.remaining_minutes ?? todayAction.task.estimate_minutes ?? '—'} 分钟剩余`),
              h('p', { className: 'zw-fineprint' }, todayAction.entry ? '这是已保存的计划，不代表正在专注，也不保证当前可执行。点击任务卡查看详情。' : '今天没有后续有效任务分配。建议不是可执行性判断；请核实依赖、最早开始时间与容量。点击任务卡查看详情。'),
              h(Button, { tone: 'primary', onClick: event => { event.stopPropagation(); focusStart(todayAction.task) }, disabled: busy, testid: 'today-focus' }, '开始专注'),
            ) : h(Empty, null, '没有待办。可以从收集箱开始。'),
          ),
          h(Section, { title: '风险与未排入' },
            unplacedMinutes > 0 ? h('div', { className: 'zw-alert zw-alert-warn' }, `已知任务中有 ${unplacedMinutes} 分钟尚无未结束分配；这不是规划可行性结论。`) : h('p', { className: 'zw-muted' }, '已知任务未发现尚待分配分钟；不代表容量、依赖或日历约束均满足。'),
            h('p', { className: 'zw-fineprint' }, `截至 ${dateTimeFor(new Date(todayCheckedAt).toISOString())}；只统计开放叶任务和未结束分配。${outstanding.unknown ? '部分数据缺失或无效，无法给出完整总量；请刷新并检查 Core 结果。' : '已结束分配不抵扣剩余工作。'}`),
            (snapshot.proposals ?? []).some(proposal => proposal.status === 'validated')
              ? h('div', { className: 'zw-proposal-banner', onClick: () => setView('proposals'), role: 'button', tabIndex: 0 },
                h('strong', null, '有提案待你确认'),
                h('span', null, '打开提案中心检查差异与影响。'),
              )
              : null,
            nextTask?.hard_deadline ? h('p', { className: 'zw-fineprint' }, `必须完成：${dateTimeFor(nextTask.hard_deadline)}`) : null,
            nextTask?.target_due && nextTask.target_due < today ? h('p', { className: 'zw-warn-text' }, '有逾期任务需要处理。') : null,
          ),
          h(Section, { title: '今天的工作', className: 'zw-span2' },
            renderTaskList(activeTasks.filter(task => task.target_due === today || todayAllocations.some(row => row.task_id === task.id)).slice(0, 8),
              '今天没有指定任务；可以查看所有任务或生成计划。'),
            h('div', { className: 'zw-inline-link', onClick: () => setView('tasks') }, '查看全部任务 →'),
          ),
        ),
      )
      }

      const renderInbox = () => h('div', { className: 'zw-page' },
        h('header', { className: 'zw-page-head' },
          h('div', null, h('div', { className: 'zw-kicker' }, '原文先保留'), h('h1', null, '收集箱')),
          h(Button, { tone: 'primary', onClick: () => setModal({ type: 'capture' }), disabled: busy }, '快速收集'),
        ),
        h('div', { className: 'zw-inbox-layout' },
          h('div', { className: 'zw-inbox-list' },
            (snapshot.inbox.length ? snapshot.inbox.slice().sort((a, b) => b.created_at.localeCompare(a.created_at)) : []).map(entry =>
              h('button', {
                key: entry.id,
                className: 'zw-inbox-row',
                onClick: () => setModal({ type: 'inbox-detail', entry }),
                'data-testid': `inbox-row-${entry.id}`,
              },
              h('span', { className: 'zw-inbox-text' }, entry.text),
              h('span', { className: 'zw-inbox-meta' },
                h(Badge, { tone: entry.status === 'needs_clarification' ? 'warn' : 'neutral' }, entry.status),
                h('span', null, dateTimeFor(entry.created_at)),
              ),
              )),
            !snapshot.inbox.length ? h(Empty, null, '收集箱是空的。先记录一句原文，不必现在决定它是什么。') : null,
          ),
          h('aside', { className: 'zw-inbox-aside' },
            h('h3', null, '处理原则'),
            h('p', null, '原文保留；澄清与转换是明确动作。转换后仍可以在任务详情中继续编辑。'),
            h('p', { className: 'zw-fineprint' }, '没有模型也能收集、澄清和手工转换。'),
          ),
        ),
      )

      const renderGoals = () => h('div', { className: 'zw-page' },
        h('header', { className: 'zw-page-head' },
          h('div', null, h('div', { className: 'zw-kicker' }, '结果与标准'), h('h1', null, '目标与项目')),
          h('div', { className: 'zw-head-actions' },
            h(Button, { onClick: () => setModal({ type: 'project' }), disabled: busy }, '新建项目'),
            h(Button, { tone: 'primary', onClick: () => setModal({ type: 'goal' }), disabled: busy }, '新建目标'),
          ),
        ),
        h('div', { className: 'zw-goals-grid' },
          snapshot.goals.map(goal => {
            const projects = snapshot.projects.filter(project => project.goal_id === goal.id)
            const projectTasks = snapshot.tasks.filter(task => projects.some(project => project.id === task.project_id))
            return h('article', { className: 'zw-card', key: goal.id },
              h('header', { className: 'zw-card-head' },
                h('div', null, h(Badge, { tone: goal.status === 'achieved' ? 'accent' : 'neutral' }, goal.status), h('h3', null, goal.title)),
                h('button', { className: 'zw-icon-btn', onClick: () => setModal({ type: 'goal', goal }), 'aria-label': '编辑目标' }, '…'),
              ),
              goal.success_criteria ? h('p', { className: 'zw-card-copy' }, goal.success_criteria) : h('p', { className: 'zw-muted' }, '还没有成功标准。'),
              h('div', { className: 'zw-card-meta' },
                h('span', null, goal.target_due ? `期望 ${goal.target_due}` : '未设期望日期'),
                h('span', null, `${projectTasks.filter(task => task.status === 'done').length}/${projectTasks.length} 任务完成`),
              ),
              projects.length ? h('div', { className: 'zw-project-list' },
                projects.map(project => h('div', { className: 'zw-project-row', key: project.id },
                  h('strong', null, project.title),
                  h('span', null, `${snapshot.tasks.filter(task => task.project_id === project.id).length} 项任务`),
                  h('button', { className: 'zw-link', onClick: () => setView('projects') }, '查看'),
                )),
              ) : h('p', { className: 'zw-muted' }, '还没有项目。可以只用一个任务开始。'),
              h('div', { className: 'zw-action-row' },
                h(Button, { onClick: () => setModal({ type: 'project', project: null, goalId: goal.id }), disabled: busy }, '添加项目'),
                h(Button, { onClick: () => setModal({ type: 'task', task: null, projectId: projects[0]?.id }), disabled: busy }, '添加任务'),
                h(Button, { onClick: () => startAiDecompose(goal), disabled: busy, testid: `ai-goal-${goal.id}` }, 'AI 拆解'),
                goal.status !== 'achieved' ? h(Button, { onClick: () => {
                  setModal({ type: 'goal-achieve', goal })
                }, disabled: busy }, '标记达成') : null,
              ),
            )
          }),
          !snapshot.goals.length ? h(Empty, null, '还没有目标。先写下想达成的结果，再决定是否需要项目。') : null,
        ),
      )

      const renderProjects = () => h('div', { className: 'zw-page' },
        h('header', { className: 'zw-page-head' },
          h('div', null, h('div', { className: 'zw-kicker' }, '范围与任务'), h('h1', null, '项目')),
          h(Button, { tone: 'primary', onClick: () => setModal({ type: 'project' }), disabled: busy }, '新建项目'),
        ),
        h('div', { className: 'zw-goals-grid' },
          snapshot.projects.map(project => {
            const tasks = snapshot.tasks.filter(task => task.project_id === project.id)
            return h('article', { className: 'zw-card', key: project.id },
              h('header', { className: 'zw-card-head' },
                h('div', null, h(Badge, { tone: 'neutral' }, project.status), h('h3', null, project.title)),
                h('button', { className: 'zw-icon-btn', onClick: () => setModal({ type: 'project', project }) }, '…'),
              ),
              project.scope ? h('p', { className: 'zw-card-copy' }, project.scope) : null,
              h('div', { className: 'zw-card-meta' }, `${tasks.length} 项任务 · ${tasks.filter(task => task.status === 'done').length} 完成`),
              (project.milestones ?? []).length ? h('div', { className: 'zw-milestones' },
                project.milestones.map(milestone => h('button', {
                  key: milestone.id,
                  className: `zw-milestone ${milestone.done ? 'zw-milestone-done' : ''}`,
                  onClick: () => setModal({ type: 'milestone', project, milestone }),
                }, h('span', null, milestone.done ? '✓' : '○'), h('span', null, milestone.title), milestone.due ? h('em', null, milestone.due) : null)),
              ) : null,
              renderTaskList(tasks.slice(0, 5), '还没有任务。'),
              h('div', { className: 'zw-action-row' },
                h(Button, { onClick: () => setModal({ type: 'task', task: null, projectId: project.id }), disabled: busy }, '添加任务'),
                h(Button, { onClick: () => setModal({ type: 'milestone', project }), disabled: busy }, '添加里程碑'),
              ),
            )
          }),
          !snapshot.projects.length ? h(Empty, null, '还没有项目。项目不是创建任务的必要条件。') : null,
        ),
      )

      const renderTasks = () => {
        const tags = [...new Set([
          ...(snapshot.tags ?? []).map(tag => tag.name),
          ...(snapshot.tasks ?? []).flatMap(task => task.tags ?? []),
        ])].filter(Boolean).sort()
        const filtered = snapshot.tasks.filter(task => {
          if (task.archived) return false
          if (taskFilter.text && !`${task.title} ${task.description ?? ''}`.toLowerCase().includes(taskFilter.text.toLowerCase())) return false
          if (taskFilter.status && task.status !== taskFilter.status) return false
          if (taskFilter.tag && !(task.tags ?? []).includes(taskFilter.tag)) return false
          if (taskFilter.overdue) {
            const overdue = (task.target_due && task.target_due < today) || (task.hard_deadline && task.hard_deadline < new Date().toISOString())
            if (!overdue) return false
          }
          return true
        }).slice().sort((a, b) => {
          if (a.status === 'done' && b.status !== 'done') return 1
          if (a.status !== 'done' && b.status === 'done') return -1
          return (a.target_due ?? '9999').localeCompare(b.target_due ?? '9999')
        })
        return h('div', { className: 'zw-page' },
          h('header', { className: 'zw-page-head' },
            h('div', null, h('div', { className: 'zw-kicker' }, '全部工作'), h('h1', null, '任务')),
            h('div', { className: 'zw-head-actions' },
              h(Button, { onClick: () => setModal({ type: 'recurrence' }), disabled: busy }, '新建循环'),
              h(Button, { onClick: () => setModal({ type: 'task', task: null }), disabled: busy, tone: 'primary' }, '新建任务'),
            ),
          ),
          h('div', { className: 'zw-toolbar zw-filterbar' },
            h('input', { placeholder: '过滤标题…', value: taskFilter.text,
              onChange: event => setTaskFilter({ ...taskFilter, text: event.target.value }) }),
            h('select', { value: taskFilter.status,
              onChange: event => setTaskFilter({ ...taskFilter, status: event.target.value }) },
              h('option', { value: '' }, '任意状态'), h('option', { value: 'open' }, '待处理'), h('option', { value: 'in_progress' }, '进行中'), h('option', { value: 'done' }, '已完成')),
            h('select', { value: taskFilter.tag,
              onChange: event => setTaskFilter({ ...taskFilter, tag: event.target.value }) },
              h('option', { value: '' }, '任意标签'), tags.map(tag => h('option', { key: tag, value: tag }, tag))),
            h('label', { className: 'zw-check-line' },
              h('input', { type: 'checkbox', checked: taskFilter.overdue,
                onChange: event => setTaskFilter({ ...taskFilter, overdue: event.target.checked }) }), '只看逾期'),
            h('Button', { onClick: () => setModal({ type: 'saved-view', initialFilter: taskFilter }), disabled: busy }, '保存视图'),
            h(Button, { onClick: refresh, disabled: busy }, COPY.refresh),
          ),
          h('div', { className: 'zw-toolbar' },
            h('span', { className: 'zw-muted' }, `筛选 ${filtered.length} / ${snapshot.tasks.length} 项；已选 ${selectedTaskIds.length} 项。`),
            h('div', { className: 'zw-action-row' },
              h(Button, {
                onClick: () => setSelectedTaskIds(selectedTaskIds.length === filtered.length ? [] : filtered.map(task => task.id)),
                disabled: busy || !filtered.length,
              }, selectedTaskIds.length === filtered.length && filtered.length ? '取消全选' : '全选当前'),
              h(Button, { onClick: () => setModal({ type: 'duplicate-suggestions' }), disabled: busy, testid: 'duplicate-suggest' }, '重复项'),
              h(Button, { onClick: () => bulkUpdate({ status: 'done' }), disabled: busy || !selectedTaskIds.length }, '批量完成'),
              h(Button, { onClick: () => bulkUpdate({ archived: true }), disabled: busy || !selectedTaskIds.length }, '批量归档'),
              h(Button, { onClick: () => { setTaskFilter({ text: '', status: '', tag: '', overdue: false }); setSelectedTaskIds([]) } }, '清除过滤'),
            ),
          ),
          h('div', { className: 'zw-task-layout' },
            h('div', { className: 'zw-task-list' },
              filtered.map(task => h(TaskRow, {
                key: task.id,
                snapshot,
                task,
                onSelect: openTask,
                selectable: true,
                selected: selectedTaskIds.includes(task.id),
                onToggleSelection: toggleTaskSelection,
              })),
              !filtered.length ? h(Empty, null, snapshot.tasks.length ? '没有符合当前过滤条件的任务。' : '还没有任务。创建一个，或者先收集一句原文。') : null,
            ),
            selectedTask ? h(TaskDetail, {
              task: selectedTask,
              snapshot,
              onClose: () => setSelectedTaskId(null),
              send,
              busy,
              onEdit: () => setModal({ type: 'task', task: selectedTask }),
              onTime: () => setModal({ type: 'time', task: selectedTask, tasks: snapshot.tasks }),
              onProgress: () => setModal({ type: 'progress', task: selectedTask }),
              onDependency: () => setModal({ type: 'dependency', task: selectedTask }),
              onReminder: () => setModal({ type: 'reminder', task: selectedTask }),
              onBlocked: () => setModal({ type: 'blocked', task: selectedTask }),
              onFocus: () => focusStart(selectedTask),
              setModal,
            }) : null,
          ),
        )
      }

      const renderSchedule = () => {
        const days = agendaDays(scheduleStart, scheduleLength)
        const unplacedList = outstanding.rows
        return h('div', { className: 'zw-page' },
          h('header', { className: 'zw-page-head' },
            h('div', null, h('div', { className: 'zw-kicker' }, '容量、预约与任务分配'), h('h1', null, '日程')),
            h('div', { className: 'zw-head-actions' },
              h(Button, { onClick: () => setModal({ type: 'time', task: null, tasks: activeTasks }), disabled: busy, testid: 'schedule-time' }, '任务时间'),
              h(Button, { onClick: () => setModal({ type: 'commitment' }), disabled: busy }, '增加预约'),
              h(Button, { onClick: () => setModal({ type: 'capacity' }), disabled: busy }, '增加容量'),
              h(Button, { onClick: () => setModal({ type: 'busy' }), disabled: busy }, '导入忙碌'),
              h(Button, { onClick: () => setModal({ type: 'capacity-candidates' }), disabled: busy, testid: 'capacity-suggest' }, '建议容量'),
              h(Button, { onClick: () => setModal({ type: 'meeting-propose' }), disabled: busy, testid: 'meeting-propose' }, '提议会议'),
              h(Button, { onClick: () => openPlan(), disabled: busy, tone: 'primary', title: '从现在 5 分钟后起规划 7 天，仅补充安排、不移动已有分配；先预览再批准' }, COPY.plan),
            h(Button, { onClick: () => openPlan('replan'), disabled: busy, title: '从现在 5 分钟后起规划 7 天，允许重排未来未锁定、未开始分配；固定预约、锁定、已开始和过去事实保持不变，需预览批准' }, '重新规划'),
            ),
          ),
          renderFirstSchedule(),
          h('div', { className: 'zw-head-actions', role: 'group', 'aria-label': '日程日期导航' },
            h('label', null, '视图 ', h('select', { value: scheduleLength, 'aria-label': '日程视图', 'data-testid': 'schedule-mode', onChange: event => {
              const length = event.target.value === '1' ? 1 : 7
              setScheduleLength(length)
              setScheduleStart(value => agendaShift(value, 0, length))
            } }, h('option', { value: 7 }, '7 天'), h('option', { value: 1 }, '单日（适合窄面板）'))),
            h('label', null, '起始日期 ', h('input', { type: 'date', value: scheduleStart, min: AGENDA_MIN,
              max: scheduleLength === 1 ? '9999-12-31' : AGENDA_MAX, 'aria-label': '日程起始日期', 'data-testid': 'schedule-date',
              onChange: event => { if (agendaDate(event.target.value)) setScheduleStart(agendaShift(event.target.value, 0, scheduleLength)) } })),
            h(Button, { onClick: () => setScheduleStart(value => agendaShift(value, -scheduleLength, scheduleLength)), disabled: scheduleStart <= AGENDA_MIN, testid: 'schedule-previous' }, scheduleLength === 1 ? '前一天' : '前 7 天'),
            h(Button, { onClick: () => setScheduleStart(agendaShift(today, 0, scheduleLength)), testid: 'schedule-today' }, '今天'),
            h(Button, { onClick: () => setScheduleStart(value => agendaShift(value, scheduleLength, scheduleLength)), disabled: scheduleStart >= (scheduleLength === 1 ? '9999-12-31' : AGENDA_MAX), testid: 'schedule-next' }, scheduleLength === 1 ? '后一天' : '后 7 天'),
            h('span', { 'aria-live': 'polite', 'data-testid': 'schedule-range' }, scheduleLength === 1 ? days[0] : `${days[0]} 至 ${days[6]}`),
          ),
          h('p', { className: 'zw-fineprint' }, '按本机时区显示；跨日安排分段展示，详情保留完整时间。外部日历仅显示只读忙碌区间；空白不代表可预约，来源缺失、未知或过期时请先核实日历。'),
          h('div', { className: `zw-week ${scheduleLength === 1 ? 'zw-single' : ''}` },
            days.map(day => {
              const entries = agendaEntries(snapshot, day)
              return h('div', { className: `zw-day ${day === today ? 'zw-day-today' : ''}`, key: day, 'data-testid': `schedule-day-${day}` },
                h('div', { className: 'zw-day-head' }, h('strong', null, dateForLocal(`${day}T12:00:00`)), h('span', null, new Intl.DateTimeFormat('zh-CN', { weekday: 'short' }).format(new Date(`${day}T12:00:00`)))),
                h(Button, { onClick: () => setModal({ type: 'quick-schedule', day }), disabled: busy, testid: `schedule-slot-${day}` }, '安排任务时段'),
                h(Button, { onClick: () => setModal({ type: 'time', day, task: null, tasks: activeTasks }), disabled: busy, testid: `schedule-task-date-${day}` }, '设置任务日期'),
                h(Button, { onClick: () => setModal({ type: 'commitment', day }), disabled: busy, testid: `schedule-add-appointment-${day}` }, '新增预约'),
                entries.map(entry => {
                  const { row, kind } = entry
                  if (kind === 'busy') return h('div', { className: 'zw-slot zw-slot-fixed', key: row.id, 'data-testid': 'time-busy' },
                    h('strong', null, `${timeFor(new Date(entry.visibleStart).toISOString())}–${entry.endsAtDayBoundary ? '24:00' : timeFor(new Date(entry.visibleEnd).toISOString())}`),
                    h('span', null, '外部日历忙碌'), h('em', null, '只读忙碌快照'),
                    entry.continuesBefore || entry.continuesAfter ? h('small', null, [entry.continuesBefore ? '从前日延续' : '', entry.continuesAfter ? '延续至次日' : ''].filter(Boolean).join(' · ')) : null,
                  )
                  const openDetail = () => setModal({ type: 'time-detail', kind, row })
                  return h('div', { className: `zw-slot zw-slot-${kind === 'commitment' ? 'fixed' : kind}`, key: `${kind}:${row.id}`, role: 'button', tabIndex: 0,
                    onClick: openDetail,
                    onKeyDown: event => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); openDetail() } },
                    'data-testid': `time-${kind}-${row.id}` },
                    h('strong', null, `${timeFor(new Date(entry.visibleStart).toISOString())}–${entry.endsAtDayBoundary ? '24:00' : timeFor(new Date(entry.visibleEnd).toISOString())}`),
                    h('span', null, kind === 'allocation' ? taskTitle(snapshot, row.task_id) : row.title),
                    h('em', null, kind === 'commitment' ? '固定预约' : kind === 'capacity' ? `${row.capacity_minutes} 分容量（全段）` : `${row.minutes} 分钟（全段）`),
                    entry.continuesBefore || entry.continuesAfter ? h('small', null, [entry.continuesBefore ? '从前日延续' : '', entry.continuesAfter ? '延续至次日' : ''].filter(Boolean).join(' · ')) : null,
                  )
                }),
                entries.length === 0 ? h('div', { className: 'zw-day-empty' }, '暂无已知安排') : null,
              )
            }),
          ),
          h('div', { className: 'zw-schedule-foot' },
            h(Section, { title: '未排入工作' },
              h('p', { className: 'zw-fineprint' }, `截至 ${dateTimeFor(new Date(todayCheckedAt).toISOString())}；开放叶任务按未结束分配计算。${outstanding.unknown ? '部分数据缺失或无效，以下仅为已知项，无法给出完整总量。' : '已结束分配不抵扣剩余工作。'} 最终以 Core 规划结果为准。`),
              unplacedList.length ? unplacedList.map(row => h('div', { className: 'zw-list-row', key: row.task.id },
                h('span', null, row.task.title), h(Badge, { tone: 'warn' }, `${row.minutes} 分钟`),
              )) : h(Empty, null, '已知项未发现尚待分配分钟；不代表规划可行。'),
            ),
            h(Section, { title: '日历新鲜度' },
              snapshot.calendar_sources?.length ? snapshot.calendar_sources.map(source => {
                const eventCount = (snapshot.calendar_events ?? []).filter(event => event.source_id === source.id && event.busy === true).length
                const freshness = calendarFreshness(source)
                return h('div', { className: 'zw-list-row', key: source.id, 'data-testid': `calendar-source-${source.id}` },
                  h('span', null, source.display_name),
                  h('span', { className: 'zw-muted' }, `${eventCount} 个忙碌事件 · 同步 ${freshness === '未知' ? '未知' : dateTimeFor(source.last_sync_at)}`),
                  h(Badge, { tone: freshness === '新鲜' ? 'accent' : 'warn' }, freshness),
                  h(Badge, { tone: 'neutral' }, source.provider),
                )
              }) : h(Empty, null, '没有外部日历来源，无法判断日历覆盖情况。忙碌导入是只读的，空白不代表可预约。'),
            ),
          ),
        )
      }

      const proposeEstimateAdjustments = async () => {
        const candidates = (snapshot.tasks ?? []).filter(task =>
          !task.archived &&
          task.status !== 'done' &&
          task.status !== 'cancelled' &&
          (task.actual_minutes ?? 0) > 0 &&
          (task.actual_minutes + (task.remaining_minutes ?? 0)) > Math.max(1, (task.estimate_minutes ?? 0)) * 1.2)
        if (!candidates.length) {
          setNotice('没有足够大的估算偏差可提议；不会为了数量制造建议。')
          return
        }
        const operations = candidates.map(task => {
          const suggested = Math.max(15, Math.min(10080, (task.actual_minutes ?? 0) + (task.remaining_minutes ?? 0)))
          return {
            operation: 'task.update',
            payload: {
              id: task.id,
              expected_version: task.version,
              task: {
                title: task.title,
                description: task.description ?? null,
                project_id: task.project_id ?? null,
                parent_id: task.parent_id ?? null,
                estimate_minutes: suggested,
                priority: task.priority ?? 2,
                target_due: task.target_due ?? null,
                hard_deadline: task.hard_deadline ?? null,
                not_before: task.not_before ?? null,
                preferred_window: task.preferred_window ?? null,
                tags: task.tags ?? [],
              },
            },
          }
        })
        try {
          await send('proposal.create', {
            intent: `根据实际投入为 ${operations.length} 个任务提交估算调整建议`,
            operations,
            assumptions: ['估算建议只基于已有实际投入与剩余分钟，不自动修改永久偏好'],
            questions: [],
          })
          setView('proposals')
          setNotice('估算调整已生成提案，需要人工批准。')
        } catch (failure) {
          setError(failure.message)
        }
      }

      const renderReview = () => {
        const todayReviews = snapshot.reviews.slice().reverse()
        const facts = todayReviews[0]?.facts
        const weekStart = startOfWeekIso(today)
        const weekActivities = (snapshot.activities ?? []).filter(row => row.occurred_at.slice(0, 10) >= weekStart)
        const weekCompleted = (snapshot.tasks ?? []).filter(task => task.completed_at && task.completed_at.slice(0, 10) >= weekStart).length
        const weekFocus = weekActivities.filter(row => row.kind === 'focus').reduce((sum, row) => sum + (row.minutes || 0), 0)
        const weekHabits = weekActivities.filter(row => row.kind === 'habit' || row.habit_id).length
        const weekActual = weekActivities.filter(row => row.kind === 'progress').reduce((sum, row) => sum + (row.minutes || 0), 0)
        return h('div', { className: 'zw-page' },
          h('header', { className: 'zw-page-head' },
            h('div', null, h('div', { className: 'zw-kicker' }, '执行事实，不是猜测'), h('h1', null, '回顾')),
            h('div', { className: 'zw-head-actions' },
              h(Button, { onClick: proposeEstimateAdjustments, disabled: busy, testid: 'estimate-propose' }, '估算调整建议'),
              h(Button, { tone: 'primary', onClick: async () => {
                try {
                  await send('review.build', { period_start: today, period_end: today })
                  setNotice('回顾已生成')
                } catch (failure) { setError(failure.message) }
              }, disabled: busy }, '生成今日回顾'),
            ),
          ),
          h(Section, { title: `本周统计（从 ${weekStart}）` },
            h('div', { className: 'zw-big-stats' },
              h('div', null, h('strong', null, weekCompleted), h('span', null, '完成任务')),
              h('div', null, h('strong', null, weekFocus), h('span', null, '专注分钟')),
              h('div', null, h('strong', null, weekActual), h('span', null, '记录投入')),
              h('div', null, h('strong', null, weekHabits), h('span', null, '习惯记录')),
            ),
            h('p', { className: 'zw-fineprint' }, '完成、专注、投入与习惯记录分别计数；专注分钟不会被自动算作任务完成。'),
          ),
          facts ? h('div', { className: 'zw-review-grid' },
            h(Section, { title: '实际执行' },
              h('div', { className: 'zw-big-stats' },
                h('div', null, h('strong', null, facts.focus_minutes), h('span', null, '专注分钟')),
                h('div', null, h('strong', null, facts.actual_minutes), h('span', null, '记录投入')),
                h('div', null, h('strong', null, facts.tasks_completed), h('span', null, '完成任务')),
                h('div', null, h('strong', null, facts.tasks_opened), h('span', null, '创建任务')),
                h('div', null, h('strong', null, `${facts.habit_adherence_percent ?? 0}%`), h('span', null, '习惯达成')),
                h('div', null, h('strong', null, facts.habit_target ?? 0), h('span', null, '习惯目标')),
              ),
            ),
            h(Section, { title: '需要留意' },
              facts.overdue_open ? h('p', { className: 'zw-warn-text' }, `${facts.overdue_open} 个任务逾期`) : h('p', { className: 'zw-muted' }, '没有逾期任务'),
              facts.blocked_open ? h('p', { className: 'zw-warn-text' }, `${facts.blocked_open} 个任务被阻塞`) : null,
              facts.unplaced_minutes ? h('p', { className: 'zw-warn-text' }, `${facts.unplaced_minutes} 分钟未排入`) : null,
              h('p', { className: 'zw-muted' }, `实际投入与专注分别记录；未完成的 ${facts.focus_minutes} 分钟不会被当成任务完成。`),
            ),
          ) : h(Empty, null, '还没有回顾。生成后会引用真实执行记录，不会从计时器推断完成。'),
          todayReviews.length ? h(Section, { title: '已生成的回顾' },
            todayReviews.slice(0, 8).map(review => h('article', { className: 'zw-review-card', key: review.id },
              h('header', null, h('strong', null, `${review.period_start} – ${review.period_end}`), h('span', { className: 'zw-muted' }, dateTimeFor(review.created_at))),
              h('p', null, `专注 ${review.facts.focus_minutes} 分钟 · 实际 ${review.facts.actual_minutes} 分钟 · 完成 ${review.facts.tasks_completed} 项`),
              review.recommendations?.length ? h('ul', null, review.recommendations.map((text, index) => h('li', { key: index }, text))) : null,
            )),
          ) : null,
        )
      }

      const renderFocus = () => {
        const current = (snapshot.focus_sessions ?? []).find(session => session.state === 'running' || session.state === 'paused')
        const task = current?.task_id ? findTask(snapshot, current.task_id) : null
        return h('div', { className: 'zw-page' },
          h('header', { className: 'zw-page-head' },
            h('div', null, h('div', { className: 'zw-kicker' }, '记录投入，不推断完成'), h('h1', null, '专注')),
          ),
          current ? h('article', { className: 'zw-focus-card' },
            h('div', { className: 'zw-focus-orb' }, '◉'),
            h('h2', null, task ? task.title : '未关联任务的专注'),
            h('p', null, `开始 ${dateTimeFor(current.started_at)} · 已记录 ${Math.floor(current.accumulated_seconds / 60)} 分钟`),
            h('div', { className: 'zw-action-row' },
              current.state === 'running'
                ? h(Button, { onClick: focusPause, disabled: busy, testid: 'focus-pause' }, '暂停')
                : h(Button, { tone: 'primary', onClick: focusResume, disabled: busy, testid: 'focus-resume' }, '继续'),
              h(Button, { tone: 'primary', onClick: focusFinish, disabled: busy, testid: 'focus-finish' }, '结束并记录进度'),
              h(Button, { onClick: focusCancel, disabled: busy, testid: 'focus-cancel' }, '取消专注'),
            ),
            h('p', { className: 'zw-fineprint' }, '结束专注只记录投入；任务完成需要另一次明确动作。'),
          ) : h('div', { className: 'zw-focus-card' },
            h('div', { className: 'zw-focus-orb' }, '◉'),
            h('h2', null, '准备开始一段专注'),
            h('p', null, '从任务详情开始，或者选择一个任务。'),
            h(Field, { label: '任务' }, h('select', { value: '', onChange: event => {
              if (event.target.value) focusStart(findTask(snapshot, event.target.value))
            } }, h('option', { value: '' }, '选择任务…'), activeTasks.map(candidate =>
              h('option', { key: candidate.id, value: candidate.id }, candidate.title)))),
            h(Button, { onClick: () => focusStart(null), disabled: busy }, '不关联任务开始'),
          ),
          h(Section, { title: '最近的专注记录' },
            (snapshot.focus_sessions ?? []).slice(-5).reverse().map(session => h('div', { className: 'zw-list-row', key: session.id },
              h('span', null, session.task_id ? taskTitle(snapshot, session.task_id) : '未关联任务'),
              h('span', { className: 'zw-muted' }, `${session.state} · ${Math.floor(session.accumulated_seconds / 60)} 分钟`),
            )),
          ),
        )
      }

      const renderSearch = () => h(SearchView, { send, reader, onSelect: id => { openTask(id); setView('tasks') } })

      const renderAudit = () => h(AuditView, { send: { query: reader.query }, busy })

      const renderBody = () => {
        if (!snapshot) return null
        switch (view) {
          case 'today': return renderToday()
          case 'inbox': return renderInbox()
          case 'goals': return renderGoals()
          case 'projects': return renderProjects()
          case 'habits': return h(HabitsView, { snapshot, send, busy, setModal })
          case 'views': return h(SavedViewsView, { snapshot, send, busy, setModal, onApply: applySavedView })
          case 'automation': return h(AutomationView, { snapshot, send, busy, setModal, report: setNotice })
          case 'tasks': return renderTasks()
          case 'schedule': return renderSchedule()
          case 'proposals': return h(ProposalsView, { snapshot, setModal, busy, runs })
          case 'runs': return h(RunsView, {
            runs,
            loading: runsLoading,
            busy,
            refresh: () => loadRuns(),
            cancel: cancelAiRun,
            onProposal: openRunProposal,
            snapshot,
            setModal,
          })
          case 'models': return h(ModelsView, { snapshot, busy, report: setNotice, onPrivacy: () => setView('privacy') })
          case 'data': return h(DataView, { snapshot, report: setNotice, refresh, send })
          case 'sync': return h(SyncView, { snapshot, report: setNotice, refresh })
          case 'review': return renderReview()
          case 'focus': return renderFocus()
          case 'search': return renderSearch()
          case 'settings': return h(SettingsView, { snapshot, send, busy, report: setNotice })
          case 'privacy': return h(PrivacyView, { snapshot, send, busy, report: setNotice, onModels: () => setView('models') })
          case 'agents': return h(AgentsView, { bridge, snapshot })
          case 'audit': return renderAudit()
          default: return renderToday()
        }
      }

      const closeModal = () => setModal(null)

      const modalContent = () => {
        if (!modal) return null
        switch (modal.type) {
          case 'command': return h(CommandPalette, { onClose: closeModal, actions: commandActions })
          case 'capture': return h(CaptureEditor, { onClose: closeModal, send, busy })
          case 'task': return h(TaskEditor, { task: modal.task ?? null, projectId: modal.projectId, snapshot: { ...snapshot, projects: snapshot.projects, tasks: snapshot.tasks }, onClose: closeModal, send, busy })
          case 'time': return h(TimeEditor, { day: modal.day, task: modal.task ?? null, tasks: modal.tasks ?? snapshot.tasks, snapshot, onClose: closeModal, send, busy })
          case 'goal': return h(GoalEditor, { goal: modal.goal ?? null, onClose: closeModal, send, busy })
          case 'project': return h(ProjectEditor, { project: modal.project ?? null, goals: snapshot.goals, goalId: modal.goalId, onClose: closeModal, send, busy })
          case 'inbox-detail': return h(InboxDetail, { key: modal.entry.id, entry: modal.entry, onClose: closeModal, send, busy, setModal })
          case 'inbox-convert': return h(InboxConvert, { entry: modal.entry, snapshot, onClose: closeModal, send, busy })
          case 'progress': return h(ProgressEditor, { task: modal.task, onClose: closeModal, send, busy })
          case 'dependency': return h(DependencyEditor, { task: modal.task, snapshot, onClose: closeModal, send, busy })
          case 'reminder': return h(ReminderEditor, { task: modal.task, onClose: closeModal, send, busy })
          case 'blocked': return h(BlockedEditor, { task: modal.task, onClose: closeModal, send, busy })
          case 'commitment': return h(CommitmentEditor, { day: modal.day, snapshot, onClose: closeModal, send, busy })
          case 'quick-schedule': return h(QuickSchedule, { day: modal.day, snapshot, send, busy, onClose: closeModal, setModal })
          case 'capacity': return h(CapacityEditor, { onClose: closeModal, send, busy })
          case 'capacity-candidates': return h(CapacitySuggestions, { onClose: closeModal, send, busy })
          case 'meeting-propose': return h(MeetingProposal, { onClose: closeModal, send, busy, setView, report: setNotice })
          case 'duplicate-suggestions': return h(DuplicateSuggestions, { onClose: closeModal, send, busy, refresh })
          case 'busy': return h(BusyImportEditor, { onClose: closeModal, send, busy })
          case 'recurrence': return h(RecurrenceEditor, { onClose: closeModal, send, busy })
          case 'plan': return h(PlanModal, {
            proposal: modal.proposal,
            plan: modal.plan,
            snapshotRevision: snapshot.workspace.revision,
            onClose: closeModal,
            onApprove: approvePlan,
            busy,
            error,
            grant: modal.grant,
          })
          case 'occurrence-move': return h(OccurrenceMove, { occurrence: modal.occurrence, task: modal.task, onClose: closeModal, send, busy })
          case 'recurrence-scope': return h(RecurrenceScopeDialog, { task: modal.task, snapshot, onClose: closeModal, send, busy, setModal })
          case 'goal-achieve': return h(GoalAchieve, { goal: modal.goal, onClose: closeModal, send, busy })
          case 'habit': return h(HabitEditor, { habit: modal.habit, onClose: closeModal, send, busy })
          case 'habit-log': return h(HabitLogEditor, { habit: modal.habit, onClose: closeModal, send, busy })
          case 'habit-detail': return h(HabitDetail, { habit: modal.habit, snapshot, onClose: closeModal, send, busy, setModal })
          case 'time-detail': return h(TimeDetail, { kind: modal.kind, row: modal.row, snapshot, onClose: closeModal, send, busy, setModal })
          case 'data-reference': return h(DataReferenceView, { proposal: modal.proposal ?? null, run: modal.run ?? null, snapshot, onClose: closeModal })
          case 'saved-view': return h(SavedViewEditor, { view: modal.view, initialFilter: modal.initialFilter, onClose: closeModal, send, busy })
          case 'milestone': return h(MilestoneEditor, { project: modal.project, milestone: modal.milestone, onClose: closeModal, send, busy })
          case 'automation': return h(AutomationEditor, { onClose: closeModal, send, busy })
          default: return null
        }
      }

      return h('div', { className: 'zw-root', ref: rootRef, 'data-zodo-workbench': 'a2' },
        h('style', null, css),
        h('header', { className: 'zw-topbar' },
          h('div', { className: 'zw-brand' }, h('span', { className: 'zw-brand-mark' }, 'Z'), h('span', null, 'Zodo')),
          h('div', { className: 'zw-connection' },
            h('span', { className: `zw-dot ${snapshot ? 'zw-dot-on' : ''}` }),
            h('span', null, snapshot ? COPY.connected : COPY.loading),
            snapshot ? h('span', { className: 'zw-revision' }, `r${snapshot.workspace.revision}`) : null,
          ),
          h('div', { className: 'zw-top-actions' },
            h(Button, { onClick: () => setModal({ type: 'capture' }), disabled: busy, testid: 'top-capture' }, '＋ 收集'),
            h(Button, { onClick: refresh, disabled: busy }, COPY.refresh),
          ),
        ),
        h('div', { className: `zw-shell ${width < 780 ? 'zw-narrow' : ''}` },
          h('nav', { className: 'zw-nav', 'aria-label': 'Zodo 业务入口' },
            h('div', { className: 'zw-nav-primary' },
              nav.map(item => h('button', {
                key: item.id,
                className: `zw-nav-item ${view === item.id ? 'zw-nav-active' : ''}`,
                onClick: () => { setView(item.id); setSelectedTaskId(null) },
                'data-testid': `nav-${item.id}`,
              }, h('span', { className: 'zw-nav-glyph' }, item.glyph), h('span', null, item.label))),
            ),
            h('div', { className: 'zw-nav-section' }, COPY.more),
            moreNav.map(item => h('button', {
              key: item.id,
              className: `zw-nav-item zw-nav-secondary ${view === item.id ? 'zw-nav-active' : ''}`,
              onClick: () => { setView(item.id); setSelectedTaskId(null) },
              'data-testid': `nav-${item.id}`,
            }, h('span', { className: 'zw-nav-glyph' }, item.glyph), h('span', null, item.label))),
            h('div', { className: 'zw-nav-foot' },
              h('span', null, snapshot?.workspace?.settings?.timezone ?? 'Asia/Shanghai'),
              h('span', null, snapshot?.workspace?.id ?? ''),
            ),
          ),
          h('main', { className: 'zw-main' },
            error ? h('div', { className: 'zw-alert zw-alert-error', role: 'alert', 'data-testid': 'zodo-error' }, error) : null,
            loading ? h('div', { className: 'zw-loading' }, h('div', { className: 'zw-spinner' }), COPY.loading) : renderBody(),
          ),
        ),
        notice ? h('div', { className: 'zw-toast', role: 'status', 'data-testid': 'zodo-toast' }, notice) : null,
        modalContent(),
      )
    }

    function InboxDetail({ entry, onClose, send, busy, setModal }) {
      const [question, setQuestion] = useState('')
      const [answer, setAnswer] = useState('')
      const [clarifying, setClarifying] = useState(false)
      const [clarifyRun, setClarifyRun] = useState(null)
      const [clarifyQuestions, setClarifyQuestions] = useState([])
      const [clarifyError, setClarifyError] = useState('')
      const alive = useRef(true)
      useEffect(() => () => { alive.current = false }, [])
      const clarifyKindLabel = kind => ({
        task: '任务', note: '笔记', goal: '目标', keep: '保持原样',
      })[kind] ?? kind
      const pollClarify = async runId => {
        for (let attempt = 0; attempt < 90; attempt += 1) {
          await new Promise(resolve => setTimeout(resolve, 1000))
          if (!alive.current) return
          try {
            const run = await getRun(runId)
            if (!alive.current) return
            setClarifyRun(run)
            if (run.state === 'succeeded') {
              setClarifyQuestions(run.output?.questions ?? run.checkpoint?.questions ?? [])
              setClarifying(false)
              return
            }
            if (['failed', 'cancelled', 'interrupted'].includes(run.state)) {
              setClarifyError(run.error?.message ?? `AI 运行 ${run.state}`)
              setClarifying(false)
              return
            }
          } catch (failure) {
            if (!alive.current) return
            setClarifyError(failure.message)
            setClarifying(false)
            return
          }
        }
        if (alive.current) {
          setClarifyError('AI 澄清运行超过 90 秒仍未结束；可在 AI 运行页继续查看。')
          setClarifying(false)
        }
      }
      const startClarify = async () => {
        setClarifying(true)
        setClarifyError('')
        setClarifyQuestions([])
        setClarifyRun(null)
        try {
          const result = await startRun('capture.clarify', { text: entry.text })
          const run = result?.run ?? result
          if (!alive.current) return
          setClarifyRun(run)
          await pollClarify(run.id)
        } catch (failure) {
          if (!alive.current) return
          setClarifyError(failure.message)
          setClarifying(false)
        }
      }
      const clarify = async () => {
        try {
          await send('capture.clarify', { id: entry.id, question, answer: answer || null })
          setQuestion('')
          setAnswer('')
        } catch {
          // The workbench error banner owns the failure.
        }
      }
      const dismiss = async () => {
        await send('capture.dismiss', { id: entry.id, reason: '用户明确忽略' })
        onClose()
      }
      const clarifyOutput = clarifyRun?.output ?? null
      return h(Modal, { title: '收集详情', subtitle: `来源 · ${entry.source?.kind ?? 'user'} · ${dateTimeFor(entry.created_at)}`, onClose, wide: true,
        footer: h(React.Fragment, null,
          h(Button, { onClick: onClose }, COPY.close),
          h(Button, { tone: 'danger', onClick: dismiss, disabled: busy || clarifying }, '忽略'),
          h(Button, { tone: 'primary', onClick: () => setModal({ type: 'inbox-convert', entry }), disabled: busy || clarifying }, '转换…')) },
        h('div', { className: 'zw-quote' }, entry.text),
        h('div', { className: 'zw-card-meta' }, h(Badge, { tone: entry.status === 'needs_clarification' ? 'warn' : 'neutral' }, entry.status)),
        h(Section, { title: 'AI 澄清（只提问，不写事实）' },
          h('p', { className: 'zw-fineprint' }, '运行意图 capture.clarify：把这段原文发给 Core 模型网关，返回需要澄清的问题。它不创建任务/目标/笔记，不产生提案，也不自动批准；转换仍需你明确操作。'),
          clarifyError ? h('div', { className: 'zw-alert zw-alert-error' }, clarifyError) : null,
          clarifyRun ? h('div', { className: 'zw-card-meta' },
            h('span', { className: 'zw-mono' }, clarifyRun.id),
            h(Badge, { tone: clarifyRun.state === 'succeeded' ? 'accent' : clarifyRun.state === 'failed' ? 'danger' : 'neutral' }, clarifyRun.state),
            clarifyOutput?.provider ? h('span', null, `${clarifyOutput.provider} · ${clarifyOutput.route}`) : null,
            clarifyOutput?.facts_written === false ? h(Badge, { tone: 'warn' }, 'facts_written: false') : null,
          ) : null,
          clarifying ? h('div', { className: 'zw-ai-progress' }, h('span', { className: 'zw-spinner' }), h('span', null, '模型正在生成澄清问题…'), null) : null,
          clarifyQuestions.length ? h('div', { className: 'zw-list' },
            clarifyQuestions.map((text, index) => h('div', { className: 'zw-list-row', key: index },
              h('span', null, `${index + 1}. ${text}`),
              h(Badge, { tone: 'neutral' }, '未回答'),
            )),
          ) : null,
          clarifyRun?.state === 'succeeded' && !clarifyQuestions.length
            ? h('Empty', null, '模型没有返回澄清问题；原文可以继续手工处理。') : null,
          clarifyOutput?.suggested_kind
            ? h('p', { className: 'zw-fineprint' }, `模型建议类型：${clarifyKindLabel(clarifyOutput.suggested_kind)}（只是建议，不自动转换）`) : null,
          clarifyOutput?.assumptions?.length
            ? h('p', { className: 'zw-fineprint' }, `模型假设 ${clarifyOutput.assumptions.length} 条：${clarifyOutput.assumptions.slice(0, 3).join(' · ')}`) : null,
          h('div', { className: 'zw-action-row' },
            h(Button, {
              onClick: startClarify,
              disabled: busy || clarifying || entry.status === 'dismissed',
              testid: 'inbox-ai-clarify',
            }, clarifying ? '正在澄清…' : 'AI 澄清'),
            clarifyRun ? h('span', { className: 'zw-fineprint' }, '运行只读；不会写入事实。') : null,
          ),
        ),
        entry.clarifications?.length ? h(Section, { title: '已有澄清' },
          entry.clarifications.map(item => h('div', { className: 'zw-list-row', key: item.id },
            h('span', null, item.question), h(Badge, { tone: item.answer ? 'accent' : 'warn' }, item.answer ?? '未回答'),
          )),
        ) : null,
        h(Section, { title: '人工补充澄清' },
          h(Field, { label: '问题', wide: true }, h('input', { value: question, onChange: event => setQuestion(event.target.value) })),
          h(Field, { label: '回答（可稍后）', wide: true }, h('input', { value: answer, onChange: event => setAnswer(event.target.value) })),
          h(Button, { onClick: clarify, disabled: busy || !question.trim() }, '保存澄清'),
        ),
        entry.converted_refs?.length ? h(Section, { title: '已转换到' },
          entry.converted_refs.map(ref => h('div', { className: 'zw-list-row', key: ref.id }, h('span', null, `${ref.kind} · ${ref.id}`))),
        ) : null,
      )
    }

    function GoalAchieve({ goal, onClose, send, busy }) {
      const [evidence, setEvidence] = useState('')
      const [error, setError] = useState('')
      const submit = async () => {
        if (!evidence.trim()) return setError('目标达成需要证据或明确确认')
        try {
          await send('goal.status', { id: goal.id, status: 'achieved', evidence: evidence.trim() })
          onClose()
        } catch (failure) {
          setError(failure.message)
        }
      }
      return h(Modal, { title: '标记目标达成', subtitle: '任务完成率不等于目标达成；这里需要明确证据。', onClose,
        footer: h(React.Fragment, null,
          h(Button, { onClick: onClose }, COPY.cancel),
          h(Button, { tone: 'primary', onClick: submit, disabled: busy }, '确认达成')) },
        error ? h('div', { className: 'zw-alert zw-alert-error' }, error) : null,
        h(Field, { label: '达成证据', wide: true }, h('textarea', { rows: 4, autoFocus: true, value: evidence,
          onChange: event => setEvidence(event.target.value) })),
      )
    }

    function SearchView({ reader, onSelect }) {
      const [text, setText] = useState('')
      const [results, setResults] = useState([])
      const [total, setTotal] = useState(0)
      const [nextOffset, setNextOffset] = useState(null)
      const [busy, setBusy] = useState(false)
      const [error, setError] = useState('')
      const requestSeq = useRef(0)
      const search = async (reset = true) => {
        if (!text.trim()) return
        const seq = ++requestSeq.current
        setError('')
        setBusy(true)
        try {
          const offset = reset ? 0 : (nextOffset ?? 0)
          const result = await reader.query('search.query', { text: text.trim(), limit: 50, offset })
          // A newer search supersedes this response; stale results must never overwrite it.
          if (seq !== requestSeq.current) return
          const incoming = result.data?.results ?? []
          setResults(reset ? incoming : previous => [...previous, ...incoming])
          setTotal(result.data?.total ?? incoming.length)
          setNextOffset(result.data?.has_more ? result.data?.next_offset ?? null : null)
        } catch (failure) {
          if (seq === requestSeq.current) setError(failure.message)
        } finally {
          if (seq === requestSeq.current) setBusy(false)
        }
      }
      const rows = results.map(result => h('button', {
        key: `${result.kind}-${result.id}`,
        className: 'zw-search-result',
        onClick: () => {
          if (result.kind === 'task') onSelect(result.id)
        },
      }, h(Badge, { tone: 'neutral' }, result.kind), h('span', null, result.title)))
      return h('div', { className: 'zw-page' },
        h('header', { className: 'zw-page-head' },
          h('div', null, h('div', { className: 'zw-kicker' }, '事实检索'), h('h1', null, '搜索')),
        ),
        error ? h('div', { className: 'zw-alert zw-alert-error' }, error) : null,
        results.length ? h('p', { className: 'zw-fineprint' }, `共 ${total} 条 · 已显示 ${results.length} 条${nextOffset == null ? '' : ` · 下一页从 ${nextOffset} 开始`}`) : null,
        h('div', { className: 'zw-searchbar' },
          h('input', {
            autoFocus: true,
            placeholder: '搜索任务、目标、项目…',
            value: text,
            onChange: event => setText(event.target.value),
            onKeyDown: event => {
              if (event.key === 'Enter' && !event.nativeEvent.isComposing) search(true)
            },
          }),
          h(Button, { tone: 'primary', onClick: () => search(true), disabled: busy }, '搜索'),
        ),
        results.length
          ? h('div', { className: 'zw-list' }, rows)
          : h(Empty, null, '输入关键词搜索已授权的任务、目标与项目。私密备注不会进入 Agent 结果。'),
        nextOffset != null ? h('div', { className: 'zw-action-row' },
          h(Button, { onClick: () => search(false), disabled: busy, testid: 'search-more' }, busy ? '加载中…' : '加载更多'),
        ) : null,
      )
    }

    const css = `
      .zw-root { --zw-canvas:#F7F8F6; --zw-surface:#FFFFFF; --zw-ink:#14202B; --zw-muted:#5F6B78; --zw-accent:#157E78; --zw-accent-soft:#E2EFEC; --zw-divider:#DFE6E5; --zw-warn:#845210; --zw-warn-bg:#FFF0D6; --zw-danger:#B42318; --zw-fixed:#E4EFF9; box-sizing:border-box; display:flex; flex-direction:column; height:100%; min-height:0; background:var(--zw-canvas); color:var(--zw-ink); font-family:var(--dsw-font-family,"Microsoft YaHei","Noto Sans SC",sans-serif); font-size:14px; line-height:1.5; }
      .zw-root * { box-sizing:border-box; }
      .zw-topbar { display:flex; align-items:center; gap:16px; padding:10px 16px; border-bottom:1px solid var(--zw-divider); background:var(--zw-surface); flex:none; }
      .zw-brand { display:flex; align-items:center; gap:8px; font-weight:650; font-size:16px; letter-spacing:-.01em; }
      .zw-brand-mark { width:24px; height:24px; border-radius:7px; background:var(--zw-accent); color:white; display:grid; place-items:center; font-size:14px; }
      .zw-connection { display:flex; align-items:center; gap:7px; color:var(--zw-muted); font-size:12px; }
      .zw-dot { width:7px; height:7px; border-radius:99px; background:#B9C2C1; }
      .zw-dot-on { background:var(--zw-accent); }
      .zw-revision { padding:1px 6px; border-radius:999px; border:1px solid var(--zw-divider); font-family:ui-monospace,Consolas,monospace; font-size:12px; }
      .zw-top-actions { margin-left:auto; display:flex; gap:8px; }
      .zw-shell { display:flex; flex:1; min-height:0; }
      .zw-nav { width:184px; flex:none; padding:14px 8px; border-right:1px solid var(--zw-divider); background:var(--zw-surface); overflow:auto; }
      .zw-nav-primary { display:grid; gap:2px; }
      .zw-nav-section { margin:18px 10px 6px; color:var(--zw-muted); font-size:12px; text-transform:uppercase; letter-spacing:.08em; }
      .zw-nav-item { width:100%; display:flex; align-items:center; gap:9px; min-height:36px; padding:0 10px; border:0; border-radius:9px; background:transparent; color:var(--zw-ink); font:inherit; text-align:left; cursor:pointer; }
      .zw-nav-item:hover { background:#F0F4F3; }
      .zw-nav-active { background:var(--zw-accent-soft); color:var(--zw-accent); font-weight:600; }
      .zw-nav-glyph { width:17px; color:var(--zw-muted); text-align:center; font-size:14px; }
      .zw-nav-active .zw-nav-glyph { color:#126B66; }
      .zw-nav-secondary { min-height:32px; color:var(--zw-muted); font-size:13px; }
      .zw-nav-foot { margin:24px 10px 0; padding-top:12px; border-top:1px solid var(--zw-divider); display:grid; gap:2px; color:var(--zw-muted); font-size:12px; font-family:ui-monospace,Consolas,monospace; overflow-wrap:anywhere; }
      .zw-main { flex:1; min-width:0; overflow:auto; position:relative; }
      .zw-page { max-width:1120px; margin:0 auto; padding:26px 28px 54px; }
      .zw-page-head { display:flex; align-items:flex-start; gap:16px; justify-content:space-between; margin-bottom:22px; }
      .zw-page-head h1 { margin:4px 0 0; font-size:28px; line-height:1.2; letter-spacing:-.02em; font-weight:650; }
      .zw-kicker { color:var(--zw-muted); font-size:12px; letter-spacing:.06em; text-transform:uppercase; }
      .zw-head-actions { display:flex; gap:8px; flex-wrap:wrap; justify-content:flex-end; }
      .zw-badge { display:inline-flex; align-items:center; min-height:20px; padding:0 7px; border-radius:999px; border:1px solid var(--zw-divider); color:var(--zw-muted); font-size:11px; white-space:nowrap; }
      .zw-badge-accent { background:var(--zw-accent-soft); border-color:transparent; color:var(--zw-accent); }
      .zw-badge-warn { background:var(--zw-warn-bg); border-color:transparent; color:var(--zw-warn); }
      .zw-badge-danger { background:#FDECEA; border-color:transparent; color:var(--zw-danger); }
      .zw-badge-soft { background:#EEF4F3; border-color:transparent; }
      .zw-badge-fixed { background:var(--zw-fixed); border-color:transparent; color:#2D536E; }
      .zw-btn { min-height:32px; padding:0 11px; border-radius:9px; border:1px solid var(--zw-divider); background:var(--zw-surface); color:var(--zw-ink); font:inherit; font-size:13px; cursor:pointer; white-space:nowrap; }
      .zw-btn:hover { border-color:#AEBDBB; }
      .zw-btn:disabled { opacity:.5; cursor:not-allowed; }
      .zw-btn-primary { background:var(--zw-accent); border-color:var(--zw-accent); color:white; }
      .zw-btn-primary:hover { background:#106B66; }
      .zw-btn-danger { color:var(--zw-danger); }
      .zw-icon-btn { border:0; background:transparent; color:var(--zw-muted); width:28px; height:28px; border-radius:7px; cursor:pointer; font-size:20px; line-height:1; }
      .zw-icon-btn:hover { background:#F0F4F3; color:var(--zw-ink); }
      .zw-section { background:var(--zw-surface); border:1px solid var(--zw-divider); border-radius:12px; padding:16px; margin-bottom:16px; }
      .zw-section-head { display:flex; align-items:center; justify-content:space-between; gap:12px; margin-bottom:12px; }
      .zw-section-head h3 { margin:0; font-size:13px; font-weight:650; letter-spacing:.01em; }
      .zw-today-grid { display:grid; grid-template-columns:minmax(0,1.4fr) minmax(0,1fr); gap:16px; }
      .zw-span2 { grid-column:1 / -1; }
      .zw-agenda { display:grid; gap:6px; }
      .zw-agenda-row { display:flex; align-items:center; gap:10px; min-height:36px; padding:4px 8px; border-radius:8px; background:#F8FAF9; }
      .zw-allocation { background:var(--zw-accent-soft); }
      .zw-time { width:100px; flex:none; color:var(--zw-muted); font-size:12px; font-variant-numeric:tabular-nums; }
      .zw-agenda-title { flex:1; min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
      .zw-next { display:grid; gap:7px; padding:12px; border-radius:9px; background:var(--zw-accent-soft); cursor:pointer; }
      .zw-next-kicker { color:var(--zw-accent); font-size:11px; }
      .zw-next strong { font-size:16px; font-weight:600; }
      .zw-inline-link { margin-top:12px; color:var(--zw-accent); font-size:12.5px; cursor:pointer; }
      .zw-proposal-banner { display:grid; gap:3px; margin-top:10px; padding:10px 11px; border-radius:9px; background:var(--zw-accent-soft); cursor:pointer; }
      .zw-proposal-banner span { color:var(--zw-muted); font-size:11.5px; }
      .zw-proposal-list { display:grid; gap:12px; margin-bottom:16px; }
      .zw-task-list { display:grid; gap:2px; }
      .zw-task-row { display:flex; align-items:center; gap:9px; width:100%; min-height:46px; padding:7px 8px; border:0; border-bottom:1px solid #EEF2F1; background:transparent; color:var(--zw-ink); font:inherit; text-align:left; cursor:pointer; border-radius:7px; }
      .zw-task-row:hover { background:#F6F9F8; }
      .zw-task-row-compact { border:0; }
      .zw-check { width:18px; height:18px; flex:none; border:1.5px solid #BCC8C6; border-radius:6px; display:grid; place-items:center; color:var(--zw-accent); font-size:11px; }
      .zw-command-list { display:grid; gap:4px; margin-top:12px; max-height:360px; overflow:auto; }
      .zw-command-item { display:flex; align-items:center; gap:10px; width:100%; border:0; border-radius:10px; background:transparent; padding:10px 12px; text-align:left; cursor:pointer; color:inherit; }
      .zw-command-item:hover, .zw-command-item:focus-visible, .zw-command-item-active { background:#EAF2F1; outline:none; }
      .zw-command-item strong { display:block; font-size:13px; }
      .zw-command-item small { display:block; color:#6B7B79; font-size:11px; }
      .zw-command-glyph { width:22px; color:var(--zw-accent); font-size:16px; text-align:center; }
      .zw-select { width:14px; height:14px; accent-color:var(--zw-accent); flex:none; }
      .zw-check-active { border-color:var(--zw-accent); background:var(--zw-accent-soft); }
      .zw-task-main { flex:1; min-width:0; display:grid; gap:1px; }
      .zw-task-title { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; font-size:13.5px; }
      .zw-task-done .zw-task-title { color:#84908C; text-decoration:line-through; }
      .zw-task-meta { color:var(--zw-muted); font-size:11px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
      .zw-empty { padding:18px 12px; color:var(--zw-muted); font-size:12.5px; background:#FAFBFA; border-radius:9px; border:1px dashed var(--zw-divider); }
      .zw-alert { border-radius:9px; padding:9px 11px; margin:0 0 14px; font-size:12.5px; }
      .zw-alert-error { background:#FDECEA; color:var(--zw-danger); }
      .zw-alert-warn { background:var(--zw-warn-bg); color:var(--zw-warn); }
      .zw-muted { color:var(--zw-muted); }
      .zw-fineprint { color:var(--zw-muted); font-size:11.5px; margin:8px 0 0; }
      .zw-warn-text { color:var(--zw-warn); }
      .zw-inbox-layout { display:grid; grid-template-columns:minmax(0,1.5fr) minmax(220px,.7fr); gap:20px; }
      .zw-inbox-list { display:grid; gap:6px; align-content:start; }
      .zw-inbox-row { display:flex; align-items:center; gap:12px; border:1px solid var(--zw-divider); background:var(--zw-surface); border-radius:10px; padding:11px 12px; color:var(--zw-ink); font:inherit; text-align:left; cursor:pointer; }
      .zw-inbox-row:hover { border-color:#B8C9C6; }
      .zw-inbox-text { flex:1; min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
      .zw-inbox-meta { display:flex; gap:8px; align-items:center; color:var(--zw-muted); font-size:11px; white-space:nowrap; }
      .zw-inbox-aside { padding:16px; border-radius:12px; background:var(--zw-surface); border:1px solid var(--zw-divider); height:fit-content; }
      .zw-inbox-aside h3 { margin:0 0 8px; font-size:13px; }
      .zw-inbox-aside p { margin:0 0 10px; color:var(--zw-muted); font-size:12.5px; }
      .zw-goals-grid { display:grid; grid-template-columns:repeat(auto-fit,minmax(280px,1fr)); gap:14px; }
      .zw-card { background:var(--zw-surface); border:1px solid var(--zw-divider); border-radius:12px; padding:16px; min-width:0; }
      .zw-card-head { display:flex; justify-content:space-between; gap:8px; align-items:flex-start; }
      .zw-card-head h3 { margin:4px 0 0; font-size:16px; font-weight:600; }
      .zw-card-copy { color:var(--zw-muted); font-size:12.5px; margin:10px 0; white-space:pre-wrap; }
      .zw-card-meta { display:flex; gap:12px; flex-wrap:wrap; color:var(--zw-muted); font-size:11.5px; margin:8px 0; }
      .zw-project-list { display:grid; gap:4px; margin:10px 0; }
      .zw-project-row { display:flex; align-items:center; gap:8px; min-height:30px; border-top:1px solid #EFF3F2; font-size:12.5px; }
      .zw-project-row strong { flex:1; font-weight:500; }
      .zw-project-row span { color:var(--zw-muted); font-size:11px; }
      .zw-link { border:0; background:transparent; color:var(--zw-accent); cursor:pointer; font:inherit; font-size:11.5px; padding:0; }
      .zw-action-row { display:flex; flex-wrap:wrap; gap:7px; margin-top:10px; }
      .zw-task-layout { display:flex; gap:18px; align-items:flex-start; }
      .zw-task-layout > .zw-task-list { flex:1; min-width:0; background:var(--zw-surface); border:1px solid var(--zw-divider); border-radius:12px; padding:6px 8px; }
      .zw-detail { width:min(420px,42%); flex:none; background:var(--zw-surface); border:1px solid var(--zw-divider); border-radius:12px; padding:16px; position:sticky; top:12px; max-height:calc(100% - 20px); overflow:auto; }
      .zw-detail-head { display:flex; justify-content:space-between; gap:10px; }
      .zw-detail-kicker { color:var(--zw-muted); font-size:11px; }
      .zw-detail h2 { margin:3px 0 8px; font-size:20px; line-height:1.3; }
      .zw-detail-meta { color:var(--zw-muted); font-size:12px; margin:0 0 10px; }
      .zw-stat-strip { display:grid; grid-template-columns:repeat(4,1fr); border-top:1px solid var(--zw-divider); border-bottom:1px solid var(--zw-divider); padding:9px 0; margin:10px 0; }
      .zw-stat-strip div { display:grid; gap:2px; text-align:center; border-right:1px solid #EFF3F2; }
      .zw-stat-strip div:last-child { border-right:0; }
      .zw-stat-strip span { color:var(--zw-muted); font-size:10.5px; }
      .zw-stat-strip strong { font-size:14px; font-weight:600; }
      .zw-list { display:grid; gap:2px; }
      .zw-list-row { display:flex; align-items:center; justify-content:space-between; gap:10px; min-height:34px; padding:4px 2px; border-bottom:1px solid #F0F3F2; font-size:12.5px; }
      .zw-list-row:last-child { border-bottom:0; }
      .zw-toolbar { display:flex; align-items:center; justify-content:space-between; margin-bottom:12px; }
      .zw-week { display:grid; grid-template-columns:repeat(7,minmax(96px,1fr)); gap:6px; overflow:auto; padding-bottom:4px; }
      .zw-week.zw-single { grid-template-columns:minmax(0,1fr); overflow:visible; min-width:0; }
      .zw-single .zw-day { min-width:0; overflow-wrap:anywhere; }
      .zw-single .zw-day-head { flex-wrap:wrap; gap:4px; }
      .zw-single .zw-slot, .zw-single button { max-width:100%; white-space:normal; overflow-wrap:anywhere; }
      .zw-day { min-height:250px; background:var(--zw-surface); border:1px solid var(--zw-divider); border-radius:10px; padding:8px; }
      .zw-day-today { border-color:#A8CFC9; box-shadow:inset 0 0 0 1px #A8CFC9; }
      .zw-day-head { display:flex; justify-content:space-between; align-items:baseline; padding-bottom:7px; margin-bottom:5px; border-bottom:1px solid #F0F3F2; }
      .zw-day-head span { color:var(--zw-muted); font-size:10.5px; }
      .zw-slot { display:grid; gap:1px; padding:6px; margin-bottom:4px; border-radius:7px; font-size:10.5px; }
      .zw-slot strong { font-weight:600; font-variant-numeric:tabular-nums; }
      .zw-slot em { font-style:normal; color:var(--zw-muted); }
      .zw-slot-capacity { background:var(--zw-accent-soft); }
      .zw-slot-fixed { background:var(--zw-fixed); }
      .zw-slot-allocation { background:#F2F7F5; border-left:3px solid var(--zw-accent); }
      .zw-day-empty { color:#9AA6A2; font-size:11px; padding:12px 4px; text-align:center; }
      .zw-schedule-foot { display:grid; grid-template-columns:minmax(0,1.2fr) minmax(0,.8fr); gap:16px; margin-top:16px; }
      .zw-schedule-foot .zw-section { margin-bottom:0; }
      .zw-review-grid { display:grid; grid-template-columns:minmax(0,1.2fr) minmax(0,.8fr); gap:16px; }
      .zw-big-stats { display:grid; grid-template-columns:repeat(2,1fr); gap:10px; }
      .zw-big-stats div { display:grid; gap:2px; padding:10px; background:#F8FAF9; border-radius:8px; }
      .zw-big-stats strong { font-size:24px; font-weight:600; letter-spacing:-.02em; }
      .zw-big-stats span { color:var(--zw-muted); font-size:11px; }
      .zw-review-card { padding:12px; border-bottom:1px solid #EFF2F1; }
      .zw-review-card header { display:flex; justify-content:space-between; gap:10px; }
      .zw-review-card p { margin:4px 0; color:var(--zw-muted); font-size:12px; }
      .zw-review-card ul { margin:4px 0 0 18px; color:var(--zw-ink); font-size:12px; }
      .zw-focus-card { max-width:520px; padding:28px; border-radius:14px; background:var(--zw-surface); border:1px solid var(--zw-divider); text-align:center; }
      .zw-focus-orb { font-size:36px; color:var(--zw-accent); margin-bottom:10px; }
      .zw-focus-card h2 { margin:0 0 8px; font-size:20px; }
      .zw-focus-card p { color:var(--zw-muted); font-size:12.5px; }
      .zw-focus-card .zw-action-row { justify-content:center; }
      .zw-searchbar { display:flex; gap:8px; margin-bottom:18px; }
      .zw-searchbar input { flex:1; min-height:40px; border:1px solid var(--zw-divider); border-radius:9px; padding:0 11px; font:inherit; background:var(--zw-surface); }
      .zw-search-result { display:flex; align-items:center; gap:9px; width:100%; text-align:left; border:0; border-bottom:1px solid #EEF1F0; background:transparent; padding:10px 4px; font:inherit; color:var(--zw-ink); cursor:pointer; }
      .zw-search-result:hover { background:#F7FAF9; }
      .zw-grid2 { display:grid; grid-template-columns:1fr 1fr; gap:10px 14px; }
      .zw-field { display:grid; gap:4px; font-size:12px; color:var(--zw-muted); }
      .zw-field-wide { grid-column:1 / -1; }
      .zw-field input, .zw-field select, .zw-field textarea { width:100%; min-height:34px; border:1px solid var(--zw-divider); border-radius:8px; padding:6px 9px; background:var(--zw-surface); color:var(--zw-ink); font:inherit; font-size:13px; }
      .zw-field textarea { resize:vertical; }
      .zw-capture-input { width:100%; border:1px solid var(--zw-divider); border-radius:10px; padding:11px; font:inherit; font-size:14px; resize:vertical; }
      .zw-check-line { display:flex; align-items:center; gap:8px; min-height:32px; margin-top:8px; font-size:12.5px; }
      .zw-weekdays { display:flex; gap:6px; margin-top:12px; flex-wrap:wrap; }
      .zw-pill { min-width:30px; min-height:28px; border:1px solid var(--zw-divider); background:var(--zw-surface); border-radius:7px; font:inherit; font-size:12px; cursor:pointer; color:var(--zw-muted); }
      .zw-pill-active { background:var(--zw-accent-soft); color:var(--zw-accent); border-color:#B6D8D2; }
      .zw-quote { border-left:3px solid #B6D8D2; background:#F6FAF9; padding:10px 12px; border-radius:0 8px 8px 0; color:var(--zw-ink); white-space:pre-wrap; margin-bottom:14px; }
      .zw-table { width:100%; border:1px solid var(--zw-divider); border-radius:9px; overflow:hidden; }
      .zw-table-head, .zw-table-row { display:grid; grid-template-columns:1.3fr 1.5fr .5fr; gap:8px; padding:7px 10px; font-size:12px; }
      .zw-table-head { background:#F5F8F7; color:var(--zw-muted); font-size:11px; }
      .zw-table-row { border-top:1px solid #EFF2F1; }
      .zw-plan-summary { display:grid; grid-template-columns:repeat(4,1fr); gap:8px; margin-bottom:14px; }
      .zw-plan-summary div { background:#F7FAF9; border-radius:8px; padding:9px; display:grid; gap:2px; }
      .zw-plan-summary strong { font-size:15px; }
      .zw-plan-summary span { color:var(--zw-muted); font-size:10.5px; }
      .zw-diagnostic { display:flex; gap:8px; align-items:center; padding:5px 0; font-size:12px; }
      .zw-overlay { position:fixed; inset:0; z-index:80; background:rgba(20,32,43,.28); display:flex; align-items:flex-start; justify-content:center; padding:6vh 16px; overflow:auto; }
      .zw-modal { width:min(560px,100%); background:var(--zw-surface); border-radius:14px; box-shadow:0 20px 60px rgba(20,32,43,.22); outline:none; }
      .zw-modal-wide { width:min(820px,100%); }
      .zw-modal-head { display:flex; justify-content:space-between; gap:12px; padding:18px 20px 10px; }
      .zw-modal-head h2 { margin:0; font-size:18px; font-weight:650; }
      .zw-modal-head p { margin:4px 0 0; color:var(--zw-muted); font-size:12px; }
      .zw-modal-body { padding:8px 20px 18px; max-height:68vh; overflow:auto; }
      .zw-modal-foot { display:flex; justify-content:flex-end; gap:8px; padding:12px 20px; border-top:1px solid var(--zw-divider); }
      .zw-toast { position:fixed; left:50%; bottom:24px; transform:translateX(-50%); z-index:90; background:#14202B; color:white; border-radius:9px; padding:8px 13px; font-size:12px; box-shadow:0 8px 24px rgba(20,32,43,.24); }
      .zw-loading { display:flex; align-items:center; justify-content:center; gap:10px; min-height:260px; color:var(--zw-muted); font-size:13px; }
      .zw-spinner { width:14px; height:14px; border:2px solid #DDE5E3; border-top-color:var(--zw-accent); border-radius:99px; animation:zw-spin .8s linear infinite; }
      @keyframes zw-spin { to { transform:rotate(360deg); } }
      .zw-kv { display:grid; grid-template-columns:max-content 1fr; gap:7px 14px; margin:0; font-size:12.5px; }
      .zw-kv dt { color:var(--zw-muted); }
      .zw-kv dd { margin:0; overflow-wrap:anywhere; }
      .zw-mono { font-family:ui-monospace,Consolas,monospace; font-size:11.5px; }
      .zw-flow { display:flex; align-items:center; gap:10px; flex-wrap:wrap; padding:12px; border-radius:10px; background:var(--zw-surface); border:1px solid var(--zw-divider); margin-bottom:16px; }
      .zw-flow div { display:grid; gap:2px; min-width:120px; padding:8px; background:#F7FAF9; border-radius:8px; }
      .zw-flow strong { font-size:12px; }
      .zw-flow span { color:var(--zw-muted); font-size:10.5px; }
      .zw-flow-arrow { color:var(--zw-muted); padding:0 4px; }
      .zw-panel { padding:12px; border-radius:9px; background:#F7FAF9; }
      .zw-panel h4 { margin:0 0 5px; font-size:12.5px; }
      .zw-panel p { margin:0; color:var(--zw-muted); font-size:12px; }
      .zw-workwindows { display:flex; flex-wrap:wrap; gap:5px; margin-top:12px; }
      .zw-workwindow { padding:2px 7px; border-radius:999px; background:#F4F8F7; color:var(--zw-muted); font-size:10.5px; }
      .zw-audit-list { display:grid; gap:2px; }
      .zw-audit-row { display:flex; justify-content:space-between; gap:12px; padding:9px 4px; border-bottom:1px solid #EFF2F1; font-size:12.5px; }
      .zw-audit-main { display:grid; gap:2px; }
      .zw-audit-main span { color:var(--zw-muted); font-size:11px; }
      .zw-audit-meta { display:flex; align-items:center; gap:9px; color:var(--zw-muted); font-size:11px; white-space:nowrap; }
      .zw-run-list { display:grid; gap:12px; }
      .zw-ai-progress { display:flex; align-items:center; gap:9px; margin-top:10px; padding:8px 10px; border-radius:8px; background:#F4F8F7; color:var(--zw-muted); font-size:12px; }
      .zw-ai-progress .zw-btn { margin-left:auto; }
      .zw-ai-settings .zw-grid2 { grid-template-columns:repeat(auto-fit,minmax(min(100%,240px),1fr)); }
      .zw-ai-settings .zw-field input, .zw-ai-settings .zw-field select { min-height:44px; font-size:14px; }
      .zw-ai-settings .zw-check-line { min-height:44px; align-items:flex-start; padding:10px 0; line-height:1.6; }
      .zw-ai-settings .zw-check-line input { flex:none; width:24px; height:24px; margin:0; accent-color:var(--zw-accent); }
      .zw-source-options { display:grid; grid-template-columns:repeat(auto-fit,minmax(min(100%,240px),1fr)); gap:12px; border:0; margin:0 0 16px; padding:0; min-width:0; }
      .zw-source-legend { font-size:14px; color:var(--zw-muted); margin-bottom:12px; }
      .zw-source-option { display:flex; align-items:flex-start; gap:12px; min-height:110px; padding:16px; border:1px solid var(--zw-divider); border-radius:12px; background:var(--zw-surface); cursor:pointer; }
      .zw-source-option input { flex:none; width:24px; height:24px; margin:0; accent-color:var(--zw-accent); }
      .zw-source-option > span { display:grid; gap:8px; min-width:0; }
      .zw-source-option strong { font-size:16px; }
      .zw-source-option span span { font-size:14px; line-height:1.6; color:var(--zw-muted); }
      .zw-source-selected { border-color:var(--zw-accent); background:var(--zw-accent-soft); }
      .zw-source-option:focus-within { outline:2px solid var(--zw-accent); outline-offset:2px; }
      .zw-privacy-options { display:grid; gap:12px; margin-bottom:16px; }
      .zw-model-selected-name { font-size:16px; font-weight:600; overflow-wrap:anywhere; }
      .zw-ai-advanced { border-top:1px solid var(--zw-divider); margin-top:24px; margin-bottom:16px; }
      .zw-ai-advanced:not([open]) > :not(summary) { display:none; }
      .zw-ai-advanced > summary { min-height:48px; padding:14px 0; font-size:14px; font-weight:600; cursor:pointer; }
      .zw-ai-settings .zw-kv { grid-template-columns:minmax(0,110px) minmax(0,1fr); font-size:13px; }
      .zw-ai-settings .zw-action-row { flex-wrap:wrap; }
      .zw-ai-settings .zw-fineprint { font-size:12px; line-height:1.7; }
      .zw-inline-field { display:flex; align-items:center; gap:4px; }
      .zw-inline-field input { flex:1; min-width:0; }
      .zw-time-preview { display:grid; gap:6px; padding:10px 12px; background:#F7FAF9; border-radius:9px; margin:12px 0; }
      .zw-time-preview div { display:flex; justify-content:space-between; align-items:baseline; gap:14px; font-size:12px; }
      .zw-time-preview span { color:var(--zw-muted); flex:none; }
      .zw-time-preview strong { font-weight:500; text-align:right; overflow-wrap:anywhere; }
      .zw-time-choices { display:grid; gap:4px; margin:8px 0; padding:8px 10px; background:#FFF7E6; border-radius:8px; font-size:12px; }
      .zw-radio-line { display:flex; align-items:flex-start; gap:7px; }
      .zw-radio-line input { margin-top:3px; accent-color:var(--zw-accent); }
      .zw-scope-badges { display:flex; flex-wrap:wrap; gap:5px; margin:8px 0 2px; }
      .zw-ref-entity { padding:10px 0; border-bottom:1px solid #EFF2F1; display:grid; gap:6px; }
      .zw-ref-entity:last-child { border-bottom:0; }
      .zw-ref-table { display:grid; gap:0; margin-top:6px; border:1px solid var(--zw-divider); border-radius:8px; overflow:hidden; }
      .zw-ref-row { display:grid; grid-template-columns:minmax(120px,.9fr) minmax(110px,.9fr) minmax(160px,2fr); gap:10px; padding:6px 9px; border-top:1px solid #EFF2F1; font-size:12px; align-items:start; }
      .zw-ref-row:first-child { border-top:0; }
      .zw-ref-row-head { background:#F5F8F7; color:var(--zw-muted); font-size:11px; }
      .zw-ref-value { overflow-wrap:anywhere; }
      .zw-warning-list { margin:6px 0 0 18px; padding:0; display:grid; gap:5px; font-size:12.5px; }
      .zw-channel-head, .zw-table-row.zw-channel-row { grid-template-columns:1.1fr 1.25fr .75fr 2fr; }
      .zw-table-row.zw-channel-row { width:100%; border:0; border-top:1px solid #EFF2F1; background:transparent; color:inherit; font:inherit; font-size:12px; text-align:left; cursor:pointer; padding:8px 10px; align-items:center; }
      .zw-table-row.zw-channel-row:hover { background:#F7FAF9; }
      .zw-filterbar { display:flex; gap:6px; flex-wrap:wrap; align-items:center; }
      .zw-filterbar input, .zw-filterbar select { min-height:32px; border:1px solid var(--zw-divider); border-radius:8px; padding:0 8px; font:inherit; font-size:12px; background:var(--zw-surface); color:var(--zw-ink); }
      .zw-filterbar input { min-width:150px; }
      .zw-progress { height:6px; border-radius:99px; background:#EDF3F1; overflow:hidden; margin:8px 0; }
      .zw-progress-fill { height:100%; background:var(--zw-accent); border-radius:99px; }
      .zw-filter-code { margin:8px 0; padding:8px; background:#F6F9F8; border-radius:7px; font:11px ui-monospace,Consolas,monospace; color:var(--zw-muted); overflow:auto; max-height:120px; }
      .zw-milestones { display:grid; gap:3px; margin:8px 0; }
      .zw-milestone { display:flex; align-items:center; gap:6px; min-height:28px; border:0; border-top:1px solid #EFF3F2; background:transparent; color:var(--zw-ink); font:inherit; font-size:12px; text-align:left; cursor:pointer; padding:0 2px; }
      .zw-milestone span:nth-child(2) { flex:1; }
      .zw-milestone em { color:var(--zw-muted); font-style:normal; font-size:10.5px; }
      .zw-milestone-done span:nth-child(2) { color:var(--zw-muted); text-decoration:line-through; }
      @media (max-width: 840px) {
        .zw-topbar { flex-wrap:wrap; gap:8px; }
        .zw-connection { order:3; }
        .zw-shell { flex-direction:column; }
        .zw-nav { width:100%; display:flex; flex-wrap:wrap; align-items:center; gap:2px; padding:6px 8px; border-right:0; border-bottom:1px solid var(--zw-divider); overflow:visible; }
        .zw-nav-primary { display:flex; gap:2px; }
        .zw-nav-section, .zw-nav-foot { display:none; }
        .zw-nav-item { width:auto; min-height:32px; padding:0 9px; white-space:nowrap; }
        /* Secondary pages must stay reachable at every viewport/container width. */
        .zw-nav-secondary { display:flex; flex:none; }
        .zw-page { padding:18px 14px 40px; }
        .zw-today-grid, .zw-review-grid, .zw-schedule-foot, .zw-inbox-layout, .zw-grid2 { grid-template-columns:1fr; }
        .zw-task-layout { flex-direction:column; }
        .zw-detail { width:100%; position:static; max-height:none; }
        .zw-week { grid-template-columns:repeat(7,minmax(120px,1fr)); }
        .zw-plan-summary { grid-template-columns:1fr 1fr; }
        .zw-modal-body { max-height:72vh; }
      }
      @media (prefers-reduced-motion: reduce) {
        .zw-spinner { animation:none; }
      }
      .zw-root { container-type:inline-size; }
      @container (max-width: 780px) {
        .zw-shell { flex-direction:column; }
        .zw-nav { width:100%; display:flex; flex-wrap:wrap; align-items:center; gap:2px; padding:6px 8px; border-right:0; border-bottom:1px solid var(--zw-divider); overflow:visible; }
        .zw-nav-primary, .zw-nav-secondary { display:flex; flex-wrap:wrap; gap:2px; }
        .zw-nav-section, .zw-nav-foot { display:none; }
        .zw-nav-item { width:auto; min-height:30px; padding:0 8px; font-size:12px; }
        .zw-nav-glyph { display:none; }
        .zw-page { padding:16px 12px 40px; }
        .zw-page-head { flex-direction:column; gap:10px; }
        .zw-page-head h1 { font-size:24px; }
        .zw-head-actions { justify-content:flex-start; }
        .zw-today-grid, .zw-review-grid, .zw-schedule-foot, .zw-inbox-layout, .zw-grid2 { grid-template-columns:1fr; }
        .zw-task-layout { flex-direction:column; }
        .zw-detail { width:100%; position:static; max-height:none; }
        .zw-plan-summary { grid-template-columns:1fr 1fr; }
        .zw-week { grid-template-columns:repeat(7,minmax(140px,1fr)); }
        .zw-topbar { flex-wrap:wrap; }
        .zw-top-actions { margin-left:0; }
        .zw-stat-strip { grid-template-columns:1fr 1fr; }
        .zw-inbox-aside { order:2; }
      }
    `

    return {
      inject: ['slots'],
      apply(ctx) {
        ctx.effect(() => ctx.slots.inject('main', () => ctx.slots.register({
          name: 'main',
          key: PANEL_ID,
        }, ZodoWorkbench)))
        ctx.effect(() => ctx.slots.inject('sidebar.panellist', () => ctx.slots.register({
          name: 'sidebar.panellist',
          id: PANEL_ID,
          order: 9,
          label: 'Zodo',
        }, ({ size = 16 }) => h('svg', {
          width: size, height: size, viewBox: '0 0 24 24', fill: 'none',
          stroke: 'currentColor', strokeWidth: 1.8, strokeLinecap: 'round', strokeLinejoin: 'round',
          'aria-hidden': true,
        }, h('rect', { x: 3, y: 3, width: 18, height: 18, rx: 5 }),
        h('path', { d: 'M8 8h8l-8 8h8' })))))
      },
    }
  },
})
