<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { AlertTriangle, ArrowRight, CalendarClock, CircleDollarSign, ReceiptText, RefreshCw, TrendingUp } from 'lucide-vue-next'
import { NButton, NEmpty, NSpin, NTooltip, useMessage } from 'naive-ui'
import { command } from '../api'
import { labelForStatus, formatMoney } from '../theme'
import type { DashboardData, EntityName } from '../types'
import StatusBadge from './StatusBadge.vue'

const emit = defineEmits<{ navigate: [entity: EntityName] }>()
const message = useMessage()
const loading = ref(true)
const data = ref<DashboardData | null>(null)
const draggingCustomer = ref<{ id: number; stage: string } | null>(null)

const metricCards = computed(() => {
  const metrics = data.value?.metrics
  return [
    { label: '本月销售额', value: formatMoney(metrics?.monthly_sales_cents), icon: TrendingUp, tone: 'blue' },
    { label: '本月回款', value: formatMoney(metrics?.monthly_received_cents), icon: CircleDollarSign, tone: 'green' },
    { label: '应收未收', value: formatMoney(metrics?.unpaid_cents), icon: ReceiptText, tone: 'amber' },
    { label: '逾期跟进', value: `${metrics?.overdue_count ?? 0} 项`, icon: AlertTriangle, tone: 'red' },
  ]
})

async function load() {
  loading.value = true
  try {
    data.value = await command<DashboardData>('get_dashboard')
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
  } finally {
    loading.value = false
  }
}

async function moveCustomer(nextStage: string) {
  const item = draggingCustomer.value
  draggingCustomer.value = null
  if (!item || item.stage === nextStage) return
  try {
    await command('transition_entity', { entity: 'customers', id: item.id, nextStatus: nextStage, reason: '工作台看板拖拽推进' })
    message.success(`已推进至${labelForStatus(nextStage)}并生成跟进记录`)
    await load()
  } catch (error) {
    message.error(error instanceof Error ? error.message : String(error))
  }
}

onMounted(load)
</script>

<template>
  <section class="dashboard-view">
    <header class="view-heading compact-heading">
      <div>
        <p class="eyebrow">今日经营概览</p>
        <h1>工作台</h1>
      </div>
      <NTooltip>
        <template #trigger>
          <NButton quaternary circle aria-label="刷新工作台" @click="load"><RefreshCw :size="18" /></NButton>
        </template>
        刷新数据
      </NTooltip>
    </header>

    <NSpin :show="loading">
      <div class="metric-grid">
        <div v-for="card in metricCards" :key="card.label" class="metric-card" :class="`metric-${card.tone}`">
          <div class="metric-icon"><component :is="card.icon" :size="18" /></div>
          <div>
            <span>{{ card.label }}</span>
            <strong>{{ card.value }}</strong>
          </div>
        </div>
      </div>

      <div class="dashboard-band">
        <section class="todo-panel">
          <div class="section-title-row">
            <div>
              <h2>今日待办</h2>
              <p>按超期、今日到期和未来计划排序</p>
            </div>
            <CalendarClock :size="20" />
          </div>
          <div v-if="data?.todos.length" class="todo-list">
            <button v-for="todo in data.todos" :key="todo.id" class="todo-row" @click="emit('navigate', todo.entity)">
              <span class="todo-marker" :class="`tone-${todo.tone}`" />
              <span class="todo-copy"><strong>{{ todo.title }}</strong><small>{{ todo.subtitle }}</small></span>
              <span class="todo-date">{{ todo.due_date || '未定日期' }}</span>
              <ArrowRight :size="16" />
            </button>
          </div>
          <NEmpty v-else description="今日没有到期待办" size="small" />
        </section>

        <section class="snapshot-panel">
          <div class="section-title-row">
            <div>
              <h2>经营提示</h2>
              <p>当前账套需要优先处理的事项</p>
            </div>
          </div>
          <div class="snapshot-callout warning-callout">
            <AlertTriangle :size="20" />
            <div>
              <strong>{{ data?.metrics.overdue_count || 0 }} 项逾期跟进</strong>
              <span>应收未收 {{ formatMoney(data?.metrics.unpaid_cents) }}</span>
            </div>
            <NButton text type="primary" @click="emit('navigate', 'follow_ups')">处理</NButton>
          </div>
          <div class="snapshot-callout info-callout">
            <CalendarClock :size="20" />
            <div>
              <strong>{{ data?.todos.length || 0 }} 项近期任务</strong>
              <span>保存跟进记录会同步更新客户的下次跟进日期</span>
            </div>
            <NButton text type="primary" @click="emit('navigate', 'follow_ups')">处理</NButton>
          </div>
        </section>
      </div>

      <section class="pipeline-section">
        <div class="section-title-row">
          <div>
            <h2>客户销售阶段</h2>
            <p>拖动客户卡片推进阶段；不符合规则的流转会被拦截</p>
          </div>
        </div>
        <div class="pipeline-board">
          <div v-for="stage in data?.stages" :key="stage.code" class="pipeline-column" @dragover.prevent @drop="moveCustomer(stage.code)">
            <header>
              <StatusBadge :value="stage.code" :label="labelForStatus(stage.code)" />
              <span>{{ stage.count }}</span>
            </header>
            <div class="pipeline-cards">
              <article
                v-for="customer in stage.customers"
                :key="customer.id"
                class="pipeline-card"
                draggable="true"
                @dragstart="draggingCustomer = { id: customer.id, stage: String(customer.stage) }"
                @dragend="draggingCustomer = null"
                @dblclick="emit('navigate', 'customers')"
              >
                <strong>{{ customer.name }}</strong>
                <span>{{ customer.material_system || '材料体系待补充' }}</span>
                <small>{{ customer.next_follow_date ? `下次跟进 ${customer.next_follow_date}` : '未设置跟进日期' }}</small>
              </article>
              <div v-if="!stage.customers.length" class="pipeline-empty">暂无客户</div>
            </div>
          </div>
        </div>
      </section>
    </NSpin>
  </section>
</template>
