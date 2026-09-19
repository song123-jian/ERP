<script setup lang="ts">
import { computed, h, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { ArchiveRestore, Bell, CheckCircle2, ChevronLeft, ChevronRight, CircleHelp, Clipboard, DatabaseBackup, HardDrive, LayoutDashboard, LoaderCircle, LockKeyhole, RefreshCw, Save, Settings2, ShieldCheck } from 'lucide-vue-next'
import { NAlert, NBadge, NButton, NConfigProvider, NDialogProvider, NDrawer, NDrawerContent, NForm, NFormItem, NInput, NInputNumber, NLayout, NLayoutContent, NLayoutHeader, NLayoutSider, NMenu, NMessageProvider, NModal, NRadioButton, NRadioGroup, NResult, NSpin, NSwitch, NTabs, NTabPane, NTag, NTooltip, lightTheme, useDialog, useMessage, type MenuOption } from 'naive-ui'
import DashboardView from './components/DashboardView.vue'
import EntityPage from './components/EntityPage.vue'
import CloudBackupPanel from './components/CloudBackupPanel.vue'
import OperationalAssessmentPanel from './components/OperationalAssessmentPanel.vue'
import { command, isTauriRuntime } from './api'
import { syncPendingCloudEvents } from './cloudSync'
import { dirtyDrafts, discardDirtyDrafts, saveDirtyDrafts } from './drafts'
import { subscribeTasks, waitForTasks, type BackgroundTask } from './tasks'
import { moduleMap, modules, navigationGroups } from './modules'
import { naiveTheme, tokens } from './theme'
import { shouldRunScheduledBackup } from './domain'
import type { BackupEntry, BootstrapInfo, CloseBehavior, EntityName, NativeBackgroundTask, ProtectionSetupResult } from './types'
import { APP_VERSION } from './version'

const activeKey = ref('dashboard')
const collapsed = ref(false)
const booting = ref(true)
const bootError = ref('')
const locked = ref(false)
const unlockSecret = ref('')
const unlockError = ref('')
const unlocking = ref(false)
const bootstrap = ref<BootstrapInfo | null>(null)
const showRecoveryNotice = ref(false)
const recoveryPickerOpen = ref(false)
const draftGuardOpen = ref(false)
const draftGuardCount = ref(0)
const draftGuardBusy = ref(false)
const pendingNavigation = ref<string | null>(null)
const exitDialogOpen = ref(false)
const exitDraftCount = ref(0)
const exitBusy = ref(false)
const activeTaskRows = ref<BackgroundTask[]>([])
const closeBehavior = ref<CloseBehavior>('minimize_to_tray')
const closeBehaviorSaving = ref(false)
const showHelp = ref(false)
const showSettings = ref(false)
const settingsTab = ref('backup')
const shuttingDown = ref(false)
const protectionPassword = ref('')
const protectionConfirm = ref('')
const disableSecret = ref('')
const protectionBusy = ref(false)
const recoveryKey = ref('')
const backups = ref<BackupEntry[]>([])
const backupsLoading = ref(false)
const backupError = ref('')
const integrityChecking = ref(false)
const restoringPath = ref('')
const businessSettingsLoading = ref(false)
const businessSettingsSaving = ref<string | null>(null)
const reminderDays = ref<number | null>(3)
const backupRetention = ref<number | null>(10)
const backupScheduleEnabled = ref(true)
const backupScheduleDays = ref<number | null>(1)
const backupScheduleSaving = ref<string | null>(null)
const scheduledBackupBusy = ref(false)
const dictRows = ref<Array<{ id: number; type: string; value: string; label: string; sort_order: number; enabled: boolean }>>([])
const dictLoading = ref(false)
const dictSaving = ref<number | null>(null)
const newDict = ref({ type: '', value: '', label: '', sort_order: 10, enabled: true })
const message = useMessage()
const dialog = useDialog()
let unlistenCloseRequested: (() => void) | null = null
let unlistenTrayExit: UnlistenFn | null = null
let unlistenTasks: (() => void) | null = null
let backupScheduleTimer: number | null = null
let cloudSyncTimer: number | null = null

const iconMap: Record<string, unknown> = {
  dashboard: LayoutDashboard,
  users: () => import('lucide-vue-next').then((mod) => mod.Users),
}

const lucideIcons: Record<string, unknown> = {}
async function resolveIcon(name: string) {
  if (lucideIcons[name]) return lucideIcons[name]
  const icons = await import('lucide-vue-next')
  const names: Record<string, keyof typeof icons> = {
    users: 'Users', contact: 'ContactRound', briefcase: 'BriefcaseBusiness', package: 'Package', flask: 'FlaskConical', 'file-text': 'FileText', 'file-signature': 'FileSignature', truck: 'Truck', 'file-output': 'FileOutput', wallet: 'WalletCards', 'circle-dollar-sign': 'CircleDollarSign', factory: 'Factory', 'shopping-cart': 'ShoppingCart', boxes: 'Boxes', warehouse: 'Warehouse', layers: 'Layers', paperclip: 'Paperclip', 'chart-no-axes-combined': 'ChartNoAxesCombined',
  }
  lucideIcons[name] = icons[names[name]]
  return lucideIcons[name]
}

const menuOptions = computed<MenuOption[]>(() => {
  const output: MenuOption[] = []
  for (const group of navigationGroups) {
    if (group.keys.length === 1 && group.keys[0] === 'dashboard') {
      output.push({ key: 'dashboard', label: '工作台', icon: () => h(LayoutDashboard, { size: 18 }) })
      continue
    }
    output.push({
      key: `group-${group.label}`,
      type: 'group',
      label: group.label,
      children: group.keys.map((key) => {
        const definition = moduleMap[key]
        return { key, label: definition.label, icon: () => h(resolveIconSync(definition.icon), { size: 17 }) }
      }),
    })
  }
  return output
})

function resolveIconSync(name: string) {
  const known: Record<string, unknown> = {
    users: (awaitedIcons as Record<string, unknown>).Users,
    contact: (awaitedIcons as Record<string, unknown>).ContactRound,
    briefcase: (awaitedIcons as Record<string, unknown>).BriefcaseBusiness,
    package: (awaitedIcons as Record<string, unknown>).Package,
    flask: (awaitedIcons as Record<string, unknown>).FlaskConical,
    'file-text': (awaitedIcons as Record<string, unknown>).FileText,
    'file-signature': (awaitedIcons as Record<string, unknown>).FileSignature,
    truck: (awaitedIcons as Record<string, unknown>).Truck,
    'file-output': (awaitedIcons as Record<string, unknown>).FileOutput,
    wallet: (awaitedIcons as Record<string, unknown>).WalletCards,
    'circle-dollar-sign': (awaitedIcons as Record<string, unknown>).CircleDollarSign,
    factory: (awaitedIcons as Record<string, unknown>).Factory,
    'shopping-cart': (awaitedIcons as Record<string, unknown>).ShoppingCart,
    boxes: (awaitedIcons as Record<string, unknown>).Boxes,
    warehouse: (awaitedIcons as Record<string, unknown>).Warehouse,
    layers: (awaitedIcons as Record<string, unknown>).Layers,
    paperclip: (awaitedIcons as Record<string, unknown>).Paperclip,
    'chart-no-axes-combined': (awaitedIcons as Record<string, unknown>).ChartNoAxesCombined,
  }
  return known[name] || PackageIconFallback
}

// Static imports keep the menu render synchronous and avoid a delayed icon flash.
import * as awaitedIcons from 'lucide-vue-next'
const PackageIconFallback = awaitedIcons.Package

const currentModule = computed(() => activeKey.value === 'dashboard' ? null : moduleMap[activeKey.value])
const activeTaskSummary = computed(() => activeTaskRows.value.map((task) => task.label).join('、'))

function taskNames(tasks: Array<{ label: string }>) {
  return tasks.map((task) => task.label).join('、')
}

function navigate(key: string) {
  if (key === activeKey.value) return
  const drafts = dirtyDrafts()
  if (drafts.length) {
    pendingNavigation.value = key
    draftGuardCount.value = drafts.length
    draftGuardOpen.value = true
    return
  }
  activeKey.value = key
}

function clearDraftNavigation() {
  pendingNavigation.value = null
  draftGuardOpen.value = false
  draftGuardCount.value = 0
}

async function resolveDraftNavigation(action: 'save' | 'discard' | 'cancel') {
  if (action === 'cancel') {
    clearDraftNavigation()
    return
  }
  if (draftGuardBusy.value) return
  draftGuardBusy.value = true
  try {
    if (action === 'save') {
      const saved = await saveDirtyDrafts()
      if (!saved) {
        message.error('仍有未保存内容未成功保存，请检查表单后重试。')
        return
      }
    } else {
      const discarded = discardDirtyDrafts()
      if (!discarded) {
        message.error('未保存内容未能安全丢弃，请检查当前表单后重试。')
        return
      }
    }
    const destination = pendingNavigation.value
    clearDraftNavigation()
    if (destination) activeKey.value = destination
  } finally {
    draftGuardBusy.value = false
  }
}

function errorText(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}

async function initialize(password?: string) {
  booting.value = true
  bootError.value = ''
  if (!password) unlockError.value = ''
  try {
    bootstrap.value = await command<BootstrapInfo>('initialize', { password: password?.trim() || null })
    closeBehavior.value = bootstrap.value.close_behavior === 'confirm_exit' ? 'confirm_exit' : 'minimize_to_tray'
    locked.value = false
    unlockSecret.value = ''
    showRecoveryNotice.value = bootstrap.value.previous_exit_dirty
    recoveryPickerOpen.value = false
    try {
      await command('mark_ui_ready')
    } catch {
      // The readiness marker is diagnostic only; a state-directory write error
      // must not turn an otherwise healthy local account into a startup failure.
    }
  } catch (error) {
    const detail = errorText(error)
    if (detail === 'PROTECTED_LOCKED' || detail.includes('PROTECTED_LOCKED')) {
      locked.value = true
      if (password) unlockError.value = '口令或恢复密钥不正确，请重试。'
    } else if (detail.includes('主库加密文件损坏')) {
      locked.value = false
      bootError.value = detail
      void loadBackups()
    } else if (detail.includes('口令或恢复密钥不正确') || detail.includes('发现加密数据库但无法解锁') || detail.includes('尚未解锁')) {
      locked.value = true
      unlockError.value = detail
    } else {
      locked.value = false
      bootError.value = detail
      void loadBackups()
    }
  } finally {
    booting.value = false
  }
}

async function unlock() {
  const secret = unlockSecret.value.trim()
  if (!secret) {
    unlockError.value = '请输入本地口令或恢复密钥。'
    return
  }
  unlocking.value = true
  try {
    await initialize(secret)
    if (bootstrap.value) void startBackupScheduler()
  } finally {
    unlocking.value = false
  }
}

async function backupNow(options: { scheduled?: boolean } = {}): Promise<boolean> {
  const scheduled = Boolean(options.scheduled)
  if (scheduled && scheduledBackupBusy.value) return false
  if (scheduled) scheduledBackupBusy.value = true
  backupError.value = ''
  try {
    const result = await command<{ path: string; checksum: string }>('backup_now')
    if (scheduled) message.info('已按计划完成自动备份')
    else message.success(`备份完成：${result.path}`)
    await loadBackups()
    return true
  } catch (error) {
    backupError.value = errorText(error)
    if (scheduled) message.warning(`定时备份未完成：${backupError.value}`)
    else message.error(backupError.value)
    return false
  } finally {
    if (scheduled) scheduledBackupBusy.value = false
  }
}

async function loadBackups() {
  backupsLoading.value = true
  try {
    backups.value = await command<BackupEntry[]>('list_backups')
  } catch (error) {
    message.error(errorText(error))
  } finally {
    backupsLoading.value = false
  }
}

async function loadBusinessSettings() {
  businessSettingsLoading.value = true
  dictLoading.value = true
  try {
    const [settings, dicts] = await Promise.all([
      command<Array<{ key: string; value: string }>>('list_settings'),
      command<Array<{ id: number; type: string; value: string; label: string; sort_order: number; enabled: boolean }>>('list_dicts'),
    ])
    applySettings(settings)
    dictRows.value = dicts.map((item) => ({ ...item, sort_order: Number(item.sort_order) || 0, enabled: Boolean(item.enabled) }))
  } catch (error) {
    message.error(errorText(error))
  } finally {
    businessSettingsLoading.value = false
    dictLoading.value = false
  }
}

function applySettings(settings: Array<{ key: string; value: string }>) {
    const values = Object.fromEntries(settings.map((item) => [item.key, Number(item.value)]))
    const persistedCloseBehavior = settings.find((item) => item.key === 'close_behavior')?.value
    closeBehavior.value = persistedCloseBehavior === 'confirm_exit' ? 'confirm_exit' : 'minimize_to_tray'
    reminderDays.value = Number.isFinite(values.reminder_days) ? values.reminder_days : 3
    backupRetention.value = Number.isFinite(values.backup_retention) ? values.backup_retention : 10
    const scheduleEnabled = settings.find((item) => item.key === 'backup_schedule_enabled')?.value
    backupScheduleEnabled.value = scheduleEnabled === undefined || ['1', 'true', 'on', 'yes'].includes(scheduleEnabled.toLowerCase())
    const scheduleDays = Number(settings.find((item) => item.key === 'backup_schedule_days')?.value)
    backupScheduleDays.value = Number.isFinite(scheduleDays) ? Math.max(1, Math.min(30, Math.round(scheduleDays))) : 1
}

async function loadBackupScheduleSettings() {
  const settings = await command<Array<{ key: string; value: string }>>('list_settings')
  applySettings(settings)
}

async function maybeRunScheduledBackup() {
  if (!bootstrap.value || locked.value || !backupScheduleEnabled.value || scheduledBackupBusy.value) return
  if (activeTaskRows.value.some((task) => task.kind === 'backup')) return
  const intervalDays = backupScheduleDays.value ?? 1
  if (!shouldRunScheduledBackup(backups.value, true, intervalDays)) return
  await backupNow({ scheduled: true })
}

async function startBackupScheduler() {
  if (!bootstrap.value || locked.value) return
  try {
    await Promise.all([loadBackupScheduleSettings(), loadBackups()])
    await maybeRunScheduledBackup()
  } catch (error) {
    message.warning(`定时备份检查未完成：${errorText(error)}`)
  }
  if (backupScheduleTimer === null) {
    backupScheduleTimer = window.setInterval(() => { void maybeRunScheduledBackup() }, 60_000)
  }
}

function requestCloudSync() {
  if (!bootstrap.value || locked.value) return
  void syncPendingCloudEvents().catch(() => undefined)
}

function startCloudSyncScheduler() {
  requestCloudSync()
  if (cloudSyncTimer === null) cloudSyncTimer = window.setInterval(requestCloudSync, 60_000)
}

async function saveCloseBehavior(value: CloseBehavior) {
  const previous = bootstrap.value?.close_behavior ?? closeBehavior.value
  closeBehavior.value = value
  closeBehaviorSaving.value = true
  try {
    const saved = await command<{ value: string }>('save_settings', { key: 'close_behavior', value })
    closeBehavior.value = saved.value === 'confirm_exit' ? 'confirm_exit' : 'minimize_to_tray'
    if (bootstrap.value) bootstrap.value = { ...bootstrap.value, close_behavior: closeBehavior.value }
    message.success('关闭窗口行为已保存')
  } catch (error) {
    closeBehavior.value = previous
    message.error(errorText(error))
  } finally {
    closeBehaviorSaving.value = false
  }
}

async function saveBusinessSetting(key: 'reminder_days' | 'backup_retention', value: number | null) {
  if (value == null || !Number.isInteger(value)) {
    message.warning('设置值必须是整数')
    return
  }
  businessSettingsSaving.value = key
  try {
    await command('save_settings', { key, value: String(value) })
    message.success('业务参数已保存')
  } catch (error) {
    message.error(errorText(error))
  } finally {
    businessSettingsSaving.value = null
  }
}

async function saveBackupScheduleSetting(key: 'backup_schedule_enabled' | 'backup_schedule_days', value: boolean | number | null) {
  if (key === 'backup_schedule_days' && (value == null || !Number.isInteger(value))) {
    message.warning('定时备份间隔必须是整数')
    return
  }
  const previousEnabled = backupScheduleEnabled.value
  const previousDays = backupScheduleDays.value
  backupScheduleSaving.value = key
  try {
    const serialized = key === 'backup_schedule_enabled' ? (value ? '1' : '0') : String(value)
    const saved = await command<{ value: string }>('save_settings', { key, value: serialized })
    if (key === 'backup_schedule_enabled') backupScheduleEnabled.value = saved.value === '1'
    else backupScheduleDays.value = Number(saved.value)
    message.success('定时备份设置已保存')
    if (key === 'backup_schedule_enabled' && backupScheduleEnabled.value) await maybeRunScheduledBackup()
  } catch (error) {
    backupScheduleEnabled.value = previousEnabled
    backupScheduleDays.value = previousDays
    message.error(errorText(error))
  } finally {
    backupScheduleSaving.value = null
  }
}

async function saveDict(row: { id: number; type: string; value: string; label: string; sort_order: number; enabled: boolean }) {
  if (!row.label.trim()) {
    message.warning('字典显示名不能为空')
    return
  }
  dictSaving.value = row.id
  try {
    await command('save_dict', { data: row })
    message.success('字典项已保存')
  } catch (error) {
    message.error(errorText(error))
  } finally {
    dictSaving.value = null
  }
}

async function addDict() {
  if (!newDict.value.type.trim() || !newDict.value.value.trim() || !newDict.value.label.trim()) {
    message.warning('请填写字典类型、code 和显示名')
    return
  }
  try {
    const saved = await command<{ id: number; type: string; value: string; label: string; sort_order: number; enabled: boolean }>('save_dict', { data: { ...newDict.value } })
    dictRows.value.push(saved)
    newDict.value = { type: '', value: '', label: '', sort_order: 10, enabled: true }
    message.success('字典项已新增')
  } catch (error) {
    message.error(errorText(error))
  }
}

function backupName(path: string) {
  return path.split(/[\\/]/).pop() || path
}

function backupStatusLabel(entry: BackupEntry) {
  if (entry.result === 'success') return '校验通过'
  if (entry.result === 'checksum_mismatch') return '校验不匹配'
  if (entry.result === 'unreadable') return '文件不可读'
  if (entry.result === 'missing') return '文件缺失'
  return '未登记记录'
}

function backupUsable(entry: BackupEntry) {
  return entry.exists && !['checksum_mismatch', 'unreadable', 'missing'].includes(entry.result)
}

function formatBytes(value: number) {
  if (value < 1024) return `${value} B`
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`
  return `${(value / (1024 * 1024)).toFixed(1)} MB`
}

function formatBackupDate(value: string) {
  if (!value) return '时间未知'
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString('zh-CN', { hour12: false })
}

function openRecoveryBackups() {
  if (bootError.value) {
    recoveryPickerOpen.value = true
    void loadBackups()
    return
  }
  showRecoveryNotice.value = false
  settingsTab.value = 'backup'
  showSettings.value = true
  void loadBackups()
}

async function enableProtection() {
  if (protectionPassword.value.length < 8) {
    message.warning('本地口令至少需要 8 个字符')
    return
  }
  if (protectionPassword.value !== protectionConfirm.value) {
    message.warning('两次输入的口令不一致')
    return
  }
  protectionBusy.value = true
  try {
    const result = await command<ProtectionSetupResult>('set_protection', { password: protectionPassword.value })
    recoveryKey.value = result.recovery_key || ''
    if (bootstrap.value) bootstrap.value = { ...bootstrap.value, protection_enabled: true }
    protectionPassword.value = ''
    protectionConfirm.value = ''
    message.success('保护模式已启用；请立即保存恢复密钥。')
    await loadBackups()
  } catch (error) {
    message.error(errorText(error))
  } finally {
    protectionBusy.value = false
  }
}

function requestDisableProtection() {
  if (!disableSecret.value.trim()) {
    message.warning('请输入当前口令或恢复密钥')
    return
  }
  dialog.warning({
    title: '关闭保护模式',
    content: '关闭后主库将以未加密形式保存。确认继续吗？',
    positiveText: '确认关闭',
    negativeText: '取消',
    onPositiveClick: disableProtection,
  })
}

async function disableProtection() {
  protectionBusy.value = true
  try {
    await command('disable_protection', { secret: disableSecret.value.trim() })
    disableSecret.value = ''
    recoveryKey.value = ''
    if (bootstrap.value) bootstrap.value = { ...bootstrap.value, protection_enabled: false }
    message.success('保护模式已关闭')
  } catch (error) {
    message.error(errorText(error))
  } finally {
    protectionBusy.value = false
  }
}

async function copyRecoveryKey() {
  if (!recoveryKey.value) return
  try {
    await navigator.clipboard.writeText(recoveryKey.value)
    message.success('恢复密钥已复制')
  } catch {
    message.warning('当前环境无法自动复制，请手动记录恢复密钥。')
  }
}

async function restoreBackup(entry: BackupEntry) {
  if (!backupUsable(entry)) return
  const recoveryOnly = !bootstrap.value || Boolean(bootError.value)
  dialog.warning({
    title: '还原备份',
    content: recoveryOnly
      ? `当前账套尚未成功打开。系统会先隔离现有数据文件，再验证并还原“${backupName(entry.path)}”。继续吗？`
      : `将先自动备份当前账套，再还原“${backupName(entry.path)}”。还原后会在当前窗口重新检查数据。继续吗？`,
    positiveText: '确认还原',
    negativeText: '取消',
    onPositiveClick: async () => {
      restoringPath.value = entry.path
      try {
        await command('restore_backup', { backupPath: entry.path })
        message.success('备份已还原，正在重新检查账套。')
        await loadBackups()
        activeKey.value = 'dashboard'
        recoveryPickerOpen.value = false
        await initialize()
        if (bootstrap.value) void startBackupScheduler()
      } catch (error) {
        message.error(errorText(error))
      } finally {
        restoringPath.value = ''
      }
    },
  })
}

async function integrityCheck() {
  integrityChecking.value = true
  try {
    await command('integrity_check_command')
    message.success('数据库完整性检查通过')
  } catch (error) {
    message.error(errorText(error))
  } finally {
    integrityChecking.value = false
  }
}

async function requestShutdown() {
  if (shuttingDown.value || locked.value || !bootstrap.value) return
  shuttingDown.value = true
  try {
    await command('shutdown')
  } finally {
    shuttingDown.value = false
  }
}

async function waitForNativeTasks(deadline: number): Promise<NativeBackgroundTask[]> {
  let remaining: NativeBackgroundTask[] = []
  while (Date.now() < deadline) {
    remaining = await command<NativeBackgroundTask[]>('list_background_tasks')
    if (!remaining.length) return []
    await new Promise((resolve) => window.setTimeout(resolve, 120))
  }
  return remaining
}

async function waitForBackgroundTasks(): Promise<Array<{ label: string }>> {
  const deadline = Date.now() + 30_000
  const localRemaining = await waitForTasks(Math.max(0, deadline - Date.now()))
  if (localRemaining.length) return localRemaining
  const nativeRemaining = await waitForNativeTasks(deadline)
  return nativeRemaining.map((task) => ({ label: task.label }))
}

async function minimizeToTray() {
  try {
    await getCurrentWindow().hide()
  } catch {
    requestExit('explicit')
  }
}

function requestExit(source: 'window' | 'explicit' | 'tray' = 'explicit') {
  if (shuttingDown.value || locked.value || !bootstrap.value) return
  if (source === 'window' && closeBehavior.value === 'minimize_to_tray') {
    void minimizeToTray()
    return
  }
  if (exitDialogOpen.value) return
  exitDraftCount.value = dirtyDrafts().length
  exitDialogOpen.value = true
}

async function resolveExit(action: 'save' | 'discard' | 'cancel') {
  if (action === 'cancel') {
    exitDialogOpen.value = false
    return
  }
  exitBusy.value = true
  try {
    if (action === 'save') {
      const saved = await saveDirtyDrafts()
      if (!saved) {
        message.error('仍有未保存内容未成功保存，暂未退出。')
        return
      }
    } else if (action === 'discard') {
      const discarded = discardDirtyDrafts()
      if (!discarded) {
        message.error('未保存内容未能安全丢弃，暂未退出。')
        return
      }
    }
    const pendingTasks = await waitForBackgroundTasks()
    if (pendingTasks.length) {
      message.error(`后台任务仍在运行：${taskNames(pendingTasks)}。已取消退出，请等待任务完成后重试。`)
      return
    }
    await requestShutdown()
    exitDialogOpen.value = false
    if (isTauriRuntime()) await getCurrentWindow().destroy()
    else message.info('演示模式已完成退出前检查。')
  } catch (error) {
    message.error(errorText(error))
  } finally {
    exitBusy.value = false
  }
}

function onKeydown(event: KeyboardEvent) {
  if (event.ctrlKey && event.key.toLowerCase() === 'd') { event.preventDefault(); navigate('dashboard') }
  if (event.ctrlKey && event.key.toLowerCase() === 'b' && !locked.value) { event.preventDefault(); backupNow() }
  if (event.ctrlKey && event.key.toLowerCase() === 'q') { event.preventDefault(); requestExit() }
  if (event.key === 'F5') { event.preventDefault(); window.location.reload() }
  if (event.key === 'Escape' && !draftGuardBusy.value && !exitBusy.value) { showHelp.value = false; showSettings.value = false; showRecoveryNotice.value = false; clearDraftNavigation(); exitDialogOpen.value = false }
}

onMounted(async () => {
  unlistenTasks = subscribeTasks((tasks) => { activeTaskRows.value = tasks })
  window.addEventListener('keydown', onKeydown)
  window.addEventListener('erp:local-business-write', requestCloudSync)
  try {
    const current = getCurrentWindow()
    unlistenCloseRequested?.()
    unlistenCloseRequested = await current.onCloseRequested(async (event) => {
      if (locked.value || !bootstrap.value) return
      event.preventDefault()
      requestExit('window')
    })
    unlistenTrayExit = await listen('erp://request-exit', () => requestExit('tray'))
  } catch (error) {
    if (isTauriRuntime()) message.error(`原生窗口事件监听失败：${errorText(error)}`)
  }
  await initialize()
  if (bootstrap.value) {
    void startBackupScheduler()
    startCloudSyncScheduler()
  }
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKeydown)
  window.removeEventListener('erp:local-business-write', requestCloudSync)
  unlistenCloseRequested?.()
  unlistenCloseRequested = null
  unlistenTrayExit?.()
  unlistenTrayExit = null
  unlistenTasks?.()
  unlistenTasks = null
  if (backupScheduleTimer !== null) {
    window.clearInterval(backupScheduleTimer)
    backupScheduleTimer = null
  }
  if (cloudSyncTimer !== null) {
    window.clearInterval(cloudSyncTimer)
    cloudSyncTimer = null
  }
})

watch(showSettings, (visible) => {
  if (visible) {
    void loadBackups()
    void loadBusinessSettings()
  }
})
</script>

<template>
  <NSpin :show="booting" description="正在检查数据并加载工作台…">
          <div v-if="locked" class="lock-screen">
            <div class="lock-panel">
              <div class="lock-icon"><LockKeyhole :size="25" /></div>
              <h1>账套已锁定</h1>
              <p>该账套已启用本地保护模式。输入本地口令或恢复密钥后继续。</p>
              <NForm @submit.prevent="unlock">
                <NFormItem label="本地口令 / 恢复密钥">
                  <NInput v-model:value="unlockSecret" type="password" autocomplete="current-password" show-password-on="click" placeholder="请输入密钥" @keyup.enter="unlock" />
                </NFormItem>
                <NAlert v-if="unlockError" type="error" :bordered="false" class="lock-error">{{ unlockError }}</NAlert>
                <NButton type="primary" block :loading="unlocking" @click="unlock">解锁账套</NButton>
              </NForm>
              <small>恢复密钥只应保存在非 U 盘介质。</small>
            </div>
          </div>
          <NResult v-else-if="bootError" status="error" title="无法打开数据账套" :description="bootError">
            <template #footer>
              <div class="boot-recovery-actions">
                <NButton secondary @click="openRecoveryBackups"><template #icon><DatabaseBackup :size="16" /></template>查看备份并恢复</NButton>
                <NButton type="primary" @click="() => initialize()">重试</NButton>
              </div>
            </template>
          </NResult>
          <NLayout v-else has-sider class="app-shell">
            <NLayoutSider v-model:collapsed="collapsed" :collapsed-width="68" :width="232" bordered collapse-mode="width" class="app-sider">
              <div class="brand-block">
                <div class="brand-mark">ERP</div>
                <div v-if="!collapsed" class="brand-copy"><strong>改性塑料销售</strong><span>个人工作进度</span></div>
              </div>
              <NMenu :value="activeKey" :collapsed="collapsed" :collapsed-width="68" :collapsed-icon-size="20" :options="menuOptions" class="app-menu" @update:value="navigate" />
              <div class="sider-bottom">
                <NButton quaternary block :class="{ 'icon-only-button': collapsed }" @click="showHelp = true"><template #icon><CircleHelp :size="17" /></template><span v-if="!collapsed">帮助与恢复</span></NButton>
                <NButton quaternary block :class="{ 'icon-only-button': collapsed }" @click="showSettings = true"><template #icon><Settings2 :size="17" /></template><span v-if="!collapsed">基础设置</span></NButton>
                <div v-if="!collapsed" class="sider-version">V{{ APP_VERSION }} · 本地离线账套</div>
              </div>
            </NLayoutSider>
            <NLayout>
              <NLayoutHeader bordered class="app-header">
                <div class="header-left">
                  <NButton quaternary circle aria-label="折叠导航" @click="collapsed = !collapsed"><ChevronLeft v-if="!collapsed" :size="18" /><ChevronRight v-else :size="18" /></NButton>
                  <div class="breadcrumbs"><span>改性塑料颗粒销售</span><ChevronRight :size="14" /><strong>{{ activeKey === 'dashboard' ? '工作台' : currentModule?.label }}</strong></div>
                </div>
                <div class="header-actions">
                  <NTag v-if="bootstrap" size="small" :bordered="false" type="success"><ShieldCheck :size="14" />本地安全模式</NTag>
                  <NTooltip v-if="activeTaskRows.length">
                    <template #trigger><NTag size="small" :bordered="false" type="warning"><LoaderCircle :size="14" class="task-spinner" />{{ activeTaskRows.length }} 项处理中</NTag></template>
                    {{ activeTaskSummary }}
                  </NTooltip>
                  <NBadge :value="bootstrap?.previous_exit_dirty ? '!' : 0" :show-zero="false" type="warning">
                    <NTooltip><template #trigger><NButton quaternary circle aria-label="提醒" @click="navigate('dashboard')"><Bell :size="18" /></NButton></template>查看待办</NTooltip>
                  </NBadge>
                  <NTooltip><template #trigger><NButton quaternary circle aria-label="立即备份" @click="() => backupNow()"><DatabaseBackup :size="18" /></NButton></template>立即备份（Ctrl+B）</NTooltip>
                  <NButton quaternary circle aria-label="存储位置" @click="showSettings = true"><HardDrive :size="18" /></NButton>
                </div>
              </NLayoutHeader>
              <NLayoutContent class="app-content">
                <DashboardView v-if="activeKey === 'dashboard'" @navigate="navigate" />
                <EntityPage v-else-if="currentModule" :key="activeKey" :module="currentModule" />
              </NLayoutContent>
            </NLayout>
          </NLayout>
  </NSpin>

        <NModal v-model:show="showRecoveryNotice" preset="card" title="检测到异常退出" :mask-closable="false" :closable="false" class="recovery-modal">
          <NAlert type="warning" :bordered="false" title="已完成启动完整性检查">
            上次运行未正常结束；当前账套已通过 SQLite 完整性检查，可以继续使用。
          </NAlert>
          <div class="recovery-details">
            <span>上次状态时间</span><strong>{{ bootstrap?.previous_exit_time ? formatBackupDate(bootstrap.previous_exit_time) : '退出状态记录缺失或损坏' }}</strong>
            <span>检查结果</span><strong>{{ bootstrap?.startup_integrity === 'verified' ? '正常' : bootstrap?.startup_integrity }}</strong>
            <span>恢复日志目录</span><code>{{ bootstrap?.recovery_log_path }}</code>
          </div>
          <p class="recovery-note">建议核对最近备份；只有在完整性检查失败或业务数据异常时才执行还原。</p>
          <div class="modal-actions recovery-actions">
            <NButton @click="openRecoveryBackups"><template #icon><DatabaseBackup :size="16" /></template>查看备份</NButton>
            <NButton type="primary" @click="showRecoveryNotice = false">继续使用</NButton>
          </div>
        </NModal>

        <NModal v-model:show="recoveryPickerOpen" preset="card" title="从备份恢复账套" :mask-closable="false" :closable="false" class="recovery-picker-modal">
          <NAlert type="warning" :bordered="false" title="当前账套未通过完整性检查">
            还原前会保留现有数据库文件的隔离副本；只有校验通过且能再次打开的备份才会替换当前账套。
          </NAlert>
          <div class="backup-list-heading"><div><h3>可用备份</h3><p>未登记文件也会先做 SQLite 完整性检查；校验不匹配的文件不能还原。</p></div><NTooltip><template #trigger><NButton quaternary circle aria-label="刷新恢复备份列表" :loading="backupsLoading" @click="loadBackups"><RefreshCw :size="16" /></NButton></template>刷新备份列表</NTooltip></div>
          <div v-if="backupsLoading" class="backup-list-empty">正在读取备份文件…</div>
          <div v-else-if="!backups.length" class="backup-list-empty">未找到可用备份，请检查 backup 目录。</div>
          <div v-else class="backup-list recovery-backup-list">
            <div v-for="entry in backups" :key="entry.path" class="backup-row">
              <div class="backup-copy"><strong>{{ backupName(entry.path) }}</strong><span>{{ formatBackupDate(entry.created_at) }} · {{ formatBytes(entry.size_bytes) }} · {{ backupStatusLabel(entry) }}</span><small>{{ entry.checksum || '无法读取校验值' }}</small></div>
              <NButton secondary size="small" :disabled="!backupUsable(entry) || Boolean(restoringPath)" :loading="restoringPath === entry.path" @click="restoreBackup(entry)"><template #icon><ArchiveRestore :size="15" /></template>还原</NButton>
            </div>
          </div>
          <div class="modal-actions recovery-actions"><NButton @click="recoveryPickerOpen = false">关闭</NButton><NButton type="primary" secondary @click="loadBackups" :loading="backupsLoading">重新读取</NButton></div>
        </NModal>

        <NModal v-model:show="draftGuardOpen" preset="card" title="存在未保存内容" :mask-closable="false" :closable="false" :close-on-esc="false" class="draft-modal">
          <p class="draft-warning">当前有 {{ draftGuardCount }} 个录入尚未保存，继续切换页面会丢失这些内容。</p>
          <div class="modal-actions">
            <NButton :disabled="draftGuardBusy" @click="resolveDraftNavigation('cancel')">继续编辑</NButton>
            <NButton type="error" secondary :disabled="draftGuardBusy" @click="resolveDraftNavigation('discard')">不保存并继续</NButton>
            <NButton type="primary" :loading="draftGuardBusy" @click="resolveDraftNavigation('save')">保存并继续</NButton>
          </div>
        </NModal>

        <NModal v-model:show="exitDialogOpen" preset="card" title="退出 ERP" :mask-closable="false" :closable="false" :close-on-esc="false" class="exit-modal">
          <NAlert v-if="exitDraftCount" type="warning" :bordered="false" title="存在未保存内容">
            当前有 {{ exitDraftCount }} 个录入尚未保存。请选择保存并退出，或明确放弃这些内容。
          </NAlert>
          <NAlert v-if="activeTaskRows.length" type="info" :bordered="false" title="正在等待后台任务">
            <div>退出前会等待当前操作完成，避免备份、导出或附件文件处于半写入状态。</div>
            <ul class="task-list"><li v-for="task in activeTaskRows" :key="task.id">{{ task.label }}</li></ul>
          </NAlert>
          <p v-if="!exitDraftCount && !activeTaskRows.length" class="exit-note">退出前会完成一致性备份、WAL checkpoint 并释放数据文件。</p>
          <div class="modal-actions">
            <NButton :disabled="exitBusy" @click="resolveExit('cancel')">继续使用</NButton>
            <NButton v-if="exitDraftCount" type="error" secondary :disabled="exitBusy" @click="resolveExit('discard')">不保存退出</NButton>
            <NButton type="primary" :loading="exitBusy" @click="resolveExit('save')">{{ exitDraftCount ? '保存并退出' : '退出并备份' }}</NButton>
          </div>
        </NModal>

        <NDrawer v-model:show="showHelp" :width="440" placement="right">
          <NDrawerContent title="帮助与恢复" closable>
            <div class="help-content">
              <NAlert type="info" :bordered="false" title="数据位置">
                当前账套、备份、导出和日志均位于程序目录下的相对路径，拷贝整个目录即可迁移。
              </NAlert>
              <h3>常用快捷键</h3>
              <div class="shortcut-list"><span>打开工作台</span><kbd>Ctrl + D</kbd><span>立即备份</span><kbd>Ctrl + B</kbd><span>刷新列表</span><kbd>F5</kbd><span>关闭弹窗</span><kbd>Esc</kbd></div>
              <h3>恢复步骤</h3>
              <ol><li>在“基础设置 → 备份与恢复”确认最近一份校验通过的备份。</li><li>先备份当前目录，再选择备份文件执行还原。</li><li>还原完成后重新打开程序，检查工作台指标和关键单据。</li></ol>
              <NButton type="primary" secondary block @click="showHelp = false">知道了</NButton>
            </div>
          </NDrawerContent>
        </NDrawer>

        <NModal v-model:show="showSettings" preset="card" title="基础设置" class="settings-modal">
          <NTabs v-model:value="settingsTab" type="line" animated>
            <NTabPane name="backup" tab="备份与恢复">
              <div class="settings-section"><div><h3>便携账套</h3><p>{{ bootstrap?.root || '正在读取…' }}</p></div><NButton type="primary" secondary @click="() => backupNow()"><template #icon><DatabaseBackup :size="16" /></template>立即备份</NButton></div>
              <NAlert v-if="backupError" type="error" :bordered="false" title="备份未完成"><div>{{ backupError }}</div><div class="backup-error-actions"><span>请检查可用空间、备份目录权限和占用程序后重试。</span><NButton size="small" type="error" secondary @click="() => backupNow()">重试备份</NButton></div></NAlert>
              <div class="backup-list-heading"><div><h3>最近备份</h3><p>备份文件带有 SHA-256 校验值；还原前会自动创建当前账套的保护备份。</p></div><NTooltip><template #trigger><NButton quaternary circle aria-label="刷新备份列表" :loading="backupsLoading" @click="loadBackups"><RefreshCw :size="16" /></NButton></template>刷新备份列表</NTooltip></div>
              <div v-if="backupsLoading" class="backup-list-empty">正在读取备份记录…</div>
              <div v-else-if="!backups.length" class="backup-list-empty">暂无可用备份</div>
              <div v-else class="backup-list">
                  <div v-for="entry in backups" :key="entry.path" class="backup-row">
                  <div class="backup-copy"><strong>{{ backupName(entry.path) }}</strong><span>{{ formatBackupDate(entry.created_at) }} · {{ formatBytes(entry.size_bytes) }} · {{ backupStatusLabel(entry) }}</span><small>{{ entry.checksum || '无法读取校验值' }}</small></div>
                  <NButton secondary size="small" :disabled="!backupUsable(entry) || Boolean(restoringPath)" :loading="restoringPath === entry.path" @click="restoreBackup(entry)"><template #icon><ArchiveRestore :size="15" /></template>还原</NButton>
                </div>
              </div>
              <NButton quaternary size="small" :loading="integrityChecking" @click="integrityCheck"><template #icon><CheckCircle2 :size="15" /></template>运行完整性检查</NButton>
              <div class="settings-section settings-section-stack backup-schedule-section">
                <div>
                  <h3>定时自动备份</h3>
                  <p>程序首次打开后会检查最近备份；托盘驻留期间每分钟检查一次，达到间隔后自动生成一致性备份。</p>
                </div>
                <div class="backup-schedule-controls">
                  <NSwitch
                    :value="backupScheduleEnabled"
                    :loading="backupScheduleSaving === 'backup_schedule_enabled'"
                    aria-label="启用定时自动备份"
                    @update:value="(value) => saveBackupScheduleSetting('backup_schedule_enabled', value)"
                  />
                  <span class="schedule-label">{{ backupScheduleEnabled ? '已启用' : '已停用' }}</span>
                  <NInputNumber v-model:value="backupScheduleDays" :min="1" :max="30" :precision="0" :disabled="!backupScheduleEnabled" aria-label="定时备份间隔天数" />
                  <span class="schedule-label">天</span>
                  <NButton type="primary" secondary :loading="backupScheduleSaving === 'backup_schedule_days'" :disabled="!backupScheduleEnabled" @click="saveBackupScheduleSetting('backup_schedule_days', backupScheduleDays)"><template #icon><Save :size="15" /></template>保存间隔</NButton>
                </div>
              </div>

              <div class="settings-section settings-section-stack"><div><h3>保护模式</h3><p>启用后，程序退出时会将主库加密落盘，备份也使用同一主密钥加密。</p></div><NTag :type="bootstrap?.protection_enabled ? 'success' : 'warning'">{{ bootstrap?.protection_enabled ? '已保护' : '未启用' }}</NTag></div>
              <div v-if="!bootstrap?.protection_enabled" class="protection-form">
                <NAlert type="info" :bordered="false">本地口令不会写入数据库；请设置至少 8 个字符的口令，并妥善保存随后生成的恢复密钥。</NAlert>
                <NForm label-placement="top">
                  <NFormItem label="设置本地口令"><NInput v-model:value="protectionPassword" type="password" autocomplete="new-password" show-password-on="click" placeholder="至少 8 个字符" /></NFormItem>
                  <NFormItem label="确认本地口令"><NInput v-model:value="protectionConfirm" type="password" autocomplete="new-password" show-password-on="click" placeholder="再次输入口令" @keyup.enter="enableProtection" /></NFormItem>
                </NForm>
                <NButton type="primary" :loading="protectionBusy" @click="enableProtection"><template #icon><ShieldCheck :size="16" /></template>启用保护模式</NButton>
              </div>
              <div v-else class="protection-form">
                <NAlert type="success" :bordered="false">保护模式已启用。恢复密钥不会再次从文件中读取，首次启用后请确认已经完成离线保存。</NAlert>
                <div v-if="recoveryKey" class="recovery-box"><div><strong>本次生成的恢复密钥</strong><code>{{ recoveryKey }}</code></div><NButton quaternary circle aria-label="复制恢复密钥" @click="copyRecoveryKey"><Clipboard :size="16" /></NButton></div>
                <NForm label-placement="top">
                  <NFormItem label="当前口令或恢复密钥"><NInput v-model:value="disableSecret" type="password" autocomplete="current-password" show-password-on="click" placeholder="用于关闭保护模式" /></NFormItem>
                </NForm>
                <NButton type="error" secondary :loading="protectionBusy" @click="requestDisableProtection">关闭保护模式</NButton>
              </div>
              <NAlert type="warning" :bordered="false">恢复密钥必须保存在非 U 盘介质。忘记本地口令时只能通过恢复密钥或人工恢复流程处理。</NAlert>
            </NTabPane>
            <NTabPane name="cloud" tab="云端备份">
              <CloudBackupPanel :local-backups="backups" :bootstrap="bootstrap" @changed="loadBackups" />
            </NTabPane>
            <NTabPane name="assessment" tab="运营评估">
              <OperationalAssessmentPanel :backups="backups" :protection-enabled="Boolean(bootstrap?.protection_enabled)" />
            </NTabPane>
            <NTabPane name="business" tab="业务参数">
              <NSpin :show="businessSettingsLoading">
                <div class="settings-section settings-section-stack">
                  <div><h3>回款临近提醒</h3><p>承诺回款日前 N 天进入“临近到期”；0 表示只在到期当天提醒。</p></div>
                  <div class="settings-inline-control"><NInputNumber v-model:value="reminderDays" :min="0" :max="365" :precision="0" /><NButton type="primary" secondary :loading="businessSettingsSaving === 'reminder_days'" @click="saveBusinessSetting('reminder_days', reminderDays)"><template #icon><Save :size="15" /></template>保存</NButton></div>
                </div>
                <div class="settings-section settings-section-stack">
                  <div><h3>备份保留份数</h3><p>自动清理最旧备份，允许设置 1–100 份。</p></div>
                  <div class="settings-inline-control"><NInputNumber v-model:value="backupRetention" :min="1" :max="100" :precision="0" /><NButton type="primary" secondary :loading="businessSettingsSaving === 'backup_retention'" @click="saveBusinessSetting('backup_retention', backupRetention)"><template #icon><Save :size="15" /></template>保存</NButton></div>
                </div>
                <div class="dict-heading"><div><h3>状态字典</h3><p>code 是历史数据的稳定标识，只能维护显示名、排序和启用状态。</p></div><NTag type="info">{{ dictRows.length }} 项</NTag></div>
                <div v-if="dictLoading" class="backup-list-empty">正在读取字典…</div>
                <div v-else class="dict-list">
                  <div v-for="row in dictRows" :key="row.id" class="dict-row">
                    <span class="dict-type">{{ row.type }}</span><code>{{ row.value }}</code>
                    <NInput v-model:value="row.label" size="small" aria-label="字典显示名" />
                    <NInputNumber v-model:value="row.sort_order" size="small" :min="0" :max="9999" :precision="0" aria-label="排序" />
                    <NSwitch v-model:value="row.enabled" size="small" aria-label="是否启用" />
                    <NButton quaternary size="small" :loading="dictSaving === row.id" @click="saveDict(row)"><template #icon><Save :size="14" /></template>保存</NButton>
                  </div>
                </div>
                <div class="dict-add-row">
                  <NInput v-model:value="newDict.type" size="small" placeholder="类型，如 freight" aria-label="新字典类型" />
                  <NInput v-model:value="newDict.value" size="small" placeholder="code" aria-label="新字典 code" />
                  <NInput v-model:value="newDict.label" size="small" placeholder="显示名" aria-label="新字典显示名" />
                  <NInputNumber v-model:value="newDict.sort_order" size="small" :min="0" :max="9999" :precision="0" aria-label="新字典排序" />
                  <NButton type="primary" secondary size="small" @click="addDict"><template #icon><Save :size="14" /></template>新增</NButton>
                </div>
              </NSpin>
            </NTabPane>
            <NTabPane name="appearance" tab="外观偏好">
              <div class="settings-section settings-section-stack"><div><h3>浅色工作台</h3><p>钢蓝主色与中性灰背景，适合长时间录入和打印。</p></div><NTag type="info">当前主题</NTag></div>
              <div class="settings-section settings-section-stack">
                <div><h3>关闭窗口行为</h3><p>点击标题栏 × 时可以暂存到托盘，Ctrl+Q 和托盘退出始终先执行退出确认。</p></div>
                <NRadioGroup v-model:value="closeBehavior" :disabled="closeBehaviorSaving" @update:value="saveCloseBehavior">
                  <NRadioButton value="minimize_to_tray">最小化到托盘</NRadioButton>
                  <NRadioButton value="confirm_exit">询问后退出</NRadioButton>
                </NRadioGroup>
              </div>
            </NTabPane>
            <NTabPane name="about" tab="关于 / 变更记录"><div class="about-block"><strong>改性塑料颗粒销售 ERP</strong><span>V{{ APP_VERSION }} · 2026-09-18</span><p>单人、离线、便携式本地账套。所有核心写操作使用事务并记录审计日志；新增定时自动备份。</p></div></NTabPane>
          </NTabs>
        </NModal>
</template>
