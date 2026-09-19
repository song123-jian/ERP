<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { Plus, Save, Trash2 } from 'lucide-vue-next'
import { NAlert, NButton, NEmpty, NFormItem, NInput, NInputNumber, NModal, NSelect, NSpin, NTooltip, useMessage } from 'naive-ui'
import { command } from '../api'
import { registerDraft, unregisterDraft } from '../drafts'
import { amountToChineseUpper, contractTotalCents } from '../print'
import { formatMoney, formatQuantity } from '../theme'
import type { DocumentItemRecord, DocumentType, EntityRecord } from '../types'

interface DetailLine {
  id?: number
  material_id: number | null
  order_item_id: number | null
  order_id: number | null
  statement_id: number | null
  batch: string
  qty_kg: number | null
  unit_price_yuan: number | null
  allocated_yuan: number | null
  material_name: string
  model: string
  manufacturer: string
  material_category: string
  material_grade: string
  note: string
}

const props = defineProps<{
  show: boolean
  documentType: DocumentType
  document: EntityRecord | null
}>()

const emit = defineEmits<{
  (event: 'update:show', value: boolean): void
  (event: 'saved'): void
}>()

const message = useMessage()
const loading = ref(false)
const saving = ref(false)
const draftCloseOpen = ref(false)
const draftBaseline = ref('')
let registeredDraftId = ''
const lines = ref<DetailLine[]>([])
const materials = ref<EntityRecord[]>([])
const orderItems = ref<EntityRecord[]>([])
const orders = ref<EntityRecord[]>([])
const statements = ref<EntityRecord[]>([])

const isMaterialDocument = computed(() => props.documentType === 'quotations' || props.documentType === 'orders')
const isContract = computed(() => props.documentType === 'contracts')
const isQuote = computed(() => props.documentType === 'quotations')
const isDelivery = computed(() => props.documentType === 'deliveries')
const isStatement = computed(() => props.documentType === 'statements')
const isPayment = computed(() => props.documentType === 'payments')
const documentNo = computed(() => String(props.document?.no ?? props.document?.statement_no ?? `#${props.document?.id ?? ''}`))
const modalTitle = computed(() => `${isPayment.value ? '回款分配' : isContract.value ? '合同明细' : '单据明细'} · ${documentNo.value}`)
const totalCents = computed(() => {
  if (isContract.value) return contractTotalCents(lines.value.map((line) => ({ amount_cents: lineAmountCents(line) })))
  return lines.value.reduce((sum, line) => {
    if (isMaterialDocument.value) return sum + lineAmountCents(line)
    if (isStatement.value || isPayment.value) return sum + Math.round(Number(line.allocated_yuan ?? 0) * 100)
    return sum
  }, 0)
})
const totalChineseUpper = computed(() => amountToChineseUpper(totalCents.value))

const materialOptions = computed(() => materials.value.map((row) => ({ label: `${String(row.code ?? '')} · ID ${row.id}`, value: Number(row.id) })))
const documentOrderId = computed(() => Number(props.document?.order_id ?? 0))
const orderItemOptions = computed(() => orderItems.value
  .filter((row) => !documentOrderId.value || Number(row.order_id) === documentOrderId.value)
  .map((row) => ({ label: `${String(row.order_no ?? '')} · ${String(row.material_code ?? '')} · ${formatQuantity(row.qty_grams)}`, value: Number(row.id) })))
const eligibleOrderOptions = computed(() => {
  const customerId = Number(props.document?.customer_id ?? 0)
  const start = String(props.document?.period_start ?? '')
  const end = String(props.document?.period_end ?? '')
  return orders.value
    .filter((row) => Number(row.customer_id) === customerId)
    .filter((row) => {
      const date = String(row.delivery_date ?? '')
      return !date || (!start || date >= start) && (!end || date <= end)
    })
    .map((row) => ({ label: `${String(row.no ?? '')} · ${String(row.material_code ?? '')} · ${formatMoney(row.amount_cents)}`, value: Number(row.id) }))
})
const statementOptions = computed(() => statements.value.map((row) => ({ label: `${String(row.no ?? '')} · ${String(row.customer_name ?? '')} · 未收 ${formatMoney(row.unpaid_cents)}`, value: Number(row.id) })))

