<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { ArchiveRestore, Cloud, LogIn, LogOut, RefreshCw, Trash2, UploadCloud } from 'lucide-vue-next'
import { NAlert, NButton, NForm, NFormItem, NInput, NSelect, NSpin, NSwitch, NTag, useDialog, useMessage } from 'naive-ui'
import { command, isTauriRuntime } from '../api'
import { readCloudSyncState, syncPendingCloudEvents, type CloudSyncState } from '../cloudSync'
import type { BackupEntry, BootstrapInfo } from '../types'
import { APP_VERSION } from '../version'
import {
  createCloudWorkspace,
  deleteCloudBackup,
  downloadCloudBackup,
  getCloudSession,
  listCloudBackups,
  listCloudWorkspaces,
  signInCloud,
  signOutCloud,
  signUpCloud,
  supabaseConfigured,
  supabaseUrl,
  uploadCloudBackup,
  type CloudBackup,
  type CloudWorkspace,
} from '../supabase'

const props = defineProps<{
  localBackups: BackupEntry[]
  bootstrap: BootstrapInfo | null
}>()

const emit = defineEmits<{ (event: 'changed'): void }>()
const message = useMessage()
const dialog = useDialog()
const session = ref<Awaited<ReturnType<typeof getCloudSession>>>(null)
const workspaces = ref<CloudWorkspace[]>([])
const selectedWorkspaceId = ref('')
const cloudBackups = ref<CloudBackup[]>([])
const email = ref('')
const password = ref('')
const workspaceName = ref('我的 ERP 账套')
const loading = ref(false)
const uploading = ref(false)
const staging = ref('')
const error = ref('')
const autoSyncEnabled = ref(false)
const syncSaving = ref(false)
const showLogin = ref(false)
const syncState = ref<CloudSyncState>({ enabled: false, pending_count: 0, failed_count: 0, last_synced_at: null, phase: 'disabled', message: '自动同步已停用' })

const signedIn = computed(() => Boolean(session.value?.user?.id))
const workspaceOptions = computed(() => workspaces.value.map((item) => ({ label: item.name, value: item.id })))
const usableLocalBackups = computed(() => props.localBackups.filter((item) => item.exists && item.result === 'success'))
const latestLocalBackup = computed(() => usableLocalBackups.value[0] ?? null)
const syncTagType = computed(() => syncState.value.phase === 'failed' ? 'error' : syncState.value.phase === 'synced' ? 'success' : syncState.value.phase === 'syncing' ? 'info' : 'warning')

function errorText(value: unknown) {
  return value instanceof Error ? value.message : String(value)
}

