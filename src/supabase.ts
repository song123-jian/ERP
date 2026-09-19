import { createClient, type Session, type SupabaseClient } from '@supabase/supabase-js'

const configuredUrl = String(import.meta.env.VITE_SUPABASE_URL ?? '').trim().replace(/\/+$/, '')
const configuredKey = String(import.meta.env.VITE_SUPABASE_PUBLISHABLE_KEY ?? '').trim()

export const supabaseConfigured = Boolean(configuredUrl && configuredKey)
export const supabase: SupabaseClient | null = supabaseConfigured
  ? createClient(configuredUrl, configuredKey, {
      auth: {
        persistSession: true,
        autoRefreshToken: true,
        detectSessionInUrl: false,
      },
    })
  : null

export interface CloudWorkspace {
  id: string
  name: string
  owner_id: string
  created_at: string
  updated_at: string
}

export interface CloudBackup {
  id: string
  workspace_id: string
  uploaded_by: string
  device_id: string
  object_path: string
  checksum: string
  size_bytes: number
  schema_version: number
  app_version: string
  created_at: string
  metadata: Record<string, unknown>
}

export interface CloudBackupFile {
  file_name: string
  bytes: number[]
  checksum: string
  size_bytes: number
  schema_version: number
  created_at: string
}

export interface CloudBackupUpload {
  fileName: string
  bytes: number[]
  checksum: string
  sizeBytes: number
  schemaVersion: number
  createdAt: string
  appVersion: string
}

export interface LocalSyncEvent {
  local_id: number
  event_id: string
  entity_type: string
  entity_id: number
  operation: string
  payload: Record<string, unknown>
  created_at: string
  retry_count: number
  last_error: string
}

function requireClient() {
  if (!supabase) throw new Error('尚未配置 Supabase，请在 .env.local 填写项目 URL 和 publishable key')
  return supabase
}

function normalizeError(error: { message?: string } | null | undefined) {
  return error?.message?.trim() || 'Supabase 请求失败，请检查网络和项目配置'
}

export function supabaseUrl() {
  return configuredUrl
}

export async function getCloudSession(): Promise<Session | null> {
  if (!supabase) return null
  const { data, error } = await supabase.auth.getSession()
  if (error) throw new Error(normalizeError(error))
  return data.session
}

export async function signInCloud(email: string, password: string) {
  const client = requireClient()
  const { data, error } = await client.auth.signInWithPassword({ email: email.trim(), password })
  if (error) throw new Error(normalizeError(error))
  return data.session
}

export async function signUpCloud(email: string, password: string) {
  const client = requireClient()
  const { data, error } = await client.auth.signUp({ email: email.trim(), password })
  if (error) throw new Error(normalizeError(error))
  return data.session
}

export async function signOutCloud() {
  if (!supabase) return
  const { error } = await supabase.auth.signOut()
  if (error) throw new Error(normalizeError(error))
}

export async function listCloudWorkspaces(): Promise<CloudWorkspace[]> {
  const client = requireClient()
  const { data, error } = await client
    .from('erp_workspaces')
    .select('id,name,owner_id,created_at,updated_at')
    .order('created_at', { ascending: true })
  if (error) throw new Error(normalizeError(error))
  return (data ?? []) as CloudWorkspace[]
}

export async function createCloudWorkspace(name: string): Promise<CloudWorkspace> {
  const client = requireClient()
  const { data, error } = await client.rpc('create_erp_workspace', { p_name: name.trim() })
  if (error) throw new Error(normalizeError(error))
  const workspaceId = String(data ?? '')
  if (!workspaceId) throw new Error('云端工作区创建成功但未返回 ID')
  const workspaces = await listCloudWorkspaces()
  const workspace = workspaces.find((item) => item.id === workspaceId)
  if (!workspace) throw new Error('云端工作区已创建，但当前账号无法读取成员关系')
  return workspace
}

export async function listCloudBackups(workspaceId: string): Promise<CloudBackup[]> {
  const client = requireClient()
  const { data, error } = await client
    .from('erp_cloud_backups')
    .select('id,workspace_id,uploaded_by,device_id,object_path,checksum,size_bytes,schema_version,app_version,created_at,metadata')
    .eq('workspace_id', workspaceId)
    .order('created_at', { ascending: false })
  if (error) throw new Error(normalizeError(error))
  return (data ?? []) as CloudBackup[]
}