function lineAmountCents(line: DetailLine) {
  return Math.round(Number(line.qty_kg ?? 0) * Number(line.unit_price_yuan ?? 0) * 100)
}

function emptyLine(): DetailLine {
  return { material_id: null, order_item_id: null, order_id: null, statement_id: null, batch: '', qty_kg: null, unit_price_yuan: null, allocated_yuan: null, material_name: '', model: '', manufacturer: '', material_category: '', material_grade: '', note: '' }
}

function draftSnapshot() {
  return JSON.stringify(lines.value)
}

const draftId = computed(() => `document-items-${props.documentType}-${Number(props.document?.id ?? 0)}`)

function detailIsDirty() {
  return props.show && Boolean(draftBaseline.value) && draftSnapshot() !== draftBaseline.value
}

function unregisterDetailDraft() {
  if (registeredDraftId) unregisterDraft(registeredDraftId)
  registeredDraftId = ''
}

function registerDetailDraft() {
  unregisterDetailDraft()
  registeredDraftId = draftId.value
  registerDraft({
    id: registeredDraftId,
    label: `${isPayment.value ? '回款分配' : '单据明细'}编辑`,
    isDirty: detailIsDirty,
    save: async () => save(false),
    discard: closeWithoutSave,
  })
}

function addLine() {
  lines.value.push(emptyLine())
}

function removeLine(index: number) {
  lines.value.splice(index, 1)
}

function materialSelected(line: DetailLine) {
  const material = materials.value.find((row) => Number(row.id) === Number(line.material_id))
  if (material && line.unit_price_yuan == null && Number(material.cost_cents) > 0) line.unit_price_yuan = Number(material.cost_cents) / 100
}

function orderSelected(line: DetailLine) {
  if (!isStatement.value || line.allocated_yuan != null) return
  const order = orders.value.find((row) => Number(row.id) === Number(line.order_id))
  if (order) line.allocated_yuan = Number(order.amount_cents ?? 0) / 100
}

function statementSelected(line: DetailLine) {
  if (!isPayment.value || line.allocated_yuan != null) return
  const statement = statements.value.find((row) => Number(row.id) === Number(line.statement_id))
  if (statement) line.allocated_yuan = Math.min(Number(statement.unpaid_cents ?? 0), Number(props.document?.amount_cents ?? 0)) / 100
}

function toLine(raw: DocumentItemRecord): DetailLine {
  return {
    id: raw.id,
    material_id: raw.material_id == null ? null : Number(raw.material_id),
    order_item_id: raw.order_item_id == null ? null : Number(raw.order_item_id),
    order_id: raw.order_id == null ? null : Number(raw.order_id),
    statement_id: raw.statement_id == null ? null : Number(raw.statement_id),
    batch: String(raw.batch ?? ''),
    qty_kg: raw.qty_grams == null ? null : Number(raw.qty_grams) / 1000,
    unit_price_yuan: raw.unit_price_cents == null ? null : Number(raw.unit_price_cents) / 100,
    allocated_yuan: raw.allocated_amount_cents == null ? null : Number(raw.allocated_amount_cents) / 100,
    material_name: String(raw.material_name ?? ''),
    model: String(raw.model ?? ''),
    manufacturer: String(raw.manufacturer ?? ''),
    material_category: String(raw.material_category ?? ''),
    material_grade: String(raw.material_grade ?? ''),
    note: String(raw.note ?? ''),
  }
}

async function loadLookups() {
  const requests: Promise<void>[] = []
  if (isMaterialDocument.value || isContract.value) requests.push(command<EntityRecord[]>('list_entities', { entity: 'materials', search: '', status: null }).then((rows) => { materials.value = rows }))
  if (isDelivery.value) requests.push(command<EntityRecord[]>('list_entities', { entity: 'order_items', search: '', status: null }).then((rows) => { orderItems.value = rows }))
  if (isStatement.value) requests.push(command<EntityRecord[]>('list_entities', { entity: 'orders', search: '', status: null }).then((rows) => { orders.value = rows }))
  if (isPayment.value) requests.push(command<EntityRecord[]>('list_entities', { entity: 'statements', search: '', status: null }).then((rows) => { statements.value = rows }))
  await Promise.all(requests)
}