function formatBytes(value: number) {
  if (value < 1024) return `${value} B`
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`
  return `${(value / (1024 * 1024)).toFixed(1)} MB`
}

async function loadCloudData() {
  if (!signedIn.value) return
  loading.value = true
  error.value = ''
  try {
    workspaces.value = await listCloudWorkspaces()
    if (!selectedWorkspaceId.value || !workspaces.value.some((item) => item.id === selectedWorkspaceId.value)) {
      selectedWorkspaceId.value = workspaces.value[0]?.id ?? ''
    }
    if (selectedWorkspaceId.value) cloudBackups.value = await listCloudBackups(selectedWorkspaceId.value)
    else cloudBackups.value = []
  } catch (value) {
    error.value = errorText(value)
  } finally {
    loading.value = false
  }
}

async function refreshSession() {
  if (!supabaseConfigured) return
  loading.value = true
  error.value = ''
  try {
    session.value = await getCloudSession()
    await loadCloudData()
  } catch (value) {
    error.value = errorText(value)
  } finally {
    loading.value = false
  }
}

async function refreshSyncState(run = false) {
  try {
    syncState.value = run ? await syncPendingCloudEvents() : await readCloudSyncState()
    autoSyncEnabled.value = syncState.value.enabled
  } catch (value) {
    syncState.value = { ...syncState.value, phase: 'failed', message: errorText(value) }
  }
}

async function setAutoSyncEnabled(value: boolean) {
  const previous = autoSyncEnabled.value
  autoSyncEnabled.value = value
  syncSaving.value = true
  try {
    const saved = await command<{ value: string }>('save_settings', { key: 'cloud_sync_enabled', value: value ? '1' : '0' })
    autoSyncEnabled.value = saved.value === '1'
    await refreshSyncState(autoSyncEnabled.value)
    message.success(autoSyncEnabled.value ? '自动同步已开启，本地业务不受登录或网络影响' : '自动同步已停用')
  } catch (valueError) {
    autoSyncEnabled.value = previous
    message.error(errorText(valueError))
  } finally {
    syncSaving.value = false
  }
}

function handleSyncState(event: Event) {
  const detail = (event as CustomEvent<CloudSyncState>).detail
  if (detail) {
    syncState.value = detail
    autoSyncEnabled.value = detail.enabled
  }
}

async function signIn() {
  if (!email.value.trim() || !password.value) {
    message.warning('请输入 Supabase 邮箱和密码')
    return
  }
  loading.value = true
  error.value = ''
  try {
    session.value = await signInCloud(email.value, password.value)
    password.value = ''
    await loadCloudData()
    await refreshSyncState(true)
    message.success('Supabase 登录成功')
  } catch (value) {
    error.value = errorText(value)
    message.error(error.value)
  } finally {
    loading.value = false
  }
}

async function signUp() {
  if (!email.value.trim() || password.value.length < 8) {
    message.warning('注册密码至少需要 8 个字符')
    return
  }
  loading.value = true
  error.value = ''
  try {
    session.value = await signUpCloud(email.value, password.value)
    password.value = ''
    if (session.value) {
      await loadCloudData()
      message.success('Supabase 账号已创建')
    } else {
      message.success('注册请求已提交，请先完成邮箱确认')
    }
  } catch (value) {
    error.value = errorText(value)
    message.error(error.value)
  } finally {
    loading.value = false
  }
}

async function signOut() {
  loading.value = true
  try {
    await signOutCloud()
    session.value = null
    workspaces.value = []
    cloudBackups.value = []
    selectedWorkspaceId.value = ''
    await refreshSyncState()
    message.success('已退出 Supabase')
  } catch (value) {
    message.error(errorText(value))
  } finally {
    loading.value = false
  }
}

async function createWorkspace() {
  if (!workspaceName.value.trim()) {
    message.warning('工作区名称不能为空')
    return
  }
  loading.value = true
  try {
    const workspace = await createCloudWorkspace(workspaceName.value)
    workspaces.value = [...workspaces.value, workspace]
    selectedWorkspaceId.value = workspace.id
    cloudBackups.value = []
    await refreshSyncState(true)
    message.success('云端工作区已创建')
  } catch (value) {
    error.value = errorText(value)
    message.error(error.value)
  } finally {
    loading.value = false
  }
}

async function selectWorkspace(value: string) {
  selectedWorkspaceId.value = value
  if (!value) {
    cloudBackups.value = []
    return
  }
  try {
    cloudBackups.value = await listCloudBackups(value)
  } catch (valueError) {
    error.value = errorText(valueError)
  }
}

async function uploadLatest() {
  const local = latestLocalBackup.value
  if (!local || !selectedWorkspaceId.value) {
    message.warning('请先选择工作区，并确保本地存在校验通过的备份')
    return
  }
  if (!isTauriRuntime()) {
    message.info('浏览器演示模式不读取本地备份文件')
    return
  }
  uploading.value = true
  try {
    const file = await command<{ file_name: string; bytes: number[]; checksum: string; size_bytes: number; schema_version: number; created_at: string }>('read_backup_bytes', { backupPath: local.path })
    const uploaded = await uploadCloudBackup(selectedWorkspaceId.value, {
      fileName: file.file_name,
      bytes: file.bytes,
      checksum: file.checksum,
      sizeBytes: file.size_bytes,
      schemaVersion: file.schema_version,
      createdAt: file.created_at,
      appVersion: props.bootstrap?.app_version || APP_VERSION,
    })
    cloudBackups.value = [uploaded, ...cloudBackups.value.filter((item) => item.id !== uploaded.id)]
    message.success('本地备份已上传到 Supabase')
  } catch (value) {
    message.error(errorText(value))
  } finally {
    uploading.value = false
  }
}

function stageBackup(backup: CloudBackup) {
  dialog.warning({
    title: '下载云端备份',
    content: '文件会先校验 SHA-256，再写入本地 backup 目录；不会自动覆盖当前账套。确认继续吗？',
    positiveText: '下载并校验',
    negativeText: '取消',
    onPositiveClick: async () => {
      staging.value = backup.id
      try {
        const bytes = await downloadCloudBackup(backup)
        const result = await command<{ path: string }>('stage_cloud_backup', {
          fileName: backup.object_path.split('/').pop() || `erp-cloud-${backup.id}.db.enc`,
          bytes: Array.from(bytes),
          checksum: backup.checksum,
          schemaVersion: backup.schema_version,
          createdAt: backup.created_at,
        })
        emit('changed')
        message.success(`云端备份已保存到本地：${result.path}`)
      } catch (value) {
        message.error(errorText(value))
      } finally {
        staging.value = ''
      }
    },
  })
}

function removeBackup(backup: CloudBackup) {
  dialog.warning({
    title: '删除云端备份',
    content: '将同时删除 Storage 文件和云端元数据，不能从 Supabase 恢复。确认继续吗？',
    positiveText: '删除',
    negativeText: '取消',
    onPositiveClick: async () => {
      try {
        await deleteCloudBackup(backup)
        cloudBackups.value = cloudBackups.value.filter((item) => item.id !== backup.id)
        message.success('云端备份已删除')
      } catch (value) {
        message.error(errorText(value))
      }
    },
  })
}

watch(selectedWorkspaceId, (value) => {
  if (value && signedIn.value) void selectWorkspace(value)
})

onMounted(() => {
  window.addEventListener('erp:cloud-sync-status', handleSyncState)
  void refreshSession()
  void refreshSyncState(true)
})

onBeforeUnmount(() => window.removeEventListener('erp:cloud-sync-status', handleSyncState))
</script>

<template>
  <NSpin :show="loading">
    <div class="settings-section settings-section-stack cloud-sync-setting">
      <div>
        <h3>自动同步</h3>
        <p>业务数据先保存到本地 SQLite；未登录、断网或云端不可用时保留待同步队列，不影响继续录入。</p>
      </div>
      <div class="cloud-sync-control">
        <NTag :type="syncTagType" size="small">{{ syncState.message }}</NTag>
        <span v-if="syncState.pending_count" class="cloud-sync-count">待同步 {{ syncState.pending_count }} 条</span>
        <NSwitch :value="autoSyncEnabled" :loading="syncSaving" aria-label="自动同步" @update:value="setAutoSyncEnabled" />
      </div>
    </div>
    <NAlert v-if="!supabaseConfigured" type="info" :bordered="false" title="尚未配置 Supabase">
      <p>复制 `.env.example` 为 `.env.local`，填写项目 URL 和 publishable key 后重新启动。当前本地 SQLite、备份和离线功能不受影响。</p>
    </NAlert>
    <template v-else-if="!signedIn">
      <NAlert type="info" :bordered="false" title="当前无需登录">
        本地业务可正常使用；开启自动同步后，数据会保持“待登录同步”，已有合法会话时才上传。项目地址：{{ supabaseUrl() }}
      </NAlert>
      <div class="modal-actions cloud-login-entry">
        <NButton secondary @click="showLogin = !showLogin"><template #icon><LogIn :size="16" /></template>{{ showLogin ? '收起登录' : '需要时登录' }}</NButton>
      </div>
      <NForm v-if="showLogin" label-placement="top" class="cloud-form">
        <NFormItem label="Supabase 邮箱"><NInput v-model:value="email" autocomplete="email" placeholder="name@example.com" /></NFormItem>
        <NFormItem label="密码"><NInput v-model:value="password" type="password" show-password-on="click" autocomplete="current-password" @keyup.enter="signIn" /></NFormItem>
      </NForm>
      <div v-if="showLogin" class="modal-actions">
        <NButton type="primary" :loading="loading" @click="signIn"><template #icon><LogIn :size="16" /></template>登录</NButton>
        <NButton secondary :loading="loading" @click="signUp">注册账号</NButton>
      </div>
    </template>
    <template v-else>
      <div class="settings-section settings-section-stack">
        <div><h3>Supabase 云端备份</h3><p>{{ session?.user?.email }} · 本地账套仍是主数据</p></div>
        <NButton quaternary size="small" :loading="loading" @click="signOut"><template #icon><LogOut :size="15" /></template>退出</NButton>
      </div>
      <div v-if="!workspaces.length" class="cloud-workspace-create">
        <NFormItem label="新建云端工作区"><NInput v-model:value="workspaceName" placeholder="例如：改性塑料销售主账套" /></NFormItem>
        <NButton type="primary" secondary :loading="loading" @click="createWorkspace"><template #icon><Cloud :size="15" /></template>创建工作区</NButton>
      </div>
      <template v-else>
        <div class="settings-section settings-section-stack">
          <div><h3>工作区</h3><p>成员只能看到自己所属工作区的备份。</p></div>
          <NSelect :value="selectedWorkspaceId" :options="workspaceOptions" @update:value="selectWorkspace" />
        </div>
        <div class="settings-section settings-section-stack">
          <div><h3>上传最新本地备份</h3><p>{{ latestLocalBackup ? `${latestLocalBackup.path} · ${latestLocalBackup.checksum}` : '暂无校验通过的本地备份' }}</p></div>
          <NButton type="primary" secondary :disabled="!latestLocalBackup || !isTauriRuntime()" :loading="uploading" @click="uploadLatest"><template #icon><UploadCloud :size="16" /></template>上传</NButton>
        </div>
        <div class="backup-list-heading"><div><h3>云端备份</h3><p>下载时会先校验 SHA-256，再保存到本地 backup 目录。</p></div><NButton quaternary circle aria-label="刷新云端备份" :loading="loading" @click="loadCloudData"><template #icon><RefreshCw :size="16" /></template></NButton></div>
        <div v-if="!cloudBackups.length" class="backup-list-empty">当前工作区暂无云端备份</div>
        <div v-else class="backup-list">
          <div v-for="backup in cloudBackups" :key="backup.id" class="backup-row">
            <div class="backup-copy"><strong>{{ backup.object_path.split('/').pop() }}</strong><span>{{ new Date(backup.created_at).toLocaleString('zh-CN', { hour12: false }) }} · {{ formatBytes(backup.size_bytes) }} · V{{ backup.schema_version }}</span><small>{{ backup.checksum }}</small></div>
            <div class="cloud-row-actions"><NButton quaternary circle aria-label="下载云端备份" :loading="staging === backup.id" @click="stageBackup(backup)"><ArchiveRestore :size="16" /></NButton><NButton quaternary circle aria-label="删除云端备份" @click="removeBackup(backup)"><Trash2 :size="16" /></NButton></div>
          </div>
        </div>
      </template>
    </template>
    <NAlert v-if="error" type="error" :bordered="false" class="cloud-error">{{ error }}</NAlert>
  </NSpin>
</template>