function deviceId() {
  const key = 'plastic-sales-erp-cloud-device-id'
  const existing = localStorage.getItem(key)?.trim()
  if (existing) return existing
  const generated = typeof crypto.randomUUID === 'function'
    ? crypto.randomUUID()
    : `device-${Date.now()}-${Math.random().toString(16).slice(2)}`
  localStorage.setItem(key, generated)
  return generated
}

export async function uploadCloudSyncEvents(workspaceId: string, events: LocalSyncEvent[]) {
  if (!events.length) return
  const client = requireClient()
  const session = await getCloudSession()
  if (!session?.user.id) throw new Error('待登录同步：本地数据已保存，登录后会自动重试')
  const currentDeviceId = deviceId()
  const rows = events.map((event) => ({
    workspace_id: workspaceId,
    uploaded_by: session.user.id,
    device_id: currentDeviceId,
    local_event_id: event.event_id,
    entity_type: event.entity_type,
    entity_id: event.entity_id,
    operation: event.operation,
    payload: event.payload,
    occurred_at: event.created_at,
  }))
  const { error } = await client
    .from('erp_sync_events')
    .upsert(rows, {
      onConflict: 'workspace_id,device_id,local_event_id',
      ignoreDuplicates: true,
    })
  if (error) throw new Error(normalizeError(error))
}

function safeFileName(value: string) {
  const candidate = value.replace(/[\\/]/g, '_').replace(/[^\w.\-\u4e00-\u9fff]/g, '_')
  return candidate || `erp-cloud-${Date.now()}.db.enc`
}

export async function uploadCloudBackup(workspaceId: string, payload: CloudBackupUpload): Promise<CloudBackup> {
  const client = requireClient()
  const session = await getCloudSession()
  if (!session?.user.id) throw new Error('请先登录 Supabase 账号')
  const fileName = safeFileName(payload.fileName)
  const path = `${workspaceId}/${deviceId()}/${fileName}`
  const body = new Blob([new Uint8Array(payload.bytes)], { type: 'application/octet-stream' })
  const { error: uploadError } = await client.storage.from('erp-backups').upload(path, body, {
    cacheControl: '3600',
    contentType: 'application/octet-stream',
    upsert: false,
  })
  if (uploadError) throw new Error(normalizeError(uploadError))
  const { data, error } = await client
    .from('erp_cloud_backups')
    .insert({
      workspace_id: workspaceId,
      uploaded_by: session.user.id,
      device_id: deviceId(),
      object_path: path,
      checksum: payload.checksum,
      size_bytes: payload.sizeBytes,
      schema_version: payload.schemaVersion,
      app_version: payload.appVersion,
      created_at: payload.createdAt,
      metadata: { source: 'plastic-sales-erp', encrypted: fileName.endsWith('.enc') },
    })
    .select('id,workspace_id,uploaded_by,device_id,object_path,checksum,size_bytes,schema_version,app_version,created_at,metadata')
    .single()
  if (error) {
    await client.storage.from('erp-backups').remove([path])
    throw new Error(normalizeError(error))
  }
  return data as CloudBackup
}

export async function downloadCloudBackup(backup: CloudBackup): Promise<Uint8Array> {
  const client = requireClient()
  const { data, error } = await client.storage.from('erp-backups').download(backup.object_path)
  if (error) throw new Error(normalizeError(error))
  const bytes = new Uint8Array(await data.arrayBuffer())
  const digest = await crypto.subtle.digest('SHA-256', bytes)
  const actual = Array.from(new Uint8Array(digest)).map((value) => value.toString(16).padStart(2, '0')).join('')
  if (actual !== backup.checksum) throw new Error('云端备份 SHA-256 校验不匹配，已拒绝保存')
  return bytes
}

export async function deleteCloudBackup(backup: CloudBackup) {
  const client = requireClient()
  const { error: storageError } = await client.storage.from('erp-backups').remove([backup.object_path])
  if (storageError) throw new Error(normalizeError(storageError))
  const { error } = await client.from('erp_cloud_backups').delete().eq('id', backup.id)
  if (error) throw new Error(normalizeError(error))
}
