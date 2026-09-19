<script setup lang="ts">
import { computed, nextTick, watch } from 'vue'
import { Printer } from 'lucide-vue-next'
import { NButton, NModal } from 'naive-ui'
import type { DocumentItemRecord, DocumentType, EntityRecord } from '../types'
import { buildContractPrintHtml, buildQuotationPrintHtml } from '../print'

const props = defineProps<{ show: boolean; documentType: DocumentType; document: EntityRecord | null; items: DocumentItemRecord[] }>()
const emit = defineEmits<{ (event: 'update:show', value: boolean): void }>()

const html = computed(() => {
  if (!props.document) return ''
  return props.documentType === 'contracts' ? buildContractPrintHtml(props.document, props.items) : buildQuotationPrintHtml(props.document, props.items)
})

async function printDocument() {
  await nextTick()
  window.print()
}

watch(() => props.show, (show) => {
  if (show) void nextTick()
})
</script>

<template>
  <NModal :show="show" preset="card" :title="documentType === 'contracts' ? '合同打印预览' : '报价单打印预览'" class="print-preview-modal" :mask-closable="false" @update:show="emit('update:show', $event)">
    <div class="print-preview-toolbar"><span>A4 · 已保存业务快照</span><NButton type="primary" @click="printDocument"><template #icon><Printer :size="16" /></template>打印 / 另存为 PDF</NButton></div>
    <div class="print-preview-canvas" v-html="html" />
    <template #footer><div class="modal-actions"><NButton @click="emit('update:show', false)">关闭</NButton><NButton type="primary" @click="printDocument"><template #icon><Printer :size="16" /></template>打印</NButton></div></template>
  </NModal>
</template>