async function load() {
  if (!props.show || !props.document?.id) return
  unregisterDetailDraft()
  draftBaseline.value = ''
  loading.value = true
  try {
    await loadLookups()
    const raw = await command<DocumentItemRecord[]>('list_document_items', { document_type: props.documentType, document_id: props.document.id })
    lines.value = raw.map(toLine)
    if (!lines.value.length && (isMaterialDocument.value || isContract.value)) {
      const line = emptyLine()
      if (!isContract.value) {
        line.material_id = props.document.material_id == null ? null : Number(props.document.material_id)
        line.qty_kg = Number(props.documentType === 'quotations' ? props.document.moq_grams : props.document.qty_grams) / 1000 || null
        line.unit_price_yuan = Number(props.document.price_cents ?? 0) / 100 || null
      }
      lines.value = [line]
    }
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
  } finally {
    loading.value = false
    if (props.show && props.document?.id) {
      draftBaseline.value = draftSnapshot()
      registerDetailDraft()
    }
  }
}

function buildPayload(): DocumentItemRecord[] | null {
  if (!lines.value.length) {
    message.warning('至少需要一条明细')
    return null
  }
  if (isMaterialDocument.value) {
    const payload: DocumentItemRecord[] = []
    for (const line of lines.value) {
      if (!line.material_id || Number(line.qty_kg ?? 0) <= 0 || Number(line.unit_price_yuan ?? 0) <= 0) {
        message.warning('请完整填写牌号、数量和单价')
        return null
      }
      payload.push({ material_id: line.material_id, batch: line.batch.trim(), material_category: line.material_category.trim(), material_grade: line.material_grade.trim(), manufacturer: line.manufacturer.trim(), note: line.note.trim(), qty_grams: Math.round(Number(line.qty_kg) * 1000), unit_price_cents: Math.round(Number(line.unit_price_yuan) * 100) })
    }
    return payload
  }
  if (isContract.value) {
    const payload: DocumentItemRecord[] = []
    for (const line of lines.value) {
      if (!line.material_name.trim() || !line.model.trim() || Number(line.qty_kg ?? 0) <= 0 || Number(line.unit_price_yuan ?? 0) <= 0) {
        message.warning('请完整填写材料名称、型号、数量和含税单价')
        return null
      }
      payload.push({ material_id: line.material_id ?? undefined, material_name: line.material_name.trim(), model: line.model.trim(), manufacturer: line.manufacturer.trim(), note: line.note.trim(), qty_grams: Math.round(Number(line.qty_kg) * 1000), unit_price_cents: Math.round(Number(line.unit_price_yuan) * 100) })
    }
    return payload
  }
  if (isDelivery.value) {
    const payload: DocumentItemRecord[] = []
    for (const line of lines.value) {
      if (!line.order_item_id || Number(line.qty_kg ?? 0) <= 0) {
        message.warning('请完整填写订单明细和送货数量')
        return null
      }
      payload.push({ order_item_id: line.order_item_id, batch: line.batch.trim(), qty_grams: Math.round(Number(line.qty_kg) * 1000) })
    }
    return payload
  }
  if (isStatement.value) {
    const payload: DocumentItemRecord[] = []
    for (const line of lines.value) {
      if (!line.order_id || Number(line.allocated_yuan ?? 0) <= 0) {
        message.warning('请完整填写订单和分配金额')
        return null
      }
      payload.push({ order_id: line.order_id, allocated_amount_cents: Math.round(Number(line.allocated_yuan) * 100) })
    }
    return payload
  }
  const payload: DocumentItemRecord[] = []
  for (const line of lines.value) {
    if (!line.statement_id || Number(line.allocated_yuan ?? 0) <= 0) {
      message.warning('请完整填写对账单和分配金额')
      return null
    }
    payload.push({ statement_id: line.statement_id, allocated_amount_cents: Math.round(Number(line.allocated_yuan) * 100) })
  }
  return payload
}

