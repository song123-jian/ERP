<script setup lang="ts">
import { computed, h, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { Archive, ArrowRightLeft, Copy, Download, FileUp, GitCompareArrows, ListPlus, MoreHorizontal, Paperclip, Pencil, Plus, Printer, RefreshCw, Search, ShoppingCart, Upload } from 'lucide-vue-next'
import { NButton, NCheckbox, NDataTable, NDropdown, NEmpty, NForm, NFormItem, NInput, NInputNumber, NModal, NSelect, NSpace, NTooltip, type DataTableColumns, useDialog, useMessage } from 'naive-ui'
import { command } from '../api'
import { registerDraft, unregisterDraft } from '../drafts'
import { amountToChineseUpper } from '../print'
import { formatMoney, formatQuantity, labelForStatus } from '../theme'
import type { AttachmentRecord, DocumentItemRecord, EntityPage as EntityPageResult, EntityRecord, ModuleDefinition } from '../types'
import DocumentItemsModal from './DocumentItemsModal.vue'
import PrintPreviewModal from './PrintPreviewModal.vue'
import QuotationActionsModal from './QuotationActionsModal.vue'
import StatusBadge from './StatusBadge.vue'

const props = defineProps<{ module: ModuleDefinition }>()
const message = useMessage()
const dialog = useDialog()
const loading = ref(false)
const search = ref('')
const statusFilter = ref<string | null>(null)
const rows = ref<EntityRecord[]>([])
const total = ref(0)
const page = ref(1)
const pageSize = ref(20)
const editorOpen = ref(false)
const transitionOpen = ref(false)
const editing = ref<EntityRecord>({ id: 0 })
const editorBaseline = ref('')
const editorSaving = ref(false)
const draftCloseOpen = ref(false)
const transitionRecord = ref<EntityRecord | null>(null)
const nextStatus = ref<string | null>(null)
const transitionReason = ref('')
const transitionBaseline = ref('')
const transitionSaving = ref(false)
const transitionCloseOpen = ref(false)
const importInput = ref<HTMLInputElement | null>(null)
const lookupRows = ref<Record<string, EntityRecord[]>>({})
type ImportStrategy = 'skip' | 'rollback'
const importStrategy = ref<ImportStrategy>('rollback')
const importStrategyOptions = [
  { label: '失败行跳过', value: 'skip' as ImportStrategy },
  { label: '任一失败则回滚', value: 'rollback' as ImportStrategy },
]
const importReport = ref<{ batch_id?: string; inserted: number; skipped: number; failed: number; errors: string[]; failed_file?: string | null; failed_file_encrypted?: boolean; rolled_back?: boolean; preflight_ok?: boolean; strategy?: ImportStrategy } | null>(null)
const importReportOpen = ref(false)
const detailOpen = ref(false)
const detailRecord = ref<EntityRecord | null>(null)
const printOpen = ref(false)
const printRecord = ref<EntityRecord | null>(null)
const printItems = ref<DocumentItemRecord[]>([])
const printLoading = ref(false)
const attachmentOpen = ref(false)
const attachmentLoading = ref(false)
const attachmentTarget = ref<EntityRecord | null>(null)
const attachmentRows = ref<AttachmentRecord[]>([])
const attachmentInput = ref<HTMLInputElement | null>(null)
const attachmentBusy = ref(false)
const quotationActionOpen = ref(false)
const quotationActionMode = ref<'compare' | 'convert'>('compare')
const quotationActionRecord = ref<EntityRecord | null>(null)

const templateSupported = computed(() => ['customers', 'materials', 'quotations', 'contracts', 'orders', 'statements', 'purchases'].includes(props.module.key))
const attachmentSupported = computed(() => props.module.key !== 'attachments')
const attachmentMaxBytes = 25 * 1024 * 1024
const contractEditorTotalCents = computed(() => props.module.key === 'contracts' ? Number(editing.value.item_total_cents ?? 0) : 0)
const contractEditorTotalUpper = computed(() => amountToChineseUpper(contractEditorTotalCents.value))

const statusOptions = computed(() => props.module.fields.find((field) => field.key === props.module.statusField)?.options ?? [])
const lookupMap: Record<string, string> = { customer_id: 'customers', material_id: 'materials', sample_id: 'samples', supplier_id: 'suppliers', order_id: 'orders', statement_id: 'statements' }
const lookupLabelMap: Record<string, string> = { customer_id: 'name', material_id: 'code', sample_id: 'code', supplier_id: 'name', order_id: 'no', statement_id: 'no' }

function lookupEntity(key: string) { return lookupMap[key] }
function isLookupField(key: string) { return Boolean(lookupEntity(key)) }
function lookupOptions(key: string) {
  const labelKey = lookupLabelMap[key] || 'name'
  return (lookupRows.value[key] || []).map((row) => ({ label: `${String(row[labelKey] ?? '')} · ID ${row.id}`, value: row.id }))
}

async function loadLookups() {
  const keys = [...new Set(props.module.fields.map((field) => field.key).filter(isLookupField))]
  const entries = await Promise.all(keys.map(async (key) => [key, await command<EntityRecord[]>('list_entities', { entity: lookupEntity(key), search: '', status: null })] as const))
  lookupRows.value = Object.fromEntries(entries)
}

const columns = computed<DataTableColumns<EntityRecord>>(() => [
  ...props.module.columns.map((column) => ({
    key: column.key,
    title: column.label,
    width: column.width,
    ellipsis: { tooltip: true },
    sorter: (a: EntityRecord, b: EntityRecord) => String(a[column.key] ?? '').localeCompare(String(b[column.key] ?? ''), 'zh-CN', { numeric: true }),
    render: (row: EntityRecord) => renderCell(row, column.key, column.kind),
  })),
  {
    key: 'actions', title: '操作', width: attachmentSupported.value ? (props.module.readOnly ? 92 : props.module.key === 'quotations' ? 300 : props.module.detailType ? 250 : props.module.statusField ? 168 : props.module.archiveable === false ? 136 : 128) : (props.module.readOnly ? 54 : props.module.key === 'quotations' ? 266 : props.module.detailType ? 218 : props.module.statusField ? 136 : props.module.archiveable === false ? 48 : 96), fixed: 'right',
    render: (row: EntityRecord) => {
      const actions: ReturnType<typeof h>[] = []
      if (attachmentSupported.value) actions.push(h(NTooltip, null, { trigger: () => h(NButton, { quaternary: true, circle: true, 'aria-label': '附件', onClick: () => openAttachments(row) }, { icon: () => h(Paperclip, { size: 16 }) }), default: () => '附件' }))
      if (!props.module.readOnly) {
        actions.push(h(NTooltip, null, { trigger: () => h(NButton, { quaternary: true, circle: true, 'aria-label': '编辑', onClick: () => openEditor(row) }, { icon: () => h(Pencil, { size: 16 }) }), default: () => '编辑' }))
        if (props.module.detailType) actions.push(h(NTooltip, null, { trigger: () => h(NButton, { quaternary: true, circle: true, 'aria-label': '查看明细', onClick: () => openDetails(row) }, { icon: () => h(ListPlus, { size: 16 }) }), default: () => '查看明细' }))
        if (props.module.key === 'contracts' || props.module.key === 'quotations') actions.push(h(NTooltip, null, { trigger: () => h(NButton, { quaternary: true, circle: true, 'aria-label': '打印预览', onClick: () => void openPrint(row) }, { icon: () => h(Printer, { size: 16 }) }), default: () => '打印预览' }))
        if (props.module.statusField && props.module.key !== 'statements') actions.push(h(NTooltip, null, { trigger: () => h(NButton, { quaternary: true, circle: true, 'aria-label': '状态流转', onClick: () => openTransition(row) }, { icon: () => h(ArrowRightLeft, { size: 16 }) }), default: () => '状态流转' }))
        if (props.module.archiveable !== false) {
          const options = props.module.key === 'quotations'
            ? [
                { label: '复制为新版本', key: 'duplicate', icon: () => h(Copy, { size: 15 }) },
                { label: '版本对比', key: 'compare', icon: () => h(GitCompareArrows, { size: 15 }) },
                { label: '转为订单', key: 'convert', icon: () => h(ShoppingCart, { size: 15 }) },
                { type: 'divider', key: 'divider' },
                { label: '归档记录', key: 'archive', icon: () => h(Archive, { size: 15 }) },
              ]
            : [{ label: '归档记录', key: 'archive', icon: () => h(Archive, { size: 15 }) }]
          actions.push(h(NDropdown, { options, onSelect: (key: string) => quotationMenuAction(row, key) }, { default: () => h(NButton, { quaternary: true, circle: true, 'aria-label': '更多操作' }, { icon: () => h(MoreHorizontal, { size: 17 }) }) }))
        }
      }
      if (!actions.length) return h('span', { class: 'readonly-mark' }, '只读')
      return h(NSpace, { size: 2, wrap: false }, () => actions)
    },
  },
])

function optionLabel(value: unknown) {
  return statusOptions.value.find((option) => option.value === value)?.label || labelForStatus(value)
}

function renderCell(row: EntityRecord, key: string, kind?: string) {
  const value = row[key]
  if (kind === 'status') return h(StatusBadge, { value, label: optionLabel(value) })
  if (kind === 'money') return h('span', { class: 'numeric-cell money-cell' }, formatMoney(value))
  if (kind === 'number') return h('span', { class: 'numeric-cell' }, key.endsWith('_grams') ? formatQuantity(value) : String(value ?? '—'))
  if (kind === 'date') return h('span', { class: 'date-cell' }, String(value || '—'))
  return String(value ?? '—')
}

async function load() {
  loading.value = true
  try {
    const [result] = await Promise.all([
      command<EntityPageResult>('list_entities_page', { entity: props.module.key, search: search.value, status: statusFilter.value, page: page.value, pageSize: pageSize.value }),
      loadLookups(),
    ])
    rows.value = result.rows
    total.value = result.total
    page.value = result.page
    pageSize.value = result.page_size
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
  } finally {
    loading.value = false
  }
}

function fieldToInput(fieldKey: string, value: unknown) {
  if (value == null) return null
  if (fieldKey.endsWith('_cents')) return Number(value) / 100
  if (fieldKey.endsWith('_grams')) return Number(value) / 1000
  return value
}

function inputToStorage(fieldKey: string, value: unknown) {
  if (value == null || value === '') return null
  if (fieldKey.endsWith('_cents')) return Math.round(Number(value) * 100)
  if (fieldKey.endsWith('_grams')) return Math.round(Number(value) * 1000)
  return value
}

function draftSnapshot(value: EntityRecord) {
  return JSON.stringify(Object.fromEntries(Object.keys(value).sort().map((key) => [key, value[key]])))
}

const draftId = `entity-editor-${props.module.key}`

function editorIsDirty() {
  return editorOpen.value && Boolean(editorBaseline.value) && draftSnapshot(editing.value) !== editorBaseline.value
}

function closeEditorWithoutSave() {
  draftCloseOpen.value = false
  editorOpen.value = false
  editorBaseline.value = ''
  unregisterDraft(draftId)
}

function registerEditorDraft() {
  registerDraft({
    id: draftId,
    label: `${props.module.label}编辑`,
    isDirty: editorIsDirty,
    save: async () => save(),
    discard: closeEditorWithoutSave,
  })
}

function openEditor(row?: EntityRecord) {
  if (props.module.readOnly) return
  const source = row ?? { id: 0 }
  editing.value = { id: Number(source.id ?? 0) }
  for (const field of props.module.fields) {
    const value = source[field.key] ?? (!row ? field.defaultValue : undefined)
    editing.value[field.key] = fieldToInput(field.key, value)
  }
  if (props.module.key === 'contracts') editing.value.item_total_cents = Number(source.item_total_cents ?? 0)
  if (!row && props.module.statusField) {
    const field = props.module.fields.find((candidate) => candidate.key === props.module.statusField)
    editing.value[props.module.statusField] = field?.options?.[0]?.value ?? null
  }
  if (!row) {
    for (const field of props.module.fields) {
      if (field.type === 'date' && field.required && !editing.value[field.key]) editing.value[field.key] = new Date().toISOString().slice(0, 10)
    }
  }
  editorBaseline.value = draftSnapshot(editing.value)
  editorOpen.value = true
  registerEditorDraft()
}

function requestEditorClose() {
  if (!editorIsDirty()) {
    closeEditorWithoutSave()
    return
  }
  draftCloseOpen.value = true
}

async function saveDraftAndClose() {
  if (editorSaving.value) return
  const saved = await save()
  if (saved) draftCloseOpen.value = false
}

function discardDraftAndClose() {
  closeEditorWithoutSave()
}

function openDetails(row: EntityRecord) {
  if (!props.module.detailType) return
  detailRecord.value = row
  detailOpen.value = true
}

async function openPrint(row: EntityRecord) {
  const documentType = props.module.detailType
  if (!documentType || !['contracts', 'quotations'].includes(documentType)) return
  printLoading.value = true
  try {
    printRecord.value = row
    printItems.value = await command<DocumentItemRecord[]>('list_document_items', { document_type: documentType, document_id: row.id })
    printOpen.value = true
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
  } finally {
    printLoading.value = false
  }
}

function attachmentName(row: AttachmentRecord) {
  if (row.file_name) return row.file_name
  return String(row.relative_path ?? '').split(/[\\/]/).pop() || '附件文件'
}

function attachmentSize(size: unknown) {
  const value = Number(size ?? 0)
  if (!Number.isFinite(value) || value <= 0) return '大小未知'
  if (value < 1024) return `${value} B`
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`
  return `${(value / (1024 * 1024)).toFixed(1)} MB`
}

function attachmentIntegrity(row: AttachmentRecord) {
  return ({ verified: '校验通过', missing: '文件缺失', checksum_mismatch: '校验不匹配', locked: '保护模式已锁定', unreadable: '无法读取', invalid_path: '路径无效', demo_only: '演示索引' } as Record<string, string>)[row.integrity] || row.integrity || '未检查'
}

function attachmentTargetText() {
  const target = attachmentTarget.value
  if (!target) return ''
  return String(target[props.module.primaryField] ?? target.id)
}

async function loadAttachments() {
  const target = attachmentTarget.value
  if (!target) return
  attachmentLoading.value = true
  try {
    attachmentRows.value = await command<AttachmentRecord[]>('list_attachments', { objectType: props.module.key, objectId: Number(target.id) })
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
  } finally {
    attachmentLoading.value = false
  }
}

async function openAttachments(row: EntityRecord) {
  attachmentTarget.value = row
  attachmentRows.value = []
  attachmentOpen.value = true
  await loadAttachments()
}

async function uploadAttachment(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  const target = attachmentTarget.value
  if (!file || !target) return
  try {
    if (file.size <= 0) throw new Error('附件不能为空')
    if (file.size > attachmentMaxBytes) throw new Error('附件大小不能超过 25 MB')
    attachmentBusy.value = true
    const bytes = Array.from(new Uint8Array(await file.arrayBuffer()))
    const result = await command<AttachmentRecord>('save_attachment', { objectType: props.module.key, objectId: Number(target.id), fileName: file.name, bytes })
    message.success(result.deduplicated ? '相同附件已存在，未重复复制' : result.repaired ? '原失效附件已修复并完成校验' : result.integrity === 'demo_only' ? '演示模式已保存附件索引（未写入本地文件）' : '附件已复制并完成校验')
    await loadAttachments()
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
  } finally {
    attachmentBusy.value = false
    input.value = ''
  }
}

async function downloadAttachment(row: AttachmentRecord) {
  try {
    const result = await command<{ file_name?: string; bytes: number[] }>('download_attachment', { attachmentId: row.id })
    const blob = new Blob([new Uint8Array(result.bytes)], { type: 'application/octet-stream' })
    const url = URL.createObjectURL(blob)
    const link = document.createElement('a')
    link.href = url
    link.download = result.file_name || attachmentName(row)
    document.body.appendChild(link)
    link.click()
    window.setTimeout(() => {
      URL.revokeObjectURL(url)
      link.remove()
    }, 1000)
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
  }
}

async function downloadTemplate() {
  try {
    const result = await command<{ path: string; note_path?: string }>('download_template', { entity: props.module.key })
    message.success(`模板已生成：${result.path}`)
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
  }
}

async function save(): Promise<boolean> {
  if (editorSaving.value) return false
  for (const field of props.module.fields) {
    if (field.required && (editing.value[field.key] == null || editing.value[field.key] === '')) {
      message.warning(`请填写${field.label}`)
      return false
    }
  }
  const payload: EntityRecord = { id: editing.value.id }
  for (const field of props.module.fields) payload[field.key] = inputToStorage(field.key, editing.value[field.key])
  const wasUpdate = Boolean(editing.value.id)
  editorSaving.value = true
  try {
    await command('save_entity', { entity: props.module.key, data: payload })
    closeEditorWithoutSave()
    message.success(wasUpdate ? '记录已更新' : '记录已新增')
    await load()
    return true
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
    return false
  } finally {
    editorSaving.value = false
  }
}

function openTransition(row: EntityRecord) {
  transitionRecord.value = row
  nextStatus.value = null
  transitionReason.value = ''
  transitionBaseline.value = JSON.stringify({ nextStatus: nextStatus.value, transitionReason: transitionReason.value })
  transitionOpen.value = true
  registerTransitionDraft()
}

function transitionIsDirty() {
  return transitionOpen.value && Boolean(transitionBaseline.value) && JSON.stringify({ nextStatus: nextStatus.value, transitionReason: transitionReason.value }) !== transitionBaseline.value
}

const transitionDraftId = `transition-${props.module.key}`

function closeTransitionWithoutSave() {
  transitionCloseOpen.value = false
  transitionOpen.value = false
  transitionBaseline.value = ''
  unregisterDraft(transitionDraftId)
}

function registerTransitionDraft() {
  registerDraft({
    id: transitionDraftId,
    label: `${props.module.label}状态流转`,
    isDirty: transitionIsDirty,
    save: async () => transition(),
    discard: closeTransitionWithoutSave,
  })
}

function requestTransitionClose() {
  if (!transitionIsDirty()) {
    closeTransitionWithoutSave()
    return
  }
  transitionCloseOpen.value = true
}

async function saveTransitionAndClose() {
  if (transitionSaving.value) return
  const saved = await transition()
  if (saved) transitionCloseOpen.value = false
}

function discardTransitionAndClose() {
  closeTransitionWithoutSave()
}

async function transition(): Promise<boolean> {
  if (!transitionRecord.value || !nextStatus.value || transitionSaving.value) return false
  transitionSaving.value = true
  try {
    await command('transition_entity', { entity: props.module.key, id: transitionRecord.value.id, nextStatus: nextStatus.value, reason: transitionReason.value.trim() || null })
    closeTransitionWithoutSave()
    message.success(props.module.key === 'customers' ? '客户阶段已更新并自动生成跟进记录' : '状态已更新并记录审计日志')
    await load()
    return true
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
    return false
  } finally {
    transitionSaving.value = false
  }
}

function archiveRecord(row: EntityRecord) {
  dialog.warning({
    title: '归档记录',
    content: `归档后“${String(row[props.module.primaryField] ?? '')}”不再出现在默认列表中，历史关联仍会保留。`,
    positiveText: '确认归档', negativeText: '取消',
    onPositiveClick: async () => {
      try {
        await command('delete_entity', { entity: props.module.key, id: row.id })
        message.success('记录已归档')
        await load()
      } catch (error) {
        message.error(error instanceof Error ? error.message : String(error))
      }
    },
  })
}

function openQuotationAction(row: EntityRecord, mode: 'compare' | 'convert') {
  quotationActionRecord.value = row
  quotationActionMode.value = mode
  quotationActionOpen.value = true
}

function quotationMenuAction(row: EntityRecord, key: string) {
  if (key === 'archive') {
    archiveRecord(row)
    return
  }
  if (key === 'duplicate') {
    dialog.warning({
      title: '复制报价版本',
      content: `将复制“${String(row.no ?? '')}”的抬头和明细快照，并生成新的草稿版本。`,
      positiveText: '确认复制',
      negativeText: '取消',
      onPositiveClick: async () => {
        try {
          const copy = await command<EntityRecord>('duplicate_quotation', { quotation_id: row.id })
          message.success(`已生成报价版本 ${String(copy.no ?? '')}`)
          await load()
        } catch (error) {
          message.error(error instanceof Error ? error.message : String(error))
        }
      },
    })
    return
  }
  if (key === 'compare') openQuotationAction(row, 'compare')
  if (key === 'convert') openQuotationAction(row, 'convert')
}

async function exportRows() {
  try {
    const result = await command<{ path: string; rows: number; encrypted?: boolean }>('export_entity', { entity: props.module.key, format: 'csv' })
    message.success(`已导出 ${result.rows} 条${result.encrypted ? '（保护模式已加密）' : ''}：${result.path}`)
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
  }
}

async function importFile(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file) return
  try {
    const bytes = Array.from(new Uint8Array(await file.arrayBuffer()))
    const result = await command<{ batch_id?: string; inserted: number; skipped: number; failed: number; errors: string[]; failed_file?: string | null; failed_file_encrypted?: boolean; rolled_back?: boolean; preflight_ok?: boolean; strategy?: ImportStrategy }>('import_file', { entity: props.module.key, fileName: file.name, bytes, strategy: importStrategy.value })
    if (result.failed) {
      importReport.value = result
      importReportOpen.value = true
      message.warning(result.rolled_back ? `导入已回滚：失败 ${result.failed}` : `导入完成：成功 ${result.inserted}，跳过 ${result.skipped}，失败 ${result.failed}`)
    }
    else message.success(`导入完成：成功 ${result.inserted}，跳过 ${result.skipped}`)
    await load()
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
  } finally {
    input.value = ''
  }
}

let searchTimer: ReturnType<typeof setTimeout> | undefined
watch(search, () => {
  clearTimeout(searchTimer)
  page.value = 1
  searchTimer = setTimeout(load, 250)
})
watch(statusFilter, () => { page.value = 1; void load() })
watch(() => props.module.key, async () => {
  search.value = ''
  statusFilter.value = null
  page.value = 1
  await nextTick()
  await load()
})
onMounted(load)
onBeforeUnmount(() => {
  unregisterDraft(draftId)
  unregisterDraft(transitionDraftId)
})
</script>

<template>
  <section class="entity-view">
    <header class="view-heading">
      <div>
        <p class="eyebrow">{{ module.group }}</p>
        <h1>{{ module.label }}</h1>
        <p>共 {{ rows.length }} 条当前记录</p>
      </div>
      <NButton v-if="!module.readOnly" type="primary" @click="openEditor()"><template #icon><Plus :size="17" /></template>新增{{ module.label.replace('管理', '').replace('跟进', '') }}</NButton>
    </header>

    <div class="table-toolbar">
      <NInput v-model:value="search" clearable :placeholder="module.searchPlaceholder" class="search-input">
        <template #prefix><Search :size="16" /></template>
      </NInput>
      <NSelect v-if="statusOptions.length" v-model:value="statusFilter" clearable :options="statusOptions" placeholder="全部状态" class="status-filter" />
      <div class="toolbar-spacer" />
      <input ref="importInput" type="file" accept=".csv" hidden @change="importFile" />
      <input ref="attachmentInput" type="file" hidden @change="uploadAttachment" />
      <NSelect v-if="templateSupported && !module.readOnly" v-model:value="importStrategy" :options="importStrategyOptions" aria-label="导入失败处理策略" class="import-strategy" />
      <NButton v-if="templateSupported && !module.readOnly" secondary @click="importInput?.click()"><template #icon><FileUp :size="17" /></template>导入 CSV</NButton>
      <NButton v-if="templateSupported && !module.readOnly" secondary @click="downloadTemplate"><template #icon><Download :size="17" /></template>下载模板</NButton>
      <NButton secondary @click="exportRows"><template #icon><Download :size="17" /></template>导出 CSV</NButton>
      <NTooltip><template #trigger><NButton quaternary circle aria-label="刷新列表" @click="load"><RefreshCw :size="17" /></NButton></template>刷新列表</NTooltip>
    </div>

    <div class="data-surface">
      <NDataTable
        remote
        :columns="columns"
        :data="rows"
        :loading="loading"
        :row-key="(row: EntityRecord) => row.id"
        :scroll-x="module.columns.reduce((sum, column) => sum + (column.width || 120), 160)"
        :pagination="{ page: page, pageSize: pageSize, itemCount: total, showSizePicker: true, pageSizes: [20, 50, 100], onUpdatePage: (value: number) => { page = value; void load() }, onUpdatePageSize: (value: number) => { pageSize = value; page = 1; void load() } }"
        :bordered="false"
        size="small"
      >
        <template #empty><NEmpty description="暂无符合条件的记录"><template #extra><NButton v-if="!module.readOnly" size="small" type="primary" secondary @click="openEditor()"><template #icon><Plus :size="15" /></template>新增记录</NButton></template></NEmpty></template>
      </NDataTable>
    </div>

    <NModal v-model:show="editorOpen" preset="card" :title="editing.id ? `编辑${module.label}` : `新增${module.label}`" class="record-modal" :mask-closable="false" :closable="false" :close-on-esc="false">
      <NForm label-placement="top" class="record-form">
        <NFormItem v-for="field in module.fields" :key="field.key" :label="field.type === 'checkbox' ? '' : field.label" :required="field.required">
          <NSelect v-if="isLookupField(field.key)" v-model:value="editing[field.key] as number" :options="lookupOptions(field.key)" filterable clearable />
          <NSelect v-else-if="field.type === 'select'" v-model:value="editing[field.key] as string" :options="field.options" :disabled="Boolean(editing.id && field.key === module.statusField)" clearable />
          <NCheckbox v-else-if="field.type === 'checkbox'" v-model:checked="editing[field.key] as boolean">{{ field.placeholder || field.label }}</NCheckbox>
          <NInputNumber v-else-if="field.type === 'number'" v-model:value="editing[field.key] as number" :min="0" :precision="field.key.endsWith('_cents') ? 2 : 3" :placeholder="field.placeholder" style="width: 100%" />
          <NInput v-else v-model:value="editing[field.key] as string" :type="field.type === 'textarea' ? 'textarea' : 'text'" :placeholder="field.placeholder" :autosize="field.type === 'textarea' ? { minRows: 3, maxRows: 6 } : false" />
        </NFormItem>
      </NForm>
      <div v-if="module.key === 'contracts'" class="contract-editor-total" aria-live="polite">
        <div><span>合同总金额（小写）</span><strong>{{ formatMoney(contractEditorTotalCents) }}</strong></div>
        <div><span>合同总金额（大写）</span><strong>{{ contractEditorTotalUpper }}</strong></div>
        <small>金额由已保存合同明细自动汇总</small>
      </div>
      <template #footer><div class="modal-actions"><NButton @click="requestEditorClose">取消</NButton><NButton type="primary" :loading="editorSaving" @click="save">保存</NButton></div></template>
    </NModal>

    <NModal v-model:show="draftCloseOpen" preset="card" title="存在未保存内容" class="draft-modal" :mask-closable="false" :closable="false" :close-on-esc="false">
      <p class="draft-warning">当前{{ module.label }}有未保存的内容，关闭后这些内容将丢失。</p>
      <template #footer>
        <div class="modal-actions">
          <NButton :disabled="editorSaving" @click="draftCloseOpen = false">继续编辑</NButton>
          <NButton type="error" secondary :disabled="editorSaving" @click="discardDraftAndClose">不保存</NButton>
          <NButton type="primary" :loading="editorSaving" @click="saveDraftAndClose">保存</NButton>
        </div>
      </template>
    </NModal>

    <NModal v-model:show="transitionOpen" preset="card" title="状态流转" class="transition-modal" :mask-closable="false" :closable="false" :close-on-esc="false">
      <div v-if="transitionRecord" class="transition-summary">
        <span>当前状态</span>
        <StatusBadge :value="transitionRecord[module.statusField || 'status']" :label="optionLabel(transitionRecord[module.statusField || 'status'])" />
      </div>
      <NForm label-placement="top">
        <NFormItem label="目标状态" required><NSelect v-model:value="nextStatus" :options="statusOptions" placeholder="请选择目标状态" /></NFormItem>
        <NFormItem label="变更原因"><NInput v-model:value="transitionReason" type="textarea" :autosize="{ minRows: 3, maxRows: 5 }" placeholder="回退、失败、取消或暂停时必须填写原因" /></NFormItem>
      </NForm>
      <template #footer><div class="modal-actions"><NButton :disabled="transitionSaving" @click="requestTransitionClose">取消</NButton><NButton type="primary" :disabled="!nextStatus" :loading="transitionSaving" @click="transition">确认流转</NButton></div></template>
    </NModal>

    <NModal v-model:show="transitionCloseOpen" preset="card" title="存在未保存内容" class="draft-modal" :mask-closable="false" :closable="false" :close-on-esc="false">
      <p class="draft-warning">当前状态流转尚未保存，关闭后选择内容将丢失。</p>
      <template #footer>
        <div class="modal-actions">
          <NButton :disabled="transitionSaving" @click="transitionCloseOpen = false">继续编辑</NButton>
          <NButton type="error" secondary :disabled="transitionSaving" @click="discardTransitionAndClose">不保存</NButton>
          <NButton type="primary" :loading="transitionSaving" @click="saveTransitionAndClose">保存</NButton>
        </div>
      </template>
    </NModal>

    <NModal v-model:show="importReportOpen" preset="card" title="导入结果" class="import-report-modal">
      <div v-if="importReport" class="import-report">
        <p>成功 {{ importReport.inserted }} 条，跳过 {{ importReport.skipped }} 条，失败 {{ importReport.failed }} 条。</p>
        <p v-if="importReport.rolled_back" class="import-report-warning">本批次已整体回滚，数据库未保留成功行。</p>
        <p v-if="importReport.failed_file" class="import-report-file">失败行文件：<code>{{ importReport.failed_file }}</code><span v-if="importReport.failed_file_encrypted">（保护模式已加密）</span></p>
        <ul v-if="importReport.errors.length">
          <li v-for="error in importReport.errors" :key="error">{{ error }}</li>
        </ul>
      </div>
      <template #footer><div class="modal-actions"><NButton type="primary" @click="importReportOpen = false">关闭</NButton></div></template>
    </NModal>

    <NModal v-model:show="attachmentOpen" preset="card" title="附件" class="attachment-modal" :mask-closable="false">
      <div class="attachment-heading">
        <div><strong>{{ attachmentTargetText() }}</strong><span>关联 {{ module.label }} · 文件上限 25 MB</span></div>
        <NButton type="primary" secondary :loading="attachmentBusy" @click="attachmentInput?.click()"><template #icon><Upload :size="16" /></template>上传附件</NButton>
      </div>
      <p class="attachment-notice">附件会复制到便携目录的 files/ 下，并登记 SHA-256；保护模式开启时文件以加密形式保存。</p>
      <div v-if="attachmentLoading" class="attachment-empty">正在读取附件…</div>
      <div v-else-if="!attachmentRows.length" class="attachment-empty">暂无附件</div>
      <div v-else class="attachment-list">
        <div v-for="row in attachmentRows" :key="row.id" class="attachment-row">
          <Paperclip :size="17" class="attachment-icon" />
          <div class="attachment-copy"><strong>{{ attachmentName(row) }}</strong><span>{{ attachmentSize(row.size_bytes) }} · {{ attachmentIntegrity(row) }} · {{ row.created_at }}</span><small>{{ row.relative_path }}</small><code>{{ row.hash }}</code></div>
          <NButton v-if="row.integrity === 'verified'" quaternary circle aria-label="下载附件" @click="downloadAttachment(row)"><Download :size="16" /></NButton>
        </div>
      </div>
      <template #footer><div class="modal-actions"><NButton @click="attachmentOpen = false">关闭</NButton></div></template>
    </NModal>

    <DocumentItemsModal
      v-if="module.detailType"
      v-model:show="detailOpen"
      :document-type="module.detailType"
      :document="detailRecord"
      @saved="load"
    />
    <PrintPreviewModal
      v-if="printRecord && module.detailType && (module.key === 'contracts' || module.key === 'quotations')"
      v-model:show="printOpen"
      :document-type="module.detailType"
      :document="printRecord"
      :items="printItems"
    />
    <QuotationActionsModal
      v-if="module.key === 'quotations' && quotationActionRecord"
      v-model:show="quotationActionOpen"
      :mode="quotationActionMode"
      :quotation="quotationActionRecord"
      @completed="load"
    />
  </section>
</template>
