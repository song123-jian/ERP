import { command } from './api'
import {
  getCloudSession,
  listCloudWorkspaces,
  supabaseConfigured,
  uploadCloudSyncEvents,
  type LocalSyncEvent,
} from './supabase'

export type CloudSyncPhase =
  | 'disabled'
  | 'local_only'
  | 'waiting_login'
  | 'waiting_workspace'
  | 'ready'
  | 'syncing'
  | 'synced'
  | 'failed'

export interface LocalSyncStatus {
  enabled: boolean
  pending_count: number
  failed_count: number
  last_synced_at: string | null
}

export interface CloudSyncState extends LocalSyncStatus {
  phase: CloudSyncPhase
  message: string
}

let activeSync: Promise<CloudSyncState> | null = null

function publish(state: CloudSyncState) {
  window.dispatchEvent(new CustomEvent<CloudSyncState>('erp:cloud-sync-status', { detail: state }))
  return state
}

export async function readCloudSyncState(): Promise<CloudSyncState> {
  const status = await command<LocalSyncStatus>('get_sync_status')
  if (!status.enabled) return publish({ ...status, phase: 'disabled', message: '自动同步已停用' })
  if (!supabaseConfigured) return publish({ ...status, phase: 'local_only', message: '本地可用，等待配置 Supabase' })
  const session = await getCloudSession()
  if (!session?.user.id) return publish({ ...status, phase: 'waiting_login', message: '待登录同步' })
  const workspaces = await listCloudWorkspaces()
  if (!workspaces.length) return publish({ ...status, phase: 'waiting_workspace', message: '待创建云端工作区' })
  return publish({
    ...status,
    phase: status.pending_count ? 'ready' : 'synced',
    message: status.pending_count ? `待同步 ${status.pending_count} 条` : '已同步',
  })
}

async function runSync(): Promise<CloudSyncState> {
  const current = await readCloudSyncState()
  if (!current.enabled || ['local_only', 'waiting_login', 'waiting_workspace'].includes(current.phase)) return current
  if (!current.pending_count) return current

  publish({ ...current, phase: 'syncing', message: `正在同步 ${current.pending_count} 条` })
  const events = await command<LocalSyncEvent[]>('list_pending_sync_events', { limit: 100 })
  if (!events.length) return readCloudSyncState()
  const ids = events.map((event) => event.local_id)
  try {
    const workspaces = await listCloudWorkspaces()
    const workspaceId = workspaces[0]?.id
    if (!workspaceId) return publish({ ...current, phase: 'waiting_workspace', message: '待创建云端工作区' })
    await uploadCloudSyncEvents(workspaceId, events)
    await command('mark_sync_events_synced', { localIds: ids })
    return readCloudSyncState()
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error)
    await command('mark_sync_events_failed', { localIds: ids, error: message }).catch(() => undefined)
    const status = await command<LocalSyncStatus>('get_sync_status')
    return publish({ ...status, phase: 'failed', message: `同步失败，稍后自动重试：${message}` })
  }
}

export function syncPendingCloudEvents() {
  if (!activeSync) activeSync = runSync().finally(() => { activeSync = null })
  return activeSync
}