async function save(refreshParent = true): Promise<boolean> {
  if (!props.document?.id || saving.value) return false
  const payload = buildPayload()
  if (!payload) return false
  if (isPayment.value && Math.round(totalCents.value) !== Number(props.document.amount_cents ?? 0)) {
    message.warning(`分配合计必须等于回款金额：${formatMoney(props.document.amount_cents)}`)
    return false
  }
  saving.value = true
  try {
    if (isPayment.value) await command('save_payment_allocations', { payment_id: props.document.id, allocations: payload })
    else await command('save_document_items', { document_type: props.documentType, document_id: props.document.id, items: payload })
    message.success('明细已保存')
    if (refreshParent) emit('saved')
    closeWithoutSave()
    return true
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
    return false
  } finally {
    saving.value = false
  }
}

function closeWithoutSave() {
  draftCloseOpen.value = false
  draftBaseline.value = ''
  unregisterDetailDraft()
  emit('update:show', false)
}

function requestClose() {
  if (!detailIsDirty()) {
    closeWithoutSave()
    return
  }
  draftCloseOpen.value = true
}

function handleModalVisibility(value: boolean) {
  // The parent owns `show`; only user initiated close requests need handling.
  if (!value && props.show) requestClose()
}

async function saveDraftAndClose() {
  const saved = await save()
  if (saved) draftCloseOpen.value = false
}

function discardDraftAndClose() {
  closeWithoutSave()
}

watch(() => [props.show, props.documentType, props.document?.id] as const, ([show]) => {
  if (show) void load()
  else unregisterDetailDraft()
})
onBeforeUnmount(unregisterDetailDraft)
</script>

