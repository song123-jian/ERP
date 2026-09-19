export type BackgroundTaskKind =
  | 'startup'
  | 'backup'
  | 'restore'
  | 'export'
  | 'import'
  | 'attachment'
  | 'protection'
  | 'integrity'
  | 'write'
  | 'read'

export interface BackgroundTask {
  id: string
  kind: BackgroundTaskKind
  label: string
  startedAt: number
}

export interface TaskHandle {
  id: string
  finish: () => void
}

const active = new Map<string, BackgroundTask>()
const listeners = new Set<(tasks: BackgroundTask[]) => void>()
let sequence = 0

function snapshot() {
  return [...active.values()].sort((a, b) => a.startedAt - b.startedAt || a.id.localeCompare(b.id))
}

function notify() {
  const current = snapshot()
  listeners.forEach((listener) => listener(current))
}

export function activeTasks() {
  return snapshot()
}

export function subscribeTasks(listener: (tasks: BackgroundTask[]) => void) {
  listeners.add(listener)
  listener(snapshot())
  return () => listeners.delete(listener)
}

export function startTask(label: string, kind: BackgroundTaskKind = 'read'): TaskHandle {
  const id = `task-${Date.now()}-${sequence++}`
  active.set(id, { id, kind, label, startedAt: Date.now() })
  notify()
  let finished = false
  return {
    id,
    finish: () => {
      if (finished) return
      finished = true
      active.delete(id)
      notify()
    },
  }
}

export async function runTrackedTask<T>(label: string, operation: () => Promise<T>, kind: BackgroundTaskKind = 'read') {
  const task = startTask(label, kind)
  try {
    return await operation()
  } finally {
    task.finish()
  }
}

export function waitForTasks(timeoutMs = 30_000): Promise<BackgroundTask[]> {
  if (!active.size) return Promise.resolve([])
  return new Promise((resolve) => {
    let settled = false
    let unsubscribe: (() => void) | null = null
    const finish = (remaining: BackgroundTask[]) => {
      if (settled) return
      settled = true
      if (unsubscribe) unsubscribe()
      clearTimeout(timeout)
      resolve(remaining)
    }
    const timeout = setTimeout(() => finish(snapshot()), timeoutMs)
    unsubscribe = subscribeTasks((tasks) => {
      if (!tasks.length) finish([])
    })
  })
}
