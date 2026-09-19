<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { GitCompareArrows, ShoppingCart } from 'lucide-vue-next'
import { NAlert, NButton, NDataTable, NForm, NFormItem, NInput, NModal, NSelect, useMessage, type DataTableColumns } from 'naive-ui'
import { command } from '../api'
import type { EntityRecord } from '../types'

type ActionMode = 'compare' | 'convert'

interface HeaderDifference {
  field: string
  label: string
  left: unknown
  right: unknown
}

interface ItemDifference {
  index: number
  change: string
  left: Record<string, unknown> | null
  right: Record<string, unknown> | null
}

interface ComparisonResult {
  left: { id: number; no: string; version: number }
  right: { id: number; no: string; version: number }
  header_differences: HeaderDifference[]
  item_differences: ItemDifference[]
  same: boolean
}

const props = defineProps<{
  show: boolean
  mode: ActionMode
  quotation: EntityRecord | null
}>()

const emit = defineEmits<{
  (event: 'update:show', value: boolean): void
  (event: 'completed'): void
}>()

const message = useMessage()
const loading = ref(false)
const candidates = ref<EntityRecord[]>([])
const rightId = ref<number | null>(null)
const orderNo = ref('')
const deliveryDate = ref('')
const comparison = ref<ComparisonResult | null>(null)

const title = computed(() => props.mode === 'compare' ? '报价版本对比' : '报价转订单')
const candidateOptions = computed(() => candidates.value
  .filter((row) => Number(row.id) !== Number(props.quotation?.id))
  .map((row) => ({ label: `${String(row.no ?? '')} · V${String(row.version ?? 1)} · ${String(row.recipient_name ?? row.customer_name ?? '')}`, value: Number(row.id) })))
const headerColumns: DataTableColumns<HeaderDifference> = [
  { key: 'label', title: '字段', width: 120 },
  { key: 'left', title: '左版本', render: (row) => formatValue(row.left) },
  { key: 'right', title: '右版本', render: (row) => formatValue(row.right) },
]
const itemColumns: DataTableColumns<ItemDifference> = [
  { key: 'index', title: '明细行', width: 80 },
  { key: 'change', title: '变化', width: 90, render: (row) => ({ added: '新增', removed: '删除', changed: '修改' }[row.change] || row.change) },
  { key: 'left', title: '左版本', render: (row) => formatItem(row.left) },
  { key: 'right', title: '右版本', render: (row) => formatItem(row.right) },
]

function formatValue(value: unknown) {
  if (value == null || value === '') return '—'
  if (typeof value === 'boolean') return value ? '是' : '否'
  return String(value)
}

function formatItem(value: Record<string, unknown> | null) {
  if (!value) return '—'
  const material = String(value.material_grade ?? value.material_category ?? value.material_id ?? '')
  const quantity = Number(value.qty_grams ?? 0) / 1000
  const price = Number(value.unit_price_cents ?? 0) / 100
  const note = String(value.note ?? '')
  return `${material || '未命名'} · ${quantity.toLocaleString('zh-CN', { maximumFractionDigits: 3 })} kg · ${price.toFixed(2)} 元/kg${note ? ` · ${note}` : ''}`
}

async function loadCandidates() {
  if (!props.show || props.mode !== 'compare' || !props.quotation?.id) return
  loading.value = true
  try {
    candidates.value = await command<EntityRecord[]>('list_entities', { entity: 'quotations', search: '', status: null })
    rightId.value = candidateOptions.value[0]?.value ?? null
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
  } finally {
    loading.value = false
  }
}

async function compare() {
  if (!props.quotation?.id || !rightId.value) {
    message.warning('请选择要对比的报价版本')
    return
  }
  loading.value = true
  try {
    comparison.value = await command<ComparisonResult>('compare_quotations', { left_id: props.quotation.id, right_id: rightId.value })
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
  } finally {
    loading.value = false
  }
}

async function convert() {
  if (!props.quotation?.id || loading.value) return
  loading.value = true
  try {
    await command('convert_quotation_to_order', { quotation_id: props.quotation.id, order_no: orderNo.value.trim(), delivery_date: deliveryDate.value || null })
    message.success('报价已转为待确认订单')
    emit('completed')
    emit('update:show', false)
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
  } finally {
    loading.value = false
  }
}

function close() {
  if (loading.value) return
  emit('update:show', false)
}

watch(() => [props.show, props.mode, props.quotation?.id] as const, ([show]) => {
  if (!show) return
  comparison.value = null
  orderNo.value = ''
  deliveryDate.value = ''
  rightId.value = null
  void loadCandidates()
}, { immediate: true })
</script>

<template>
  <NModal :show="show" preset="card" :title="title" class="quotation-action-modal" :mask-closable="false" :closable="false" @update:show="(value) => value ? emit('update:show', true) : close()">
    <template v-if="mode === 'convert'">
      <NAlert type="info" :bordered="false" class="detail-alert">将复制当前报价的全部明细快照，生成“待确认”订单；订单号留空时自动生成。</NAlert>
      <NForm label-placement="top">
        <NFormItem label="来源报价"><NInput :value="`${quotation?.no ?? ''} · V${quotation?.version ?? 1}`" readonly /></NFormItem>
        <NFormItem label="订单号"><NInput v-model:value="orderNo" placeholder="可选，例如 SO-20260918-001" /></NFormItem>
        <NFormItem label="交期"><NInput v-model:value="deliveryDate" placeholder="YYYY-MM-DD，可选" /></NFormItem>
      </NForm>
    </template>
    <template v-else>
      <NForm label-placement="top">
        <NFormItem label="左版本"><NInput :value="`${quotation?.no ?? ''} · V${quotation?.version ?? 1}`" readonly /></NFormItem>
        <NFormItem label="右版本" required><NSelect v-model:value="rightId" :options="candidateOptions" filterable clearable placeholder="选择另一份报价版本" /></NFormItem>
      </NForm>
      <NButton type="primary" secondary :loading="loading" :disabled="!rightId" @click="compare"><template #icon><GitCompareArrows :size="16" /></template>开始对比</NButton>
      <div v-if="comparison" class="quotation-comparison">
        <NAlert :type="comparison.same ? 'success' : 'warning'" :bordered="false">{{ comparison.same ? '两份报价的抬头和明细完全一致。' : `发现 ${comparison.header_differences.length + comparison.item_differences.length} 处差异。` }}</NAlert>
        <NDataTable v-if="comparison.header_differences.length" :columns="headerColumns" :data="comparison.header_differences" :bordered="false" size="small" />
        <NDataTable v-if="comparison.item_differences.length" :columns="itemColumns" :data="comparison.item_differences" :bordered="false" size="small" />
      </div>
    </template>
    <template #footer>
      <div class="modal-actions">
        <NButton @click="close">关闭</NButton>
        <NButton v-if="mode === 'convert'" type="primary" :loading="loading" @click="convert"><template #icon><ShoppingCart :size="16" /></template>生成订单</NButton>
      </div>
    </template>
  </NModal>
</template>
