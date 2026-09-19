<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { CircleCheck, CircleHelp, Play, RefreshCw, TriangleAlert } from 'lucide-vue-next'
import { NAlert, NButton, NEmpty, NSelect, NSpin, NTag, useMessage } from 'naive-ui'
import { command } from '../api'
import type { BackupEntry, OperationalAssessment, OperationalMetric, RestoreDrillRecord } from '../types'

const props = defineProps<{
  backups: BackupEntry[]
  protectionEnabled: boolean
}>()

const message = useMessage()
const loading = ref(false)
const drilling = ref(false)
const assessment = ref<OperationalAssessment | null>(null)
const selectedBackupPath = ref('')
const drillError = ref('')
const drillResult = ref<RestoreDrillRecord | null>(null)

const usableBackups = computed(() => props.backups.filter((entry) => entry.exists && entry.result === 'success' && entry.checksum))
const backupOptions = computed(() => usableBackups.value.map((entry) => ({
  label: `${entry.path.split(/[\\/]/).pop() || entry.path} · ${formatDate(entry.created_at)}`,
  value: entry.path,
})))

function formatDate(value: string | null | undefined) {
  if (!value) return '时间未知'
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString('zh-CN', { hour12: false })
}

function formatMetricValue(metric: OperationalMetric) {
  if (metric.value == null || !Number.isFinite(metric.value)) return '未采集'
  if (metric.unit === '%') return `${metric.value.toFixed(2)}%`
  if (metric.unit === '小时') return `${metric.value.toFixed(2)} 小时`
  if (metric.unit === '分钟') return `${metric.value.toFixed(2)} 分钟`
  return `${metric.value.toFixed(2)} ${metric.unit}`
}

function statusLabel(status: string) {
  if (status === 'pass') return '达标'
  if (status === 'fail') return '未达标'
  return '数据不足'
}

function statusType(status: string): 'success' | 'error' | 'warning' | 'default' {
  if (status === 'pass') return 'success'
  if (status === 'fail') return 'error'
  return 'warning'
}

async function loadAssessment() {
  loading.value = true
  try {
    assessment.value = await command<OperationalAssessment>('get_operational_metrics')
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
  } finally {
    loading.value = false
  }
}

function selectDefaultBackup() {
  if (!selectedBackupPath.value || !usableBackups.value.some((entry) => entry.path === selectedBackupPath.value)) {
    selectedBackupPath.value = usableBackups.value[0]?.path || ''
  }
}

async function runDrill() {
  selectDefaultBackup()
  if (!selectedBackupPath.value) {
    drillError.value = '暂无校验通过的备份，请先完成一次备份。'
    return
  }
  drilling.value = true
  drillError.value = ''
  drillResult.value = null
  try {
    drillResult.value = await command<RestoreDrillRecord & { ok?: boolean }>('run_restore_drill', { backup_path: selectedBackupPath.value })
    message.success('恢复演练完成，当前主库未被替换')
    await loadAssessment()
  } catch (error) {
    drillError.value = error instanceof Error ? error.message : String(error)
    await loadAssessment()
  } finally {
    drilling.value = false
  }
}

watch(() => props.backups, () => {
  selectDefaultBackup()
}, { deep: true })

onMounted(() => {
  selectDefaultBackup()
  void loadAssessment()
})
</script>

<template>
  <NSpin :show="loading">
    <div class="assessment-toolbar">
      <div>
        <h3>运营评估</h3>
        <p v-if="assessment">生成时间 {{ formatDate(assessment.generated_at) }} · 统计窗口 {{ assessment.window_start }} 至 {{ assessment.window_end }}</p>
        <p v-else>正在读取指标数据…</p>
      </div>
      <NButton secondary :loading="loading" @click="loadAssessment">
        <template #icon><RefreshCw :size="15" /></template>
        刷新评估
      </NButton>
    </div>

    <div v-if="assessment" class="assessment-grid">
      <section v-for="metric in assessment.metrics" :key="metric.key" class="assessment-metric">
        <div class="assessment-metric-heading">
          <strong>{{ metric.label }}</strong>
          <NTag size="small" :type="statusType(metric.status)">{{ statusLabel(metric.status) }}</NTag>
        </div>
        <div class="assessment-metric-value">{{ formatMetricValue(metric) }}</div>
        <div class="assessment-metric-target">目标 {{ metric.target }} · 样本 {{ metric.sample_size }}</div>
        <small>{{ metric.source }} · {{ metric.window }}</small>
        <p>{{ metric.formula }}</p>
      </section>
    </div>

    <section class="assessment-drill-section">
      <div class="assessment-section-heading">
        <div>
          <h3>恢复演练</h3>
          <p>在临时 SQLite 副本中校验备份、迁移和完整性，不替换当前主库。</p>
        </div>
        <CircleCheck v-if="assessment?.latest_restore_drill?.result === 'success'" class="assessment-section-icon success" :size="20" />
        <TriangleAlert v-else class="assessment-section-icon warning" :size="20" />
      </div>
      <div class="assessment-drill-controls">
        <NSelect v-model:value="selectedBackupPath" :options="backupOptions" clearable filterable placeholder="选择校验通过的备份" />
        <NButton type="primary" :loading="drilling" :disabled="!selectedBackupPath" @click="runDrill">
          <template #icon><Play :size="15" /></template>
          运行恢复演练
        </NButton>
      </div>
      <NAlert v-if="drillError" type="error" :bordered="false" class="assessment-alert">
        <template #icon><TriangleAlert :size="16" /></template>
        {{ drillError }}
      </NAlert>
      <NAlert v-else-if="drillResult" type="success" :bordered="false" class="assessment-alert">
        <template #icon><CircleCheck :size="16" /></template>
        演练通过：耗时 {{ ((drillResult.duration_ms || 0) / 1000).toFixed(2) }} 秒，校验值 {{ drillResult.backup_checksum }}
      </NAlert>
      <NEmpty v-else-if="!assessment?.latest_restore_drill" description="尚未有恢复演练记录">
        <template #icon><CircleHelp :size="22" /></template>
      </NEmpty>
      <div v-else class="assessment-latest-drill">
        <span>最近结果：{{ assessment.latest_restore_drill.result === 'success' ? '通过' : assessment.latest_restore_drill.result }}</span>
        <span>开始 {{ formatDate(assessment.latest_restore_drill.started_at) }}</span>
        <span>耗时 {{ ((assessment.latest_restore_drill.duration_ms || 0) / 1000).toFixed(2) }} 秒</span>
      </div>
      <small v-if="props.protectionEnabled" class="assessment-protection-note">当前账套已启用保护模式，恢复演练需要先解锁。</small>
    </section>
  </NSpin>
</template>