<template>
  <NModal :show="show" preset="card" :title="modalTitle" class="detail-modal" :mask-closable="false" :closable="false" :close-on-esc="false" @update:show="handleModalVisibility">
    <NSpin :show="loading">
      <div v-if="isContract" class="detail-summary contract-total-summary">
        <div class="contract-total-cell"><span>合同总金额（小写）</span><strong>{{ formatMoney(totalCents) }}</strong></div>
        <div class="contract-total-cell"><span>合同总金额（大写）</span><strong class="contract-total-upper">{{ totalChineseUpper }}</strong></div>
      </div>
      <div v-else class="detail-summary">
        <span>{{ isMaterialDocument ? '明细金额' : isDelivery ? '送货数量' : '分配合计' }}</span>
        <strong>{{ isDelivery ? formatQuantity(lines.reduce((sum, line) => sum + Number(line.qty_kg ?? 0) * 1000, 0)) : formatMoney(totalCents) }}</strong>
      </div>
      <NAlert v-if="isPayment" type="info" :bordered="false" class="detail-alert">回款金额 {{ formatMoney(document?.amount_cents) }}，分配合计必须完全相等。</NAlert>
      <NAlert v-else-if="isDelivery" type="info" :bordered="false" class="detail-alert">送货数量只能来自当前订单明细，且累计送货不能超过订单数量。</NAlert>
      <NAlert v-else-if="isStatement" type="info" :bordered="false" class="detail-alert">订单必须属于当前客户，交期必须落在对账期间内。</NAlert>
      <div v-if="lines.length" class="detail-lines">
        <div v-for="(line, index) in lines" :key="line.id ?? `new-${index}`" class="detail-line">
          <div class="detail-line-index">{{ index + 1 }}</div>
          <div class="detail-line-fields">
            <template v-if="isContract">
              <NFormItem label="材料名称" required><NInput v-model:value="line.material_name" placeholder="如 PP" /></NFormItem>
              <NFormItem label="型号 / 牌号" required><NInput v-model:value="line.model" placeholder="如 PI0-S27A[BK16452]" /></NFormItem>
              <NFormItem label="厂家"><NInput v-model:value="line.manufacturer" placeholder="可选" /></NFormItem>
              <NFormItem label="关联材料牌号"><NSelect v-model:value="line.material_id" :options="materialOptions" filterable clearable placeholder="可选，保存快照" /></NFormItem>
              <NFormItem label="数量（kg）" required><NInputNumber v-model:value="line.qty_kg" :min="0" :precision="3" style="width: 100%" /></NFormItem>
              <NFormItem label="含税单价（元/kg）" required><NInputNumber v-model:value="line.unit_price_yuan" :min="0" :precision="2" style="width: 100%" /></NFormItem>
              <NFormItem label="备注"><NInput v-model:value="line.note" placeholder="可选" /></NFormItem>
              <div class="detail-line-total">金额 {{ formatMoney(lineAmountCents(line)) }}</div>
            </template>
            <template v-else-if="isMaterialDocument">
              <NFormItem label="牌号" required><NSelect v-model:value="line.material_id" :options="materialOptions" filterable clearable placeholder="选择材料牌号" @update:value="materialSelected(line)" /></NFormItem>
              <NFormItem label="批次"><NInput v-model:value="line.batch" placeholder="可选" /></NFormItem>
              <template v-if="isQuote">
                <NFormItem label="材料类别"><NInput v-model:value="line.material_category" placeholder="如 PP-GF20" /></NFormItem>
                <NFormItem label="报价牌号"><NInput v-model:value="line.material_grade" placeholder="如 PG4-S01A" /></NFormItem>
                <NFormItem label="厂家"><NInput v-model:value="line.manufacturer" placeholder="可选" /></NFormItem>
                <NFormItem label="备注"><NInput v-model:value="line.note" placeholder="如 未税、含运费" /></NFormItem>
              </template>
              <NFormItem label="数量（kg）" required><NInputNumber v-model:value="line.qty_kg" :min="0" :precision="3" style="width: 100%" /></NFormItem>
              <NFormItem :label="isQuote ? '报价单价（元/kg）' : '含税单价（元/kg）'" required><NInputNumber v-model:value="line.unit_price_yuan" :min="0" :precision="2" style="width: 100%" /></NFormItem>
              <div class="detail-line-total">金额 {{ formatMoney(lineAmountCents(line)) }}</div>
            </template>
            <template v-else-if="isDelivery">
              <NFormItem label="订单明细" required><NSelect v-model:value="line.order_item_id" :options="orderItemOptions" filterable clearable placeholder="选择订单明细" /></NFormItem>
              <NFormItem label="批次"><NInput v-model:value="line.batch" placeholder="可选" /></NFormItem>
              <NFormItem label="送货数量（kg）" required><NInputNumber v-model:value="line.qty_kg" :min="0" :precision="3" style="width: 100%" /></NFormItem>
            </template>
            <template v-else-if="isStatement">
              <NFormItem label="订单" required><NSelect v-model:value="line.order_id" :options="eligibleOrderOptions" filterable clearable placeholder="选择订单" @update:value="orderSelected(line)" /></NFormItem>
              <NFormItem label="分配金额（元）" required><NInputNumber v-model:value="line.allocated_yuan" :min="0" :precision="2" style="width: 100%" /></NFormItem>
            </template>
            <template v-else>
              <NFormItem label="对账单" required><NSelect v-model:value="line.statement_id" :options="statementOptions" filterable clearable placeholder="选择对账单" @update:value="statementSelected(line)" /></NFormItem>
              <NFormItem label="分配金额（元）" required><NInputNumber v-model:value="line.allocated_yuan" :min="0" :precision="2" style="width: 100%" /></NFormItem>
            </template>
          </div>
          <NTooltip><template #trigger><NButton quaternary circle aria-label="删除明细" @click="removeLine(index)"><Trash2 :size="16" /></NButton></template>删除明细</NTooltip>
        </div>
      </div>
      <NEmpty v-else description="暂无明细，请添加一行" />
      <NButton secondary class="add-detail-button" @click="addLine"><template #icon><Plus :size="16" /></template>添加明细</NButton>
    </NSpin>
    <template #footer><div class="modal-actions"><NButton :disabled="saving" @click="requestClose">取消</NButton><NButton type="primary" :loading="saving" @click="save()"><template #icon><Save :size="16" /></template>保存明细</NButton></div></template>
  </NModal>

  <NModal v-model:show="draftCloseOpen" preset="card" title="存在未保存明细" class="draft-modal" :mask-closable="false" :closable="false" :close-on-esc="false">
    <p class="draft-warning">当前{{ isPayment ? '回款分配' : '单据明细' }}有未保存的内容，关闭后这些内容将丢失。</p>
    <template #footer>
      <div class="modal-actions">
        <NButton :disabled="saving" @click="draftCloseOpen = false">继续编辑</NButton>
        <NButton type="error" secondary :disabled="saving" @click="discardDraftAndClose">不保存</NButton>
        <NButton type="primary" :loading="saving" @click="saveDraftAndClose">保存</NButton>
      </div>
    </template>
  </NModal>
</template>
