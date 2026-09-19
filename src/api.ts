import { invoke } from '@tauri-apps/api/core'
import { runTrackedTask, type BackgroundTaskKind } from './tasks'
import type { AttachmentRecord, BackupEntry, BootstrapInfo, DashboardData, DocumentItemRecord, EntityName, EntityRecord, OperationalAssessment, RestoreDrillRecord } from './types'
import { canTransition, derivedPaymentStatus, orderAmountCents, transitionNeedsReason, statusField, unpaidCents } from './domain'
import { DEMO_APP_VERSION } from './version'

const browserKey = 'plastic-sales-erp-demo-v1'
const browserSettingsKey = `${browserKey}-settings`
const browserDictsKey = `${browserKey}-dicts`
const browserSyncKey = `${browserKey}-sync-events`

type BrowserDb = Record<EntityName, EntityRecord[]>

function browserDate(date = new Date()) {
  const month = String(date.getMonth() + 1).padStart(2, '0')
  const day = String(date.getDate()).padStart(2, '0')
  return `${date.getFullYear()}-${month}-${day}`
}

const today = browserDate()
const inDays = (days: number) => browserDate(new Date(Date.now() + days * 86400000))

const defaultDb: BrowserDb = {
  customers: [
    { id: 1, name: '华东精密内饰', main_host: '华东汽车 Tier1', direction: '内饰出风口', material_system: 'PC/ABS', stage: 'quoting', next_follow_date: today, notes: '关注低气味与阻燃等级' },
    { id: 2, name: '宁波新锐电器', main_host: '新锐汽车', direction: '车载电器壳体', material_system: 'PP', stage: 'sampling', next_follow_date: inDays(2), notes: '等待第二轮测试反馈' },
    { id: 3, name: '苏州联创', main_host: '联创电子', direction: '结构件', material_system: 'PP+EPDM', stage: 'contacted', next_follow_date: inDays(-2), notes: '已确认年度预计量' },
    { id: 4, name: '嘉兴恒达', main_host: '', direction: '通用注塑', material_system: 'ABS', stage: 'lead', next_follow_date: inDays(7), notes: '' },
    { id: 5, name: '安徽远航', main_host: '远航新能源', direction: '电池包附件', material_system: 'PPS', stage: 'won', next_follow_date: inDays(14), notes: '定点后按月维护' },
    { id: 6, name: '浙江零跑科技股份有限公司', main_host: '零跑汽车', direction: '汽车零部件', material_system: 'PP-GF20', stage: 'quoting', next_follow_date: null, notes: '附件报价示例客户' },
    { id: 7, name: '台州市骏普塑模有限公司', main_host: '', direction: '塑料模具及制品', material_system: 'PP', stage: 'won', next_follow_date: null, notes: '附件合同示例客户' },
  ],
  follow_ups: [
    { id: 1, customer_id: 1, content: '已确认报价版本和阻燃测试安排。', follow_date: inDays(-1), next_date: today },
    { id: 2, customer_id: 2, content: '客户正在汇总第二轮测试反馈。', follow_date: today, next_date: inDays(2) },
    { id: 3, customer_id: 3, content: '已确认年度预计量，待推进送样。', follow_date: inDays(-4), next_date: inDays(-2) },
    { id: 4, customer_id: 4, content: '首次沟通后等待应用信息。', follow_date: today, next_date: inDays(7) },
    { id: 5, customer_id: 5, content: '定点项目按月维护客户关系。', follow_date: today, next_date: inDays(14) },
  ],
  contacts: [
    { id: 1, customer_id: 1, customer_name: '华东精密内饰', name: '王工', role: '采购', phone: '13800001234', email: '', preferred_channel: '微信' },
    { id: 2, customer_id: 2, customer_name: '宁波新锐电器', name: '陈工', role: '材料工程', phone: '13900005678', email: '', preferred_channel: '电话' },
  ],
  projects: [],
  materials: [
    { id: 1, code: 'PC/ABS-FR301', base_resin: 'PC/ABS', modification: '阻燃', mi: '12 g/10min', impact: '65 kJ/m²', hdt: '92 ℃', density: '1.18', supplier_name: '金发', supplier_id: 1, cost_cents: 1680, cost_date: today },
    { id: 2, code: 'PP+EPDM-TD20', base_resin: 'PP', modification: 'TD20', mi: '8 g/10min', impact: '38 kJ/m²', hdt: '105 ℃', density: '1.04', supplier_name: '聚隆（冠久）', supplier_id: 2, cost_cents: 920, cost_date: today },
    { id: 3, code: 'PPS-GF40', base_resin: 'PPS', modification: 'GF40', mi: '—', impact: '—', hdt: '260 ℃', density: '1.65', supplier_name: '金发', supplier_id: 1, cost_cents: 4860, cost_date: today },
    { id: 4, code: 'PG4-S01A', base_resin: 'PP', modification: 'GF20', mi: '', impact: '', hdt: '', density: '', supplier_name: '聚隆（冠久）', supplier_id: 2, cost_cents: 0, cost_date: null },
  ],
  samples: [
    { id: 1, code: 'S-202609-001', customer_id: 2, customer_name: '宁波新锐电器', material_id: 2, material_code: 'PP+EPDM-TD20', batch: 'B260901', weight_grams: 5000, sent_date: inDays(-7), test_items: '冲击、气味', status: 'testing', fail_reason: '' },
    { id: 2, code: 'S-202609-002', customer_id: 1, customer_name: '华东精密内饰', material_id: 1, material_code: 'PC/ABS- FR301', batch: 'B260905', weight_grams: 3000, sent_date: inDays(-15), test_items: '阻燃、耐热', status: 'passed', fail_reason: '' },
  ],
  sample_tests: [
    { id: 1, sample_id: 1, test_date: inDays(-5), test_item: '冲击强度', result: '38 kJ/m²', conclusion: '满足首轮目标', next_action: '补充气味测试结果' },
    { id: 2, sample_id: 2, test_date: inDays(-11), test_item: '阻燃与耐热', result: 'V-0 / 92 ℃', conclusion: '测试通过', next_action: '保留样品批次记录' },
  ],
  contracts: [
    {
      id: 1,
      no: 'TZJP202609-18',
      seller_name: '南京聚隆科技股份有限公司',
      buyer_name: '台州市骏普塑模有限公司',
      customer_id: 7,
      execution_place: '南京',
      contract_date: '2026-09-18',
      settlement_method: '款到发货',
      packaging: '25KG/袋',
      terms: '二、质量要求技术标准：质量要求按供方材料技术质量标准。\n三、交（提）货地点、方式：交货地点为供方仓库，供方代办运输。\n四、运输方式及目的地和费用负担：汽运至需方指定地点，供方负责运费。\n五、包装方式：25KG/袋。\n六、验收方法及提出异议期限：按条款（二）验收，提出异议期限为收货一周内。异议期内，需方不得批量投料，一经投料，供方将不承担任何经济损失。\n七、结算方式及期限：款到发货。\n八、违约责任：需方应按约付款，延迟一日承担货款总额万分之三的违约金。\n九、解决合同纠纷的方式：双方协商解决，协商不成，由供方所在地法院诉讼解决。\n十、本合同经双方签字或盖章后即生效，传真件具有同等法律效力。',
      seller_address: '南京市高新技术开发区聚龙路8号',
      seller_legal_representative: '刘曙阳',
      seller_agent: '张迎波',
      seller_phone: '',
      seller_fax: '',
      seller_bank: '中信银行建邺支行',
      seller_account: '7329210182800049261',
      buyer_address: '浙江省台州黄岩区北城街道锦川路8号',
      buyer_legal_representative: '',
      buyer_agent: '',
      buyer_phone: '',
      buyer_fax: '',
      buyer_bank: '台州银行股份有限公司黄岩工业园区支行',
      buyer_account: '530166084800015',
      status: 'draft',
    },
  ],
  quotations: [
    { id: 1, no: 'Q-202609-001', customer_id: 1, customer_name: '华东精密内饰', material_id: 1, material_code: 'PC/ABS-FR301', price_cents: 2180, moq_grams: 50000, freight: '卖方', valid_until: inDays(12), seller_name: '南京聚隆科技股份有限公司', recipient_name: '华东精密内饰', quote_date: today, subject: '材料报价', price_note: '含税单价', adjustment_note: '', footer_address: '', footer_phone: '', footer_fax: '', footer_email: '', version: 2, status: 'sent' },
    { id: 2, no: 'Q-202609-002', customer_id: 2, customer_name: '宁波新锐电器', material_id: 2, material_code: 'PP+EPDM-TD20', price_cents: 1360, moq_grams: 100000, freight: '买方', valid_until: inDays(-3), seller_name: '南京聚隆科技股份有限公司', recipient_name: '宁波新锐电器', quote_date: today, subject: '材料报价', price_note: '', adjustment_note: '', footer_address: '', footer_phone: '', footer_fax: '', footer_email: '', version: 1, status: 'expired' },
    { id: 3, no: '零跑PP-GF20-0615', customer_id: 6, customer_name: '浙江零跑科技股份有限公司', material_id: 4, material_code: 'PG4-S01A', price_cents: 1060, moq_grams: 1000, freight: '含运费', valid_until: null, seller_name: '南京聚隆科技股份有限公司', recipient_name: '浙江零跑科技股份有限公司', sender_name: '', recipient_contact: '', recipient_fax: '', cc: '', page_count: 1, request_review: true, request_comment: true, quote_date: '2026-06-15', subject: '材料报价', price_note: '未税、含运费', adjustment_note: '为保证贵我双方的利益，我公司将根据原材料价格的上下波动及时调整供货价格。', footer_address: '南京高新技术开发区聚龙路 8 号', footer_phone: '025-58840064', footer_fax: '025-58746904', footer_email: 'julong@publicl.ptt.js.cn', version: 1, status: 'draft' },
  ],
  orders: [
    { id: 1, no: 'SO-202609-001', customer_id: 1, customer_name: '华东精密内饰', material_id: 1, material_code: 'PC/ABS-FR301', qty_grams: 120000, price_cents: 2180, amount_cents: 261600, delivery_date: inDays(-1), status: 'preparing', sign_status: 'pending', quotation_id: 1 },
    { id: 2, no: 'SO-202608-008', customer_id: 5, customer_name: '安徽远航', material_id: 3, material_code: 'PPS-GF40', qty_grams: 50000, price_cents: 5720, amount_cents: 286000, delivery_date: inDays(-12), status: 'signed', sign_status: 'signed', quotation_id: null },
    { id: 3, no: 'SO-202609-002', customer_id: 2, customer_name: '宁波新锐电器', material_id: 2, material_code: 'PP+EPDM-TD20', qty_grams: 80000, price_cents: 1420, amount_cents: 113600, delivery_date: inDays(5), status: 'pending_confirm', sign_status: 'pending', quotation_id: null },
  ],
  deliveries: [],
  statements: [
    { id: 1, no: 'AR-202608-001', customer_id: 5, customer_name: '安徽远航', period_start: '2026-08-01', period_end: '2026-08-31', total_cents: 286000, received_cents: 100000, unpaid_cents: 186000, account_days: 60, aging_days: 12, promise_date: inDays(-4), pay_status: 'overdue' },
    { id: 2, no: 'AR-202609-001', customer_id: 1, customer_name: '华东精密内饰', period_start: '2026-09-01', period_end: '2026-09-30', total_cents: 261600, received_cents: 0, unpaid_cents: 261600, account_days: 45, aging_days: 0, promise_date: inDays(18), pay_status: 'not_due' },
  ],
  payments: [{ id: 1, statement_id: 1, statement_no: 'AR-202608-001', amount_cents: 100000, pay_date: inDays(-16), method: '转账' }],
  suppliers: [{ id: 1, name: '金发', contact: '采购部', price_ref_cents: 1680 }, { id: 2, name: '聚隆（冠久）', contact: '销售部', price_ref_cents: 920 }],
  purchases: [{ id: 1, no: 'PO-202609-001', supplier_id: 2, supplier_name: '聚隆（冠久）', material_id: 2, material_code: 'PP+EPDM-TD20', qty_grams: 200000, status: 'in_transit', eta: inDays(3) }],
  inventory: [{ id: 1, material_id: 1, material_code: 'PC/ABS-FR301', opening_grams: 0, on_hand_grams: 260000, in_transit_grams: 0, safety_grams: 100000, available_grams: 260000 }, { id: 2, material_id: 2, material_code: 'PP+EPDM-TD20', opening_grams: 0, on_hand_grams: 60000, in_transit_grams: 200000, safety_grams: 120000, available_grams: 260000 }, { id: 3, material_id: 3, material_code: 'PPS-GF40', opening_grams: 0, on_hand_grams: 18000, in_transit_grams: 0, safety_grams: 30000, available_grams: 18000 }],
  quotation_items: [
    { id: 1, quotation_id: 1, quotation_no: 'Q-202609-001', material_id: 1, material_code: 'PC/ABS-FR301', batch: 'B260901', material_category: 'PC/ABS', material_grade: 'PC/ABS-FR301', manufacturer: '金发', note: '', qty_grams: 50000, unit_price_cents: 2180, amount_cents: 109000 },
    { id: 2, quotation_id: 2, quotation_no: 'Q-202609-002', material_id: 2, material_code: 'PP+EPDM-TD20', batch: 'B260902', material_category: 'PP', material_grade: 'PP+EPDM-TD20', manufacturer: '聚隆', note: '', qty_grams: 100000, unit_price_cents: 1360, amount_cents: 136000 },
    { id: 3, quotation_id: 3, quotation_no: '零跑PP-GF20-0615', material_id: 4, material_code: 'PG4-S01A', batch: '', material_category: 'PP-GF20', material_grade: 'PG4-S01A', manufacturer: '南京聚隆科技股份有限公司', note: '未税、含运费', qty_grams: 1000, unit_price_cents: 1060, amount_cents: 1060 },
  ],
  contract_items: [
    { id: 1, contract_id: 1, contract_no: 'TZJP202609-18', material_id: null, material_name: 'PP', model: 'PI0-S27A[BK16452]', manufacturer: '聚隆', qty_grams: 200000, unit_price_cents: 1180, amount_cents: 236000, note: '具体交期双方协商' },
  ],
  order_items: [
    { id: 1, order_id: 1, order_no: 'SO-202609-001', material_id: 1, material_code: 'PC/ABS-FR301', batch: 'B260901', qty_grams: 120000, unit_price_cents: 2180, amount_cents: 261600 },
    { id: 2, order_id: 2, order_no: 'SO-202608-008', material_id: 3, material_code: 'PPS-GF40', batch: 'B260820', qty_grams: 50000, unit_price_cents: 5720, amount_cents: 286000 },
    { id: 3, order_id: 3, order_no: 'SO-202609-002', material_id: 2, material_code: 'PP+EPDM-TD20', batch: 'B260905', qty_grams: 80000, unit_price_cents: 1420, amount_cents: 113600 },
  ],
  delivery_items: [],
  statement_items: [
    { id: 1, statement_id: 1, statement_no: 'AR-202608-001', order_id: 2, order_no: 'SO-202608-008', allocated_amount_cents: 286000 },
    { id: 2, statement_id: 2, statement_no: 'AR-202609-001', order_id: 1, order_no: 'SO-202609-001', allocated_amount_cents: 261600 },
  ],
  payment_allocations: [{ id: 1, payment_id: 1, payment_amount_cents: 100000, pay_date: inDays(-16), statement_id: 1, statement_no: 'AR-202608-001', allocated_amount_cents: 100000 }],
  inventory_movements: [
    { id: 1, material_id: 1, material_code: 'PC/ABS-FR301', movement_type: 'opening', qty_grams: 260000, batch: 'B260901', warehouse: '主仓', unit_cost_cents: 1680, source_id: null, created_at: `${today}T08:00:00+08:00` },
    { id: 2, material_id: 2, material_code: 'PP+EPDM-TD20', movement_type: 'opening', qty_grams: 60000, batch: 'B260905', warehouse: '主仓', unit_cost_cents: 920, source_id: null, created_at: `${today}T08:01:00+08:00` },
    { id: 3, material_id: 3, material_code: 'PPS-GF40', movement_type: 'opening', qty_grams: 18000, batch: 'B260820', warehouse: '主仓', unit_cost_cents: 4860, source_id: null, created_at: `${today}T08:02:00+08:00` },
  ],
  stock_reservations: [{ id: 1, material_id: 1, material_code: 'PC/ABS-FR301', order_id: 1, order_no: 'SO-202609-001', reserved_grams: 120000, batch: 'B260901', warehouse: '主仓', status: 'active', created_at: `${today}T08:10:00+08:00` }],
  attachments: [],
  cost_history: [
    { id: 1, material_id: 1, material_code: 'PC/ABS-FR301', cost_cents: 1680, cost_date: today, source: 'manual', note: '演示初始成本', created_at: `${today}T08:00:00+08:00` },
    { id: 2, material_id: 2, material_code: 'PP+EPDM-TD20', cost_cents: 920, cost_date: today, source: 'manual', note: '演示初始成本', created_at: `${today}T08:00:00+08:00` },
    { id: 3, material_id: 3, material_code: 'PPS-GF40', cost_cents: 4860, cost_date: today, source: 'manual', note: '演示初始成本', created_at: `${today}T08:00:00+08:00` },
  ],
}

const defaultBrowserSettings: Record<string, string> = {
  backup_retention: '10',
  reminder_days: '3',
  close_behavior: 'minimize_to_tray',
  backup_schedule_enabled: '1',
  backup_schedule_days: '1',
  cloud_sync_enabled: '0',
}
const defaultBrowserDicts: EntityRecord[] = [
  { id: 1, type: 'customer_stage', value: 'lead', label: '潜在客户', sort_order: 10, enabled: true },
  { id: 2, type: 'customer_stage', value: 'contacted', label: '初步接触', sort_order: 20, enabled: true },
  { id: 3, type: 'customer_stage', value: 'sampling', label: '送样测试', sort_order: 30, enabled: true },
  { id: 4, type: 'customer_stage', value: 'quoting', label: '报价谈判', sort_order: 40, enabled: true },
  { id: 5, type: 'customer_stage', value: 'won', label: '定点量产', sort_order: 50, enabled: true },
  { id: 6, type: 'customer_stage', value: 'paused_lost', label: '暂停 / 流失', sort_order: 60, enabled: true },
  { id: 7, type: 'sample_status', value: 'pending_send', label: '待送样', sort_order: 10, enabled: true },
  { id: 8, type: 'sample_status', value: 'sent', label: '已送样', sort_order: 20, enabled: true },
  { id: 9, type: 'sample_status', value: 'testing', label: '测试中', sort_order: 30, enabled: true },
  { id: 10, type: 'sample_status', value: 'passed', label: '通过', sort_order: 40, enabled: true },
  { id: 11, type: 'sample_status', value: 'failed', label: '不通过', sort_order: 50, enabled: true },
  { id: 12, type: 'sample_status', value: 'retest', label: '待复测', sort_order: 60, enabled: true },
  { id: 13, type: 'order_status', value: 'pending_confirm', label: '待确认', sort_order: 10, enabled: true },
  { id: 14, type: 'order_status', value: 'preparing', label: '备货中', sort_order: 20, enabled: true },
  { id: 15, type: 'order_status', value: 'delivered', label: '已送货', sort_order: 30, enabled: true },
  { id: 16, type: 'order_status', value: 'signed', label: '已回签', sort_order: 40, enabled: true },
  { id: 17, type: 'order_status', value: 'completed', label: '已完成', sort_order: 50, enabled: true },
  { id: 18, type: 'order_status', value: 'cancelled', label: '已取消', sort_order: 60, enabled: true },
  { id: 19, type: 'pay_status', value: 'not_due', label: '未到期', sort_order: 10, enabled: true },
  { id: 20, type: 'pay_status', value: 'near_due', label: '临近到期', sort_order: 20, enabled: true },
  { id: 21, type: 'pay_status', value: 'overdue', label: '已逾期', sort_order: 30, enabled: true },
  { id: 22, type: 'pay_status', value: 'settled', label: '已结清', sort_order: 40, enabled: true },
  { id: 23, type: 'purchase_status', value: 'pending_quote', label: '待询价', sort_order: 10, enabled: true },
  { id: 24, type: 'purchase_status', value: 'ordered', label: '已下单', sort_order: 20, enabled: true },
  { id: 25, type: 'purchase_status', value: 'in_transit', label: '在途', sort_order: 30, enabled: true },
  { id: 26, type: 'purchase_status', value: 'received', label: '已入库', sort_order: 40, enabled: true },
  { id: 27, type: 'purchase_status', value: 'closed', label: '关闭', sort_order: 50, enabled: true },
  { id: 28, type: 'purchase_status', value: 'cancelled', label: '已取消', sort_order: 60, enabled: true },
]

function loadBrowserSettings() {
  try {
    const parsed = JSON.parse(localStorage.getItem(browserSettingsKey) || '{}') as Record<string, unknown>
    return Object.fromEntries(Object.entries(defaultBrowserSettings).map(([key, fallback]) => [key, String(parsed[key] ?? fallback)]))
  } catch {
    return { ...defaultBrowserSettings }
  }
}

function loadBrowserDicts() {
  try {
    const parsed = JSON.parse(localStorage.getItem(browserDictsKey) || 'null')
    return Array.isArray(parsed) ? parsed as EntityRecord[] : structuredClone(defaultBrowserDicts)
  } catch {
    return structuredClone(defaultBrowserDicts)
  }
}

const browserSettings = loadBrowserSettings()
const browserDicts = loadBrowserDicts()

type BrowserSyncEvent = {
  local_id: number
  event_id: string
  entity_type: string
  entity_id: number
  operation: string
  payload: Record<string, unknown>
  created_at: string
  retry_count: number
  last_error: string
  synced_at?: string
}

function loadBrowserSyncEvents(): BrowserSyncEvent[] {
  try {
    const parsed = JSON.parse(localStorage.getItem(browserSyncKey) || '[]')
    return Array.isArray(parsed) ? parsed as BrowserSyncEvent[] : []
  } catch {
    return []
  }
}

let browserSyncEvents = loadBrowserSyncEvents()

function saveBrowserSyncEvents() {
  localStorage.setItem(browserSyncKey, JSON.stringify(browserSyncEvents))
}

function browserReminderDays() {
  const parsed = Number.parseInt(browserSettings.reminder_days, 10)
  return Number.isFinite(parsed) ? Math.max(0, Math.min(365, parsed)) : 3
}

function browserSettingValue(key: string, value: unknown) {
  if (key === 'close_behavior') {
    const normalized = String(value ?? '').trim()
    if (normalized === 'minimize_to_tray' || normalized === 'tray') return 'minimize_to_tray'
    if (normalized === 'confirm_exit' || normalized === 'exit') return 'confirm_exit'
    throw new Error('关闭窗口行为只能选择最小化到托盘或询问后退出')
  }
  if (key === 'backup_schedule_enabled' || key === 'cloud_sync_enabled') {
    const normalized = String(value ?? '').trim().toLowerCase()
    if (['1', 'true', 'on', 'yes'].includes(normalized)) return '1'
    if (['0', 'false', 'off', 'no'].includes(normalized)) return '0'
    throw new Error(key === 'cloud_sync_enabled' ? '自动同步只能设置为启用或停用' : '定时自动备份只能设置为启用或停用')
  }
  const parsed = Number(value)
  if (!Number.isInteger(parsed)) throw new Error(`${key === 'reminder_days' ? '回款临近提醒天数' : key === 'backup_schedule_days' ? '定时备份间隔天数' : '备份保留份数'}必须是整数`)
  const bounds = key === 'reminder_days'
    ? { min: 0, max: 365, label: '回款临近提醒天数' }
    : key === 'backup_retention'
      ? { min: 1, max: 100, label: '备份保留份数' }
      : key === 'backup_schedule_days'
        ? { min: 1, max: 30, label: '定时备份间隔天数' }
        : null
  if (!bounds) throw new Error(`不支持的设置项：${key}`)
  if (parsed < bounds.min || parsed > bounds.max) throw new Error(`${bounds.label}应在 ${bounds.min} 至 ${bounds.max} 之间`)
  return String(parsed)
}

function validateBrowserStatusEntry(entity: EntityName, id: number, next: string) {
  if (entity === 'statements') throw new Error('对账状态由应收余额、承诺回款日和提醒参数自动计算，不能手工流转')
  const row = (browserDb[entity] ?? []).find((item) => Number(item.id) === id)
  if (!row) throw new Error('记录不存在')
  if (entity === 'samples') {
    if (next === 'sent' && (!String(row.batch ?? '').trim() || !String(row.sent_date ?? '').trim())) throw new Error('样品进入已送样前必须填写送样日期和批次')
    if (next === 'testing') {
      const hasTestItem = String(row.test_items ?? '').trim() || (browserDb.sample_tests ?? []).some((item) => Number(item.sample_id) === id && String(item.test_item ?? '').trim())
      if (!hasTestItem) throw new Error('样品进入测试中前必须填写测试项目')
    }
    if (next === 'passed') {
      const latest = [...(browserDb.sample_tests ?? [])].filter((item) => Number(item.sample_id) === id).sort((a, b) => String(b.test_date ?? '').localeCompare(String(a.test_date ?? '')) || Number(b.id) - Number(a.id))[0]
      if (!String(latest?.conclusion ?? '').trim()) throw new Error('样品测试通过前必须登记测试结论')
    }
    if (['failed', 'retest'].includes(next) && !String(row.fail_reason ?? '').trim()) throw new Error('样品不通过或待复测前必须填写原因')
  }
  if (entity === 'orders') {
    if (next === 'preparing' && !String(row.delivery_date ?? '').trim()) throw new Error('订单进入备货中前必须填写交期')
    if (next === 'delivered') {
      const hasDelivery = (browserDb.deliveries ?? []).some((delivery) => Number(delivery.order_id) === id && String(delivery.sent_date ?? '').trim() && (browserDb.delivery_items ?? []).filter((item) => Number(item.delivery_id) === Number(delivery.id)).some((item) => Number(item.qty_grams ?? 0) > 0))
      if (!hasDelivery) throw new Error('订单进入已送货前必须登记送货日期和数量')
    }
    if (next === 'signed') {
      const hasSignedDelivery = (browserDb.deliveries ?? []).some((delivery) => Number(delivery.order_id) === id && String(delivery.sent_date ?? '').trim() && String(delivery.sign_date ?? '').trim())
      if (!hasSignedDelivery) throw new Error('订单进入已回签前必须登记送货日期和回签日期')
    }
  }
}

function loadBrowserDb(): BrowserDb {
  try {
    const raw = localStorage.getItem(browserKey)
    if (!raw) return structuredClone(defaultDb)
    const parsed = JSON.parse(raw) as Partial<BrowserDb>
    const hasInventoryMovements = Array.isArray(parsed.inventory_movements)
    const db = Object.fromEntries((Object.keys(defaultDb) as EntityName[]).map((entity) => {
      const rows = parsed[entity]
      return [entity, Array.isArray(rows) ? rows : structuredClone(defaultDb[entity])]
    })) as BrowserDb
    let changed = false
    if (!hasInventoryMovements) {
      // Older demo snapshots had inventory balances but no movement ledger.
      // Rebuild an opening movement from each balance so derived stock remains stable.
      db.inventory_movements = []
      changed = true
    }
    const materialCodes = new Map((db.materials ?? []).map((row) => [Number(row.id), String(row.code ?? '')]))
    let nextMovementId = Math.max(0, ...(db.inventory_movements ?? []).map((row) => Number(row.id) || 0)) + 1
    for (const inventory of db.inventory ?? []) {
      const materialId = Number(inventory.material_id)
      if (!Number.isInteger(materialId) || materialId <= 0) continue
      const existingMovements = (db.inventory_movements ?? []).filter((row) => Number(row.material_id) === materialId)
      const storedOnHand = Number(inventory.on_hand_grams)
      const storedOpening = Number(inventory.opening_grams)
      const current = Number.isFinite(storedOnHand) ? storedOnHand : (Number.isFinite(storedOpening) ? storedOpening : 0)
      if (!existingMovements.length && current > 0) {
        db.inventory_movements.push({
          id: nextMovementId++, material_id: materialId, material_code: materialCodes.get(materialId) ?? '',
          movement_type: 'opening', qty_grams: Math.round(current), batch: '', warehouse: '主仓', unit_cost_cents: 0,
          source_id: null, created_at: new Date().toISOString(),
        })
        changed = true
      }
      const opening = Math.round(current - movementNetFor(db, materialId))
      if (!Number.isFinite(storedOpening) || storedOpening !== opening) {
        inventory.opening_grams = opening
        changed = true
      }
    }
    for (const movement of db.inventory_movements ?? []) {
      const code = materialCodes.get(Number(movement.material_id))
      if (code && movement.material_code !== code) {
        movement.material_code = code
        changed = true
      }
    }
    if (!Array.isArray(parsed.follow_ups)) {
      db.follow_ups = db.customers.filter((customer) => Boolean(customer.next_follow_date)).map((customer, index) => ({
        id: index + 1,
        customer_id: customer.id,
        content: '历史迁移：保留原下次跟进日期。',
        follow_date: today,
        next_date: customer.next_follow_date,
      }))
      changed = true
    }
    if (changed) localStorage.setItem(browserKey, JSON.stringify(db))
    return db
  } catch {
    return structuredClone(defaultDb)
  }
}

function saveBrowserDb(db: BrowserDb) {
  localStorage.setItem(browserKey, JSON.stringify(db))
}

function restoreBrowserDb(snapshot: BrowserDb) {
  for (const entity of Object.keys(defaultDb) as EntityName[]) {
    browserDb[entity] = structuredClone(snapshot[entity] ?? [])
  }
  saveBrowserDb(browserDb)
}

function movementNetFor(db: BrowserDb, materialId: number) {
  return (db.inventory_movements ?? []).filter((row) => Number(row.material_id) === materialId).reduce((sum, row) => {
    const type = String(row.movement_type ?? '')
    const inbound = ['opening', 'purchase_receipt', 'adjustment_in', 'return_in'].includes(type)
    return sum + (inbound ? 1 : -1) * Number(row.qty_grams ?? 0)
  }, 0)
}

const documentEntityMap: Record<string, { entity: EntityName; key: string }> = {
  contracts: { entity: 'contract_items', key: 'contract_id' },
  quotations: { entity: 'quotation_items', key: 'quotation_id' },
  orders: { entity: 'order_items', key: 'order_id' },
  deliveries: { entity: 'delivery_items', key: 'delivery_id' },
  statements: { entity: 'statement_items', key: 'statement_id' },
  payments: { entity: 'payment_allocations', key: 'payment_id' },
}

function nextBrowserId(entity: EntityName) {
  return Math.max(0, ...(browserDb[entity] ?? []).map((row) => Number(row.id) || 0)) + 1
}

function lineAmountCents(qtyGrams: number, unitPriceCents: number) {
  return Math.round(qtyGrams * unitPriceCents / 1000)
}

function browserDocumentItems(documentType: string, documentId: number) {
  const mapping = documentEntityMap[documentType]
  if (!mapping) throw new Error(`不支持的明细单据类型：${documentType}`)
  return (browserDb[mapping.entity] ?? []).filter((row) => Number(row[mapping.key]) === documentId)
}

function browserQuotationBaseNo(no: string) {
  const trimmed = no.trim()
  const match = trimmed.match(/^(.*)-V(\d+)$/)
  return match ? match[1] : trimmed
}

function nextBrowserQuotationNo(sourceNo: string, sourceVersion: number) {
  const base = browserQuotationBaseNo(sourceNo)
  let version = Math.max(1, sourceVersion) + 1
  while ((browserDb.quotations ?? []).some((row) => String(row.no ?? '') === `${base}-V${version}`)) version += 1
  return { no: `${base}-V${version}`, version }
}

function browserQuotationComparison(leftId: number, rightId: number) {
  if (leftId <= 0 || rightId <= 0 || leftId === rightId) throw new Error('请选择两条不同的报价版本')
  const left = browserDb.quotations.find((row) => Number(row.id) === leftId)
  const right = browserDb.quotations.find((row) => Number(row.id) === rightId)
  if (!left || !right) throw new Error('报价版本不存在')
  const fields: Array<[string, string]> = [
    ['customer_id', '客户'], ['recipient_name', '收件单位'], ['sender_name', '发件人'], ['recipient_contact', '收件人'],
    ['recipient_fax', '传真号'], ['cc', '抄送'], ['page_count', '页数'], ['request_review', '请审阅'],
    ['request_comment', '请批注'], ['quote_date', '报价日期'], ['subject', '主题'], ['price_note', '价格说明'],
    ['adjustment_note', '调价说明'], ['footer_address', '页脚地址'], ['footer_phone', '页脚电话'], ['footer_fax', '页脚传真'], ['footer_email', '页脚邮箱'],
  ]
  const headerDifferences = fields.flatMap(([field, label]) => {
    const leftValue = left[field] ?? null
    const rightValue = right[field] ?? null
    return JSON.stringify(leftValue) === JSON.stringify(rightValue) ? [] : [{ field, label, left: leftValue, right: rightValue }]
  })
  const itemSignature = (item: EntityRecord | undefined) => item ? ({
    material_id: item.material_id ?? null,
    batch: item.batch ?? null,
    material_category: item.material_category ?? null,
    material_grade: item.material_grade ?? null,
    manufacturer: item.manufacturer ?? null,
    note: item.note ?? null,
    qty_grams: item.qty_grams ?? null,
    unit_price_cents: item.unit_price_cents ?? null,
    amount_cents: item.amount_cents ?? null,
  }) : null
  const leftItems = browserDocumentItems('quotations', leftId)
  const rightItems = browserDocumentItems('quotations', rightId)
  const itemDifferences = [] as Array<Record<string, unknown>>
  for (let index = 0; index < Math.max(leftItems.length, rightItems.length); index += 1) {
    const leftValue = itemSignature(leftItems[index])
    const rightValue = itemSignature(rightItems[index])
    if (JSON.stringify(leftValue) === JSON.stringify(rightValue)) continue
    itemDifferences.push({
      index: index + 1,
      change: !leftValue ? 'added' : !rightValue ? 'removed' : 'changed',
      left: leftValue,
      right: rightValue,
    })
  }
  return {
    left: { id: leftId, no: left.no, version: left.version },
    right: { id: rightId, no: right.no, version: right.version },
    header_differences: headerDifferences,
    item_differences: itemDifferences,
    same: headerDifferences.length === 0 && itemDifferences.length === 0,
  }
}

function duplicateBrowserQuotation(quotationId: number) {
  const source = browserDb.quotations.find((row) => Number(row.id) === quotationId)
  if (!source) throw new Error('报价单不存在')
  const next = nextBrowserQuotationNo(String(source.no ?? ''), Number(source.version ?? 1))
  const id = nextBrowserId('quotations')
  const record: EntityRecord = {
    ...structuredClone(source),
    id,
    no: next.no,
    version: next.version,
    status: 'draft',
    quote_date: browserDate(),
    valid_until: null,
  }
  delete record.item_count
  delete record.item_total_cents
  browserDb.quotations.unshift(record)
  const sourceItems = browserDocumentItems('quotations', quotationId)
  let nextItem = nextBrowserId('quotation_items')
  const copiedItems = sourceItems.map((item) => ({ ...structuredClone(item), id: nextItem++, quotation_id: id, quotation_no: next.no }))
  browserDb.quotation_items.push(...copiedItems)
  saveBrowserDb(browserDb)
  return enrichBrowserRow('quotations', record)
}

function convertBrowserQuotation(quotationId: number, requestedOrderNo: string, deliveryDate: string) {
  const quotation = browserDb.quotations.find((row) => Number(row.id) === quotationId)
  if (!quotation) throw new Error('报价单不存在')
  if (['expired', 'rejected'].includes(String(quotation.status ?? ''))) throw new Error('已过期或已拒绝的报价不能转为订单')
  let items = browserDocumentItems('quotations', quotationId).map((item) => ({ ...item }))
  if (!items.length) items = [{ id: 0, material_id: Number(quotation.material_id), qty_grams: Number(quotation.moq_grams), unit_price_cents: Number(quotation.price_cents), batch: '' }]
  for (const item of items) {
    const materialId = Number(item.material_id)
    if (!materialId || !browserDb.materials.some((row) => Number(row.id) === materialId)) throw new Error('报价客户或牌号信息不完整，不能转为订单')
    if (Number(item.qty_grams) <= 0 || Number(item.unit_price_cents) <= 0) throw new Error('报价数量和单价必须大于 0，不能转为订单')
  }
  const date = deliveryDate.trim()
  if (date && !/^\d{4}-\d{2}-\d{2}$/.test(date)) throw new Error('交期必须使用 YYYY-MM-DD 日期')
  const used = new Set((browserDb.orders ?? []).map((row) => String(row.no ?? '')))
  let no = requestedOrderNo.trim()
  if (no) {
    if (used.has(no)) throw new Error('订单号已存在，请更换订单号')
  } else {
    const prefix = `SO-${browserDate().replaceAll('-', '')}`
    let sequence = 1
    do { no = `${prefix}-${String(sequence).padStart(3, '0')}`; sequence += 1 } while (used.has(no))
  }
  const first = items[0]
  const orderId = nextBrowserId('orders')
  const order: EntityRecord = {
    id: orderId,
    no,
    customer_id: Number(quotation.customer_id),
    customer_name: quotation.customer_name,
    material_id: Number(first.material_id),
    material_code: browserDb.materials.find((row) => Number(row.id) === Number(first.material_id))?.code,
    qty_grams: items.reduce((sum, item) => sum + Number(item.qty_grams), 0),
    price_cents: Number(first.unit_price_cents),
    amount_cents: items.reduce((sum, item) => sum + Number(item.amount_cents ?? lineAmountCents(Number(item.qty_grams), Number(item.unit_price_cents))), 0),
    delivery_date: date || null,
    status: 'pending_confirm',
    sign_status: 'pending',
    quotation_id: quotationId,
  }
  browserDb.orders.unshift(order)
  let nextItem = nextBrowserId('order_items')
  browserDb.order_items.push(...items.map((item) => ({
    id: nextItem++,
    order_id: orderId,
    order_no: no,
    material_id: Number(item.material_id),
    material_code: browserDb.materials.find((row) => Number(row.id) === Number(item.material_id))?.code ?? '',
    batch: String(item.batch ?? ''),
    qty_grams: Number(item.qty_grams),
    unit_price_cents: Number(item.unit_price_cents),
    amount_cents: Number(item.amount_cents ?? lineAmountCents(Number(item.qty_grams), Number(item.unit_price_cents))),
  })))
  syncBrowserDocumentHeader('orders', orderId)
  saveBrowserDb(browserDb)
  return enrichBrowserRow('orders', order)
}

function movementNet(materialId: number) {
  return movementNetFor(browserDb, materialId)
}

function refreshBrowserInventory(materialId?: number) {
  const targets = (browserDb.inventory ?? []).filter((row) => materialId == null || Number(row.material_id) === materialId)
  for (const row of targets) {
    const id = Number(row.material_id)
    const opening = Number(row.opening_grams ?? 0)
    const onHand = opening + movementNet(id)
    const transit = (browserDb.purchases ?? []).filter((purchase) => Number(purchase.material_id) === id && ['ordered', 'in_transit'].includes(String(purchase.status ?? ''))).reduce((sum, purchase) => sum + Number(purchase.qty_grams ?? 0), 0)
    const reserved = (browserDb.stock_reservations ?? []).filter((reservation) => Number(reservation.material_id) === id && String(reservation.status ?? '') === 'active').reduce((sum, reservation) => sum + Number(reservation.reserved_grams ?? 0), 0)
    row.on_hand_grams = onHand
    row.in_transit_grams = transit
    row.reserved_grams = reserved
    row.available_grams = onHand + transit - reserved
    row.low_stock = Number(row.available_grams ?? 0) < Number(row.safety_grams ?? 0)
  }
}

function syncBrowserDocumentHeader(documentType: string, documentId: number) {
  const items = browserDocumentItems(documentType, documentId)
  const document = (browserDb[documentType as EntityName] ?? []).find((row) => row.id === documentId)
  if (!document || !items.length) return
  const first = items[0]
  const totalQty = items.reduce((sum, item) => sum + Number(item.qty_grams ?? 0), 0)
  const total = items.reduce((sum, item) => sum + Number(item.amount_cents ?? 0), 0)
  document.material_id = Number(first.material_id)
  document.price_cents = Number(first.unit_price_cents)
  if (documentType === 'quotations') {
    document.moq_grams = totalQty
    document.item_total_cents = total
  } else if (documentType === 'orders') {
    document.qty_grams = totalQty
    document.amount_cents = total
  }
}

function browserOrderTotal(orderId: number) {
  const items = browserDocumentItems('orders', orderId)
  if (items.length) return items.reduce((sum, item) => sum + Number(item.amount_cents ?? 0), 0)
  const order = browserDb.orders.find((row) => row.id === orderId)
  return order ? orderAmountCents(order) : 0
}

function refreshBrowserDeliveryMovements(deliveryId: number, oldSourceIds: number[] = []) {
  const sourceIds = oldSourceIds.length ? oldSourceIds : (browserDb.inventory_movements ?? []).filter((row) => row.movement_type === 'sale_issue' && browserDb.delivery_items.some((item) => Number(item.id) === Number(row.source_id) && Number(item.delivery_id) === deliveryId)).map((row) => Number(row.source_id))
  const delivery = browserDb.deliveries.find((row) => row.id === deliveryId)
  if (!delivery || !String(delivery.sent_date ?? '')) {
    browserDb.inventory_movements = (browserDb.inventory_movements ?? []).filter((row) => !sourceIds.includes(Number(row.source_id)) || row.movement_type !== 'sale_issue')
    refreshBrowserInventory()
    return
  }
  const items = browserDocumentItems('deliveries', deliveryId)
  const oldMovements = (browserDb.inventory_movements ?? []).filter((row) => row.movement_type === 'sale_issue' && sourceIds.includes(Number(row.source_id)))
  refreshBrowserInventory()
  const baseOnHand = new Map<number, number>((browserDb.inventory ?? []).map((row) => [Number(row.material_id), Number(row.on_hand_grams ?? 0)]))
  for (const movement of oldMovements) {
    const materialId = Number(movement.material_id)
    baseOnHand.set(materialId, (baseOnHand.get(materialId) ?? 0) + Number(movement.qty_grams ?? 0))
  }
  const transitByMaterial = new Map<number, number>()
  for (const purchase of browserDb.purchases ?? []) {
    if (!['ordered', 'in_transit'].includes(String(purchase.status ?? ''))) continue
    const materialId = Number(purchase.material_id)
    transitByMaterial.set(materialId, (transitByMaterial.get(materialId) ?? 0) + Number(purchase.qty_grams ?? 0))
  }
  const reservedByMaterial = new Map<number, number>()
  const orderReservedByMaterial = new Map<number, number>()
  for (const reservation of browserDb.stock_reservations ?? []) {
    if (String(reservation.status ?? '') !== 'active') continue
    const materialId = Number(reservation.material_id)
    const reserved = Number(reservation.reserved_grams ?? 0)
    reservedByMaterial.set(materialId, (reservedByMaterial.get(materialId) ?? 0) + reserved)
    if (Number(reservation.order_id) === Number(delivery.order_id)) orderReservedByMaterial.set(materialId, (orderReservedByMaterial.get(materialId) ?? 0) + reserved)
  }
  const pendingByMaterial = new Map<number, number>()
  for (const item of items) {
    const orderItem = browserDb.order_items.find((row) => Number(row.id) === Number(item.order_item_id))
    if (!orderItem || Number(orderItem.order_id) !== Number(delivery.order_id)) throw new Error('送货明细对应的订单明细不存在')
    const materialId = Number(orderItem.material_id)
    const qty = Number(item.qty_grams ?? 0)
    const onHand = (baseOnHand.get(materialId) ?? 0) - (pendingByMaterial.get(materialId) ?? 0)
    const available = onHand + (transitByMaterial.get(materialId) ?? 0) - (reservedByMaterial.get(materialId) ?? 0)
    const allowance = available + (orderReservedByMaterial.get(materialId) ?? 0)
    if (!Number.isFinite(qty) || qty <= 0 || qty > allowance) throw new Error('库存可用量不足，不能登记送货出库')
    pendingByMaterial.set(materialId, (pendingByMaterial.get(materialId) ?? 0) + qty)
  }
  browserDb.inventory_movements = (browserDb.inventory_movements ?? []).filter((row) => !sourceIds.includes(Number(row.source_id)) || row.movement_type !== 'sale_issue')
  const order = browserDb.orders.find((row) => Number(row.id) === Number(delivery.order_id))
  let movementId = nextBrowserId('inventory_movements')
  for (const item of items) {
    const orderItem = browserDb.order_items.find((row) => row.id === Number(item.order_item_id))
    if (!orderItem) continue
    browserDb.inventory_movements.unshift({ id: movementId++, material_id: Number(orderItem.material_id), material_code: orderItem.material_code, movement_type: 'sale_issue', qty_grams: Number(item.qty_grams), batch: String(item.batch ?? orderItem.batch ?? ''), warehouse: '', unit_cost_cents: 0, source_id: Number(item.id), created_at: new Date().toISOString() })
  }
  if (order) {
    for (const reservation of browserDb.stock_reservations ?? []) {
      if (Number(reservation.order_id) === Number(order.id) && String(reservation.status ?? '') === 'active') reservation.status = 'released'
    }
    if (String(delivery.sign_date ?? '')) {
      if (['preparing', 'delivered', 'signed'].includes(String(order.status ?? ''))) {
        order.status = 'signed'
        order.sign_status = 'signed'
      }
    } else if (['preparing', 'pending_confirm'].includes(String(order.status ?? ''))) {
      order.status = 'delivered'
    }
  }
  refreshBrowserInventory()
}

function ensureBrowserDeliveryItemsFromOrder(deliveryId: number) {
  const delivery = browserDb.deliveries.find((row) => row.id === deliveryId)
  if (!delivery) throw new Error('送货单不存在')
  const orderId = Number(delivery.order_id)
  const order = browserDb.orders.find((row) => Number(row.id) === orderId)
  if (!order) throw new Error('订单不存在')
  const deliveryItems = browserDb.delivery_items ?? []
  if (deliveryItems.some((row) => Number(row.delivery_id) === deliveryId)) return
  let orderItems = (browserDb.order_items ?? []).filter((row) => Number(row.order_id) === orderId)
  if (!orderItems.length) {
    const fallback = {
      id: nextBrowserId('order_items'), order_id: orderId, order_no: order.no,
      material_id: order.material_id, material_code: order.material_code, batch: '', qty_grams: order.qty_grams,
      unit_price_cents: order.price_cents, amount_cents: orderAmountCents(order),
    }
    browserDb.order_items.push(fallback)
    orderItems = [fallback]
  }
  let nextId = nextBrowserId('delivery_items')
  const generated: EntityRecord[] = []
  for (const orderItem of orderItems) {
    const orderedQty = Number(orderItem.qty_grams ?? 0)
    const deliveredElsewhere = deliveryItems
      .filter((row) => Number(row.order_item_id) === Number(orderItem.id) && Number(row.delivery_id) !== deliveryId)
      .reduce((sum, row) => sum + Number(row.qty_grams ?? 0), 0)
    const remaining = orderedQty - deliveredElsewhere
    if (remaining > 0) generated.push({
      id: nextId++, delivery_id: deliveryId, order_item_id: Number(orderItem.id),
      batch: String(orderItem.batch ?? ''), qty_grams: remaining,
    })
  }
  if (!generated.length) throw new Error('订单已全部送货，不能生成空送货单')
  browserDb.delivery_items.push(...generated)
}

function saveBrowserDocumentItems(documentType: string, documentId: number, rawItems: DocumentItemRecord[]) {
  const mapping = documentEntityMap[documentType]
  if (!mapping) throw new Error(`不支持的明细单据类型：${documentType}`)
  const document = (browserDb[documentType as EntityName] ?? []).find((row) => row.id === documentId)
  if (!document) throw new Error('单据不存在')
  if (!rawItems.length) throw new Error('至少需要一条明细')
  const existing = browserDb[mapping.entity] ?? []
  let oldMovementSources: number[] = []
  if (documentType === 'orders') {
    const oldIds = existing.filter((row) => Number(row.order_id) === documentId).map((row) => Number(row.id))
    if ((browserDb.delivery_items ?? []).some((row) => oldIds.includes(Number(row.order_item_id)))) throw new Error('订单已有送货明细，不能整体替换订单明细')
  }
  const normalized: EntityRecord[] = []
  let nextId = nextBrowserId(mapping.entity)
  if (documentType === 'quotations' || documentType === 'orders') {
    for (const item of rawItems) {
      const materialId = Number(item.material_id)
      const qty = Number(item.qty_grams)
      const unitPrice = Number(item.unit_price_cents ?? item.price_cents ?? 0)
      if (!Number.isInteger(materialId) || materialId <= 0 || !browserDb.materials.some((row) => row.id === materialId)) throw new Error('明细牌号不存在')
      if (!Number.isFinite(qty) || qty <= 0 || !Number.isFinite(unitPrice) || unitPrice <= 0) throw new Error('明细数量和单价必须大于 0')
      normalized.push({ id: nextId++, [mapping.key]: documentId, material_id: materialId, material_code: browserDb.materials.find((row) => row.id === materialId)?.code ?? '', batch: String(item.batch ?? '').trim(), qty_grams: Math.round(qty), unit_price_cents: Math.round(unitPrice), amount_cents: lineAmountCents(Math.round(qty), Math.round(unitPrice)) })
    }
    if (documentType === 'quotations') {
      for (let index = 0; index < rawItems.length; index += 1) {
        Object.assign(normalized[index], {
          material_category: String(rawItems[index].material_category ?? '').trim(),
          material_grade: String(rawItems[index].material_grade ?? '').trim(),
          manufacturer: String(rawItems[index].manufacturer ?? '').trim(),
          note: String(rawItems[index].note ?? '').trim(),
        })
      }
    }
  } else if (documentType === 'contracts') {
    for (const item of rawItems) {
      const materialId = Number(item.material_id ?? 0)
      if (materialId > 0 && !browserDb.materials.some((row) => Number(row.id) === materialId)) throw new Error('合同明细牌号不存在')
      const model = String(item.model ?? '').trim()
      const materialName = String(item.material_name ?? '').trim() || (materialId > 0 ? String(browserDb.materials.find((row) => Number(row.id) === materialId)?.code ?? '') : '')
      const qty = Number(item.qty_grams)
      const unitPrice = Number(item.unit_price_cents ?? item.price_cents ?? 0)
      if (!model || !materialName || !Number.isFinite(qty) || qty <= 0 || !Number.isFinite(unitPrice) || unitPrice <= 0) throw new Error('请完整填写合同明细的材料、型号、数量和单价')
      normalized.push({ id: nextId++, [mapping.key]: documentId, material_id: materialId > 0 ? materialId : null, material_name: materialName, model, manufacturer: String(item.manufacturer ?? '').trim(), note: String(item.note ?? '').trim(), qty_grams: Math.round(qty), unit_price_cents: Math.round(unitPrice), amount_cents: lineAmountCents(Math.round(qty), Math.round(unitPrice)) })
    }
  } else if (documentType === 'deliveries') {
    const orderId = Number(document.order_id)
    const orderItems = (browserDb.order_items ?? []).filter((row) => Number(row.order_id) === orderId)
    if (!orderItems.length) {
      const order = browserDb.orders.find((row) => row.id === orderId)
      if (order) {
        const fallback = { id: nextBrowserId('order_items'), order_id: orderId, order_no: order.no, material_id: order.material_id, material_code: order.material_code, batch: '', qty_grams: order.qty_grams, unit_price_cents: order.price_cents, amount_cents: orderAmountCents(order) }
        browserDb.order_items.push(fallback)
        orderItems.push(fallback)
      }
    }
    const oldDeliveryIds = new Set((browserDb.delivery_items ?? []).filter((row) => Number(row.delivery_id) === documentId).map((row) => Number(row.id)))
    oldMovementSources = (browserDb.inventory_movements ?? []).filter((row) => row.movement_type === 'sale_issue' && oldDeliveryIds.has(Number(row.source_id))).map((row) => Number(row.source_id))
    for (const item of rawItems) {
      const orderItem = orderItems.find((row) => Number(row.id) === Number(item.order_item_id))
      if (!orderItem) throw new Error('送货明细对应的订单明细不存在')
      const qty = Math.round(Number(item.qty_grams))
      const deliveredElsewhere = (browserDb.delivery_items ?? []).filter((row) => Number(row.order_item_id) === Number(orderItem.id) && !oldDeliveryIds.has(Number(row.id))).reduce((sum, row) => sum + Number(row.qty_grams ?? 0), 0)
      if (qty <= 0 || qty > Number(orderItem.qty_grams) || deliveredElsewhere + qty > Number(orderItem.qty_grams)) throw new Error('送货数量不能超过订单明细剩余数量')
      normalized.push({ id: nextId++, delivery_id: documentId, order_item_id: Number(orderItem.id), batch: String(item.batch ?? orderItem.batch ?? '').trim(), qty_grams: qty })
    }
  } else if (documentType === 'statements') {
    const customerId = Number(document.customer_id)
    const start = String(document.period_start ?? '')
    const end = String(document.period_end ?? '')
    const localOrderTotals = new Map<number, number>()
    for (const item of rawItems) {
      const orderId = Number(item.order_id)
      const order = browserDb.orders.find((row) => row.id === orderId)
      if (!order || Number(order.customer_id) !== customerId) throw new Error('对账明细客户必须与对账单一致')
      const date = String(order.delivery_date ?? '')
      if (date && (date < start || date > end)) throw new Error('订单交期不在对账期间内')
      const allocated = Math.round(Number(item.allocated_amount_cents ?? item.amount_cents))
      const orderTotal = browserOrderTotal(orderId)
      const other = (browserDb.statement_items ?? []).filter((row) => Number(row.order_id) === orderId && Number(row.statement_id) !== documentId).reduce((sum, row) => sum + Number(row.allocated_amount_cents ?? 0), 0)
      const local = (localOrderTotals.get(orderId) ?? 0) + allocated
      if (allocated <= 0 || local + other > orderTotal) throw new Error('对账分配金额不能超过订单未对账金额')
      localOrderTotals.set(orderId, local)
      normalized.push({ id: nextId++, statement_id: documentId, statement_no: document.no, order_id: orderId, order_no: order.no, allocated_amount_cents: allocated })
    }
  }
  browserDb[mapping.entity] = existing.filter((row) => Number(row[mapping.key]) !== documentId).concat(normalized)
  syncBrowserDocumentHeader(documentType, documentId)
  if (documentType === 'statements') refreshBrowserStatement(documentId)
  if (documentType === 'deliveries') refreshBrowserDeliveryMovements(documentId, [...oldMovementSources])
  saveBrowserDb(browserDb)
  return browserDocumentItems(documentType, documentId)
}

function browserStatementTotal(statementId: number) {
  const statement = browserDb.statements.find((row) => Number(row.id) === statementId)
  if (!statement) return 0
  const items = browserDocumentItems('statements', statementId)
  if (items.length) return items.reduce((sum, item) => sum + Number(item.allocated_amount_cents ?? 0), 0)
  const start = String(statement.period_start ?? '')
  const end = String(statement.period_end ?? '')
  const customerId = Number(statement.customer_id)
  return (browserDb.orders ?? [])
    .filter((order) => Number(order.customer_id) === customerId && String(order.delivery_date ?? '') >= start && String(order.delivery_date ?? '') <= end)
    .reduce((sum, order) => sum + browserOrderTotal(Number(order.id)), 0)
}

function browserPaymentContribution(payment: EntityRecord, statementId: number) {
  const paymentId = Number(payment.id)
  const allocations = (browserDb.payment_allocations ?? []).filter((item) => Number(item.payment_id) === paymentId)
  if (allocations.length) {
    return allocations.filter((item) => Number(item.statement_id) === statementId).reduce((sum, item) => sum + Number(item.allocated_amount_cents ?? 0), 0)
  }
  return Number(payment.statement_id) === statementId ? Number(payment.amount_cents ?? 0) : 0
}

function browserStatementReceived(statementId: number, excludedPaymentId = 0) {
  return (browserDb.payments ?? []).reduce((sum, payment) => {
    if (Number(payment.id) === excludedPaymentId) return sum
    return sum + browserPaymentContribution(payment, statementId)
  }, 0)
}

function saveBrowserPaymentAllocations(paymentId: number, rawItems: DocumentItemRecord[]) {
  const payment = browserDb.payments.find((row) => row.id === paymentId)
  if (!payment) throw new Error('回款记录不存在')
  if (!rawItems.length) throw new Error('至少需要一条回款分配')
  const amount = Number(payment.amount_cents ?? 0)
  if (!Number.isSafeInteger(amount) || amount <= 0) throw new Error('回款金额必须大于 0')
  let total = 0
  let nextId = nextBrowserId('payment_allocations')
  const normalized: EntityRecord[] = []
  const seenStatements = new Set<number>()
  for (const item of rawItems) {
    const statementId = Number(item.statement_id)
    const statement = browserDb.statements.find((row) => row.id === statementId)
    const rawAllocated = Number(item.allocated_amount_cents ?? item.amount_cents)
    const allocated = Math.round(rawAllocated)
    if (!statement) throw new Error('回款分配的对账单不存在')
    if (seenStatements.has(statementId)) throw new Error('同一回款不能重复分配到同一张对账单')
    if (!Number.isSafeInteger(rawAllocated) || !Number.isSafeInteger(allocated) || allocated <= 0) throw new Error('分配金额必须大于 0')
    const statementTotal = browserStatementTotal(statementId)
    const receivedExcludingCurrent = browserStatementReceived(statementId, paymentId)
    const available = Math.max(0, statementTotal - receivedExcludingCurrent)
    if (allocated > available) throw new Error(`对账单可收余额不足，不能分配 ${allocated} 分（当前可收 ${available} 分）`)
    seenStatements.add(statementId)
    total += allocated
    if (!Number.isSafeInteger(total)) throw new Error('分配金额超出范围')
    normalized.push({ id: nextId++, payment_id: paymentId, payment_amount_cents: amount, pay_date: payment.pay_date, statement_id: statementId, statement_no: statement.no, allocated_amount_cents: allocated })
  }
  if (total !== amount) throw new Error(`回款分配合计必须等于回款金额（当前 ${total} 分，回款 ${amount} 分）`)
  const affected = new Set((browserDb.payment_allocations ?? []).filter((row) => Number(row.payment_id) === paymentId).map((row) => Number(row.statement_id)))
  normalized.forEach((row) => affected.add(Number(row.statement_id)))
  browserDb.payment_allocations = (browserDb.payment_allocations ?? []).filter((row) => Number(row.payment_id) !== paymentId).concat(normalized)
  const first = normalized[0]
  payment.statement_id = first.statement_id
  payment.statement_no = first.statement_no
  for (const statementId of affected) refreshBrowserStatement(statementId)
  saveBrowserDb(browserDb)
  return browserDocumentItems('payments', paymentId)
}

function enrichBrowserRow(entity: EntityName, row: EntityRecord): EntityRecord {
  if (entity === 'follow_ups') {
    const customer = browserDb.customers.find((item) => item.id === Number(row.customer_id))
    return { ...row, customer_name: customer?.name ?? '已删除客户' }
  }
  if (entity === 'sample_tests') {
    const sample = browserDb.samples.find((item) => item.id === Number(row.sample_id))
    const customer = browserDb.customers.find((item) => item.id === Number(sample?.customer_id))
    return { ...row, sample_code: sample?.code ?? '已删除样品', customer_name: customer?.name ?? '已删除客户' }
  }
  if (entity === 'contracts') {
    const items = browserDocumentItems(entity, Number(row.id))
    return { ...row, item_count: items.length, item_total_cents: items.reduce((sum, item) => sum + Number(item.amount_cents ?? 0), 0) }
  }
  if (entity === 'quotations' || entity === 'orders') {
    const items = browserDocumentItems(entity, Number(row.id))
    const customer = browserDb.customers.find((item) => Number(item.id) === Number(row.customer_id))
    const material = browserDb.materials.find((item) => Number(item.id) === Number(row.material_id))
    return {
      ...row,
      customer_name: row.customer_name ?? customer?.name,
      material_code: row.material_code ?? material?.code,
      item_count: items.length,
      ...(entity === 'quotations' ? { item_total_cents: items.reduce((sum, item) => sum + Number(item.amount_cents ?? 0), 0) } : { amount_cents: items.length ? items.reduce((sum, item) => sum + Number(item.amount_cents ?? 0), 0) : orderAmountCents(row) }),
    }
  }
  if (entity === 'deliveries') {
    const order = browserDb.orders.find((item) => Number(item.id) === Number(row.order_id))
    const customer = browserDb.customers.find((item) => Number(item.id) === Number(order?.customer_id))
    return { ...row, order_no: row.order_no ?? order?.no, customer_name: row.customer_name ?? customer?.name }
  }
  if (entity === 'statements') {
    const customer = browserDb.customers.find((item) => Number(item.id) === Number(row.customer_id))
    return { ...row, customer_name: row.customer_name ?? customer?.name }
  }
  if (entity === 'purchases') {
    const supplier = browserDb.suppliers.find((item) => Number(item.id) === Number(row.supplier_id))
    const material = browserDb.materials.find((item) => Number(item.id) === Number(row.material_id))
    return { ...row, supplier_name: row.supplier_name ?? supplier?.name, material_code: row.material_code ?? material?.code }
  }
  if (entity === 'stock_reservations') {
    const material = browserDb.materials.find((item) => Number(item.id) === Number(row.material_id))
    const order = browserDb.orders.find((item) => Number(item.id) === Number(row.order_id))
    return { ...row, material_code: row.material_code ?? material?.code, order_no: row.order_no ?? order?.no }
  }
  if (entity === 'inventory_movements' || entity === 'cost_history') {
    const material = browserDb.materials.find((item) => Number(item.id) === Number(row.material_id))
    return { ...row, material_code: row.material_code ?? material?.code }
  }
  if (entity === 'inventory') {
    refreshBrowserInventory(Number(row.material_id))
    const material = browserDb.materials.find((item) => Number(item.id) === Number(row.material_id))
    return { ...row, material_code: row.material_code ?? material?.code }
  }
  if (entity === 'payments') {
    const allocations = browserDocumentItems('payments', Number(row.id))
    const statementNos = allocations.map((item) => String(item.statement_no ?? '')).filter(Boolean)
    return { ...row, allocation_statements: statementNos.join('、') || row.statement_no }
  }
  if (entity === 'attachments') {
    return { ...row, exists: true, size_bytes: Number(row.size_bytes ?? 0), integrity: 'demo_only', encrypted: false }
  }
  return row
}

function latestBrowserFollowUps() {
  const latest = new Map<number, EntityRecord>()
  const ordered = [...browserDb.follow_ups].sort((left, right) => {
    const byDate = String(right.follow_date ?? '').localeCompare(String(left.follow_date ?? ''))
    return byDate || Number(right.id) - Number(left.id)
  })
  for (const record of ordered) {
    const customerId = Number(record.customer_id)
    if (!latest.has(customerId)) latest.set(customerId, record)
  }
  return [...latest.values()]
}

function refreshBrowserCustomerFollowDate(customerId: number) {
  const customer = browserDb.customers.find((item) => item.id === customerId)
  const latest = latestBrowserFollowUps().find((record) => Number(record.customer_id) === customerId)
  if (customer) customer.next_follow_date = latest?.next_date ?? null
}

function browserDictLabel(type: string, value: string) {
  const label = browserDicts.find((row) => String(row.type) === type && String(row.value) === value)?.label
  return String(label ?? value).trim() || value
}

const browserDb = loadBrowserDb()
refreshBrowserInventory()
for (const statement of browserDb.statements ?? []) refreshBrowserStatement(Number(statement.id))
saveBrowserDb(browserDb)
let browserProtectionEnabled = false
const browserBackups: BackupEntry[] = []
const browserRestoreDrills: RestoreDrillRecord[] = []
const browserAttachmentObjectTypes = new Set<EntityName>([
  'customers', 'follow_ups', 'contacts', 'projects', 'materials', 'samples', 'sample_tests', 'contracts', 'contract_items', 'quotations', 'orders', 'deliveries', 'statements', 'payments', 'suppliers', 'purchases', 'inventory',
  'quotation_items', 'order_items', 'delivery_items', 'statement_items', 'payment_allocations', 'inventory_movements', 'stock_reservations', 'cost_history',
])
const browserMaxAttachmentBytes = 25 * 1024 * 1024

async function browserAttachmentHash(bytes: number[]) {
  const cryptoApi = globalThis.crypto
  if (cryptoApi?.subtle) {
    const digest = await cryptoApi.subtle.digest('SHA-256', new Uint8Array(bytes))
    return Array.from(new Uint8Array(digest), (value) => value.toString(16).padStart(2, '0')).join('')
  }
  // The browser demo never writes a file; this deterministic fallback only keeps
  // the metadata shape usable in environments without Web Crypto (for example jsdom).
  let state = 2166136261
  for (const value of bytes) state = Math.imul(state ^ value, 16777619) >>> 0
  return state.toString(16).padStart(8, '0').repeat(8)
}

function browserAttachmentName(fileName: string) {
  const candidate = fileName.trim().replace(/\\/g, '/').split('/').pop() || ''
  const cleaned = candidate.replace(/[<>:"/\\|?*\u0000-\u001f]/g, '_').replace(/^[ .]+|[ .]+$/g, '').slice(0, 120)
  if (!cleaned) throw new Error('附件文件名不能为空')
  return cleaned
}

export function isTauriRuntime() {
  return Boolean((window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__)
}

const trackedCommands: Record<string, { label: string; kind: BackgroundTaskKind }> = {
  initialize: { label: '启动与完整性检查', kind: 'startup' },
  save_settings: { label: '保存设置', kind: 'write' },
  get_sync_status: { label: '读取同步状态', kind: 'read' },
  list_pending_sync_events: { label: '读取待同步数据', kind: 'read' },
  mark_sync_events_synced: { label: '确认云端同步', kind: 'write' },
  mark_sync_events_failed: { label: '记录同步失败', kind: 'write' },
  save_dict: { label: '保存状态字典', kind: 'write' },
  save_entity: { label: '保存业务记录', kind: 'write' },
  duplicate_quotation: { label: '复制报价版本', kind: 'write' },
  compare_quotations: { label: '比较报价版本', kind: 'read' },
  convert_quotation_to_order: { label: '报价转订单', kind: 'write' },
  save_document_items: { label: '保存单据明细', kind: 'write' },
  save_payment_allocations: { label: '保存回款分配', kind: 'write' },
  delete_entity: { label: '归档业务记录', kind: 'write' },
  transition_entity: { label: '更新业务状态', kind: 'write' },
  list_attachments: { label: '检查附件完整性', kind: 'attachment' },
  save_attachment: { label: '保存附件', kind: 'attachment' },
  download_attachment: { label: '读取附件', kind: 'attachment' },
  download_template: { label: '生成导入模板', kind: 'export' },
  backup_now: { label: '创建账套备份', kind: 'backup' },
  list_backups: { label: '检查备份文件', kind: 'read' },
  read_backup_bytes: { label: '读取云端备份', kind: 'backup' },
  stage_cloud_backup: { label: '保存云端备份', kind: 'backup' },
  shutdown: { label: '退出前备份与关闭', kind: 'backup' },
  export_entity: { label: '导出 CSV', kind: 'export' },
  import_file: { label: '导入 CSV', kind: 'import' },
  restore_backup: { label: '还原账套备份', kind: 'restore' },
  integrity_check_command: { label: '运行数据库完整性检查', kind: 'integrity' },
  run_restore_drill: { label: '运行恢复演练', kind: 'restore' },
  get_operational_metrics: { label: '读取运营评估', kind: 'read' },
  set_protection: { label: '迁移保护模式附件', kind: 'protection' },
  disable_protection: { label: '迁移未保护附件', kind: 'protection' },
}

const businessWriteCommands = new Set([
  'save_dict', 'save_entity', 'duplicate_quotation', 'convert_quotation_to_order',
  'save_document_items', 'save_payment_allocations', 'delete_entity', 'transition_entity',
])

function recordBrowserSyncEvent(name: string, args: Record<string, unknown>, result: unknown) {
  if (browserSettings.cloud_sync_enabled !== '1') return
  const resultRecord = result && typeof result === 'object' ? result as Record<string, unknown> : {}
  const entityType = String(args.entity ?? args.documentType ?? args.document_type ?? name)
  const entityId = Number(resultRecord.id ?? args.id ?? args.documentId ?? args.document_id ?? 0)
  browserSyncEvents.push({
    local_id: Math.max(0, ...browserSyncEvents.map((item) => item.local_id)) + 1,
    event_id: typeof crypto.randomUUID === 'function' ? crypto.randomUUID() : `event-${Date.now()}-${Math.random().toString(16).slice(2)}`,
    entity_type: entityType,
    entity_id: Number.isFinite(entityId) ? entityId : 0,
    operation: name,
    payload: { args, result: resultRecord },
    created_at: new Date().toISOString(),
    retry_count: 0,
    last_error: '',
  })
  saveBrowserSyncEvents()
}

export async function command<T>(name: string, args: Record<string, unknown> = {}): Promise<T> {
  const call = () => isTauriRuntime() ? invoke<T>(name, args) : browserCommand<T>(name, args)
  const tracked = trackedCommands[name]
  const result = tracked ? await runTrackedTask(tracked.label, call, tracked.kind) : await call()
  if (businessWriteCommands.has(name)) {
    if (!isTauriRuntime()) recordBrowserSyncEvent(name, args, result)
    window.dispatchEvent(new CustomEvent('erp:local-business-write'))
  }
  return result
}

async function browserCommand<T>(name: string, args: Record<string, unknown>): Promise<T> {
  if (name === 'initialize') {
    if (browserProtectionEnabled && !String(args.password ?? '').trim()) throw new Error('PROTECTED_LOCKED')
    return ({ app_version: DEMO_APP_VERSION, schema_version: 9, root: '浏览器演示存储', previous_exit_dirty: false, previous_exit_time: null, startup_integrity: 'demo_only', recovery_log_path: '浏览器演示不写入日志', protection_enabled: browserProtectionEnabled, close_behavior: browserSettings.close_behavior === 'confirm_exit' ? 'confirm_exit' : 'minimize_to_tray' } as T)
  }
  if (name === 'mark_ui_ready') return ({ ready_at: new Date().toISOString(), app_version: DEMO_APP_VERSION } as T)
  if (name === 'list_settings') {
    return Object.entries(browserSettings).map(([key, value]) => ({ key, value, updated_at: new Date().toISOString() })) as T
  }
  if (name === 'save_settings') {
    const key = String(args.key ?? '').trim()
    const value = browserSettingValue(key, args.value)
    browserSettings[key] = value
    localStorage.setItem(browserSettingsKey, JSON.stringify(browserSettings))
    return ({ key, value, updated_at: new Date().toISOString() } as T)
  }
  if (name === 'get_sync_status') {
    const pending = browserSyncEvents.filter((event) => !event.synced_at)
    return ({
      enabled: browserSettings.cloud_sync_enabled === '1',
      pending_count: pending.length,
      failed_count: pending.filter((event) => event.last_error).length,
      last_synced_at: browserSyncEvents.map((event) => event.synced_at).filter(Boolean).sort().at(-1) ?? null,
    } as T)
  }
  if (name === 'list_pending_sync_events') {
    const limit = Math.max(1, Math.min(500, Number(args.limit ?? 100)))
    return browserSyncEvents.filter((event) => !event.synced_at).slice(0, limit) as T
  }
  if (name === 'mark_sync_events_synced') {
    const ids = new Set((args.localIds ?? args.local_ids ?? []) as number[])
    const syncedAt = new Date().toISOString()
    let updated = 0
    browserSyncEvents = browserSyncEvents.map((event) => {
      if (!ids.has(event.local_id) || event.synced_at) return event
      updated += 1
      return { ...event, synced_at: syncedAt, last_error: '' }
    })
    saveBrowserSyncEvents()
    return ({ updated, synced_at: syncedAt } as T)
  }
  if (name === 'mark_sync_events_failed') {
    const ids = new Set((args.localIds ?? args.local_ids ?? []) as number[])
    const error = String(args.error ?? '').slice(0, 500)
    let updated = 0
    browserSyncEvents = browserSyncEvents.map((event) => {
      if (!ids.has(event.local_id) || event.synced_at) return event
      updated += 1
      return { ...event, retry_count: event.retry_count + 1, last_error: error }
    })
    saveBrowserSyncEvents()
    return ({ updated } as T)
  }
  if (name === 'list_dicts') {
    const wanted = String(args.dictType ?? args.dict_type ?? '').trim()
    return browserDicts.filter((row) => !wanted || String(row.type ?? '') === wanted).map((row) => ({ ...row })) as T
  }
  if (name === 'save_dict') {
    const input = (args.data ?? {}) as EntityRecord
    const id = Number(input.id ?? 0)
    const type = String(input.type ?? '').trim()
    const value = String(input.value ?? '').trim()
    const label = String(input.label ?? '').trim()
    if (!type || !value || !label) throw new Error('字典类型、code 和显示名不能为空')
    if (type.length > 64 || value.length > 128 || label.length > 128) throw new Error('字典类型、code 或显示名过长')
    const sortOrder = Math.max(0, Math.round(Number(input.sort_order ?? 0)))
    const enabled = input.enabled === false || input.enabled === 0 || String(input.enabled ?? '').toLowerCase() === 'false' ? false : true
    const existing = id > 0 ? browserDicts.find((row) => Number(row.id) === id) : undefined
    if (id > 0 && !existing) throw new Error('字典记录不存在')
    if (existing && (String(existing.type) !== type || String(existing.value) !== value)) throw new Error('字典 code 不可修改；请停用旧项后新增新 code')
    const duplicate = browserDicts.find((row) => Number(row.id) !== id && String(row.type) === type && String(row.value) === value)
    if (duplicate) throw new Error('字典 code 已存在')
    const record = { id: id || Math.max(0, ...browserDicts.map((row) => Number(row.id) || 0)) + 1, type, value, label, sort_order: sortOrder, enabled }
    if (existing) Object.assign(existing, record)
    else browserDicts.unshift(record)
    localStorage.setItem(browserDictsKey, JSON.stringify(browserDicts))
    return record as T
  }
  if (name === 'shutdown') return ({ backup_path: '浏览器演示存储', status: 'clean' } as T)
  if (name === 'list_background_tasks') return [] as T
  if (name === 'list_entities_page') {
    const entity = args.entity as EntityName
    const search = String(args.search ?? '').trim().toLowerCase()
    const status = String(args.status ?? '').trim()
    const page = Math.max(1, Number(args.page ?? 1) || 1)
    const pageSize = Math.min(100, Math.max(1, Number(args.pageSize ?? 20) || 20))
    refreshBrowserInventory()
    const filtered = (browserDb[entity] ?? []).map((row) => enrichBrowserRow(entity, row)).filter((row) => {
      const haystack = Object.values(row).join(' ').toLowerCase()
      const rowStatus = entity === 'customers' || entity === 'projects' ? row.stage : row.status ?? row.pay_status
      return (!search || haystack.includes(search)) && (!status || String(rowStatus ?? '') === status)
    })
    const start = (page - 1) * pageSize
    return { rows: filtered.slice(start, start + pageSize), total: filtered.length, page, page_size: pageSize } as T
  }
  if (name === 'list_entities') {
    const entity = args.entity as EntityName
    const search = String(args.search ?? '').trim().toLowerCase()
    const status = String(args.status ?? '').trim()
    refreshBrowserInventory()
    const rows = (browserDb[entity] ?? []).map((row) => enrichBrowserRow(entity, row))
    return rows.filter((row) => {
      const haystack = Object.values(row).join(' ').toLowerCase()
      const rowStatus = entity === 'customers' || entity === 'projects' ? row.stage : row.status ?? row.pay_status
      return (!search || haystack.includes(search)) && (!status || String(rowStatus ?? '') === status)
    }) as T
  }
  if (name === 'list_attachments') {
    const objectType = String(args.objectType ?? args.object_type ?? '').trim()
    const objectId = Number(args.objectId ?? args.object_id ?? 0)
    if (objectType && !browserAttachmentObjectTypes.has(objectType as EntityName)) throw new Error(`不支持为该对象登记附件：${objectType}`)
    return (browserDb.attachments ?? []).filter((row) => (!objectType || String(row.object_type) === objectType) && (!objectId || Number(row.object_id) === objectId)).map((row) => ({
      ...row,
      exists: true,
      size_bytes: Number(row.size_bytes ?? 0),
      integrity: 'demo_only',
      encrypted: false,
    })) as T
  }
  if (name === 'save_attachment') {
    const objectType = String(args.objectType ?? args.object_type ?? '').trim() as EntityName
    const objectId = Number(args.objectId ?? args.object_id ?? 0)
    if (!browserAttachmentObjectTypes.has(objectType) || !Number.isInteger(objectId) || objectId <= 0) throw new Error('附件关联对象无效')
    if (!(browserDb[objectType] ?? []).some((row) => Number(row.id) === objectId)) throw new Error('附件关联对象不存在')
    const rawBytes = Array.isArray(args.bytes) ? args.bytes.map((value) => Number(value)) : []
    if (!rawBytes.length) throw new Error('附件不能为空')
    if (rawBytes.length > browserMaxAttachmentBytes) throw new Error('附件大小不能超过 25 MB')
    if (rawBytes.some((value) => !Number.isInteger(value) || value < 0 || value > 255)) throw new Error('附件数据格式无效')
    const safeName = browserAttachmentName(String(args.fileName ?? args.file_name ?? ''))
    const hash = await browserAttachmentHash(rawBytes)
    const existing = (browserDb.attachments ?? []).find((row) => String(row.object_type) === objectType && Number(row.object_id) === objectId && String(row.hash) === hash)
    if (existing) return { ...existing, exists: true, size_bytes: Number(existing.size_bytes ?? rawBytes.length), integrity: 'demo_only', encrypted: false, deduplicated: true, demo_only: true } as T
    const id = Math.max(0, ...(browserDb.attachments ?? []).map((row) => Number(row.id) || 0)) + 1
    const record: AttachmentRecord = {
      id,
      object_type: objectType,
      object_id: objectId,
      relative_path: `files/${objectType}/${objectId}/demo-${Date.now()}-${hash.slice(0, 12)}-${safeName}`,
      hash,
      created_at: new Date().toISOString(),
      exists: true,
      size_bytes: rawBytes.length,
      integrity: 'demo_only',
      encrypted: false,
      file_name: safeName,
      deduplicated: false,
      demo_only: true,
    }
    browserDb.attachments.unshift(record)
    saveBrowserDb(browserDb)
    return record as T
  }
  if (name === 'download_attachment') throw new Error('浏览器演示模式不保存附件文件，无法下载')
  if (name === 'duplicate_quotation') {
    const before = structuredClone(browserDb)
    try {
      return duplicateBrowserQuotation(Number(args.quotation_id ?? args.quotationId ?? 0)) as T
    } catch (error) {
      restoreBrowserDb(before)
      throw error
    }
  }
  if (name === 'compare_quotations') {
    return browserQuotationComparison(Number(args.left_id ?? args.leftId ?? 0), Number(args.right_id ?? args.rightId ?? 0)) as T
  }
  if (name === 'convert_quotation_to_order') {
    const before = structuredClone(browserDb)
    try {
      return convertBrowserQuotation(Number(args.quotation_id ?? args.quotationId ?? 0), String(args.order_no ?? args.orderNo ?? ''), String(args.delivery_date ?? args.deliveryDate ?? '')) as T
    } catch (error) {
      restoreBrowserDb(before)
      throw error
    }
  }
  if (name === 'save_entity') {
    const before = structuredClone(browserDb)
    try {
      const entity = args.entity as EntityName
      if (['quotation_items', 'order_items', 'delivery_items', 'statement_items', 'payment_allocations', 'inventory_movements', 'stock_reservations', 'attachments', 'cost_history'].includes(entity) && ['inventory_movements', 'stock_reservations', 'attachments', 'cost_history'].includes(entity)) throw new Error('该记录由业务流程维护，不能在浏览器演示模式直接编辑')
      const input = (args.data ?? {}) as EntityRecord
      const rows = browserDb[entity] ?? []
      const id = Number(input.id ?? 0) || Math.max(0, ...rows.map((item) => item.id)) + 1
      const existing = rows.find((item) => item.id === id)
      if (entity === 'follow_ups') {
        const customerId = Number(input.customer_id)
        if (!Number.isInteger(customerId) || customerId <= 0 || !browserDb.customers.some((item) => item.id === customerId)) throw new Error('客户必须存在')
        if (!String(input.content ?? '').trim()) throw new Error('跟进内容不能为空')
        if (!String(input.follow_date ?? '').trim()) throw new Error('跟进日期不能为空')
      }
      if (entity === 'sample_tests') {
        const sampleId = Number(input.sample_id)
        if (!Number.isInteger(sampleId) || sampleId <= 0 || !browserDb.samples.some((item) => item.id === sampleId)) throw new Error('样品必须存在')
        if (!String(input.test_date ?? '').trim()) throw new Error('测试日期不能为空')
        if (!String(input.test_item ?? '').trim()) throw new Error('测试项目不能为空')
      }
      if (entity === 'contracts') {
        if (!String(input.no ?? '').trim()) throw new Error('合同号不能为空')
        if (!String(input.seller_name ?? '').trim() || !String(input.buyer_name ?? '').trim()) throw new Error('供方和需方不能为空')
        if (!String(input.contract_date ?? '').trim()) throw new Error('合同日期不能为空')
      }
      const record: EntityRecord = { ...input, id }
      delete record.items
      if (entity === 'orders') record.amount_cents = orderAmountCents(record)
      if (entity === 'inventory') {
        const materialId = Number(record.material_id)
        if (!materialId || !browserDb.materials.some((item) => item.id === materialId)) throw new Error('牌号必须存在')
        const requested = Math.max(0, Number(record.on_hand_grams ?? 0))
        record.opening_grams = requested - movementNet(materialId)
        record.on_hand_grams = requested
        record.in_transit_grams = Number(existing?.in_transit_grams ?? 0)
        record.safety_grams = Math.max(0, Number(record.safety_grams ?? 0))
      }
      const index = rows.findIndex((item) => item.id === id)
      if (index >= 0) rows[index] = record
      else rows.unshift(record)
      browserDb[entity] = rows
      if (statusField(entity) && entity !== 'statements') validateBrowserStatusEntry(entity, id, String(record[statusField(entity)!] ?? ''))
      if (entity === 'follow_ups') {
        const previousCustomerId = Number(existing?.customer_id ?? 0)
        if (previousCustomerId && previousCustomerId !== Number(record.customer_id)) refreshBrowserCustomerFollowDate(previousCustomerId)
        refreshBrowserCustomerFollowDate(Number(record.customer_id))
      }
      if (entity === 'payments') refreshBrowserStatement(Number(record.statement_id))
      if (entity === 'statements') refreshBrowserStatement(id)
      if (entity === 'purchases' || entity === 'inventory' || entity === 'stock_reservations') refreshBrowserInventory()
      if (entity === 'payments') {
        const inputItems = Array.isArray(input.items) ? input.items as DocumentItemRecord[] : []
        saveBrowserPaymentAllocations(id, inputItems.length ? inputItems : [{ statement_id: Number(record.statement_id), allocated_amount_cents: Number(record.amount_cents) }])
      }
      if (entity === 'contracts' || entity === 'quotations' || entity === 'orders' || entity === 'deliveries' || entity === 'statements') {
        const inputItems = Array.isArray(input.items) ? input.items as DocumentItemRecord[] : []
        if (inputItems.length) saveBrowserDocumentItems(entity, id, inputItems)
        else if (entity === 'deliveries' && String(record.sent_date ?? '')) {
          // Match the native path: a dated delivery auto-fills remaining order lines
          // and creates the corresponding sale issue movements.
          ensureBrowserDeliveryItemsFromOrder(id)
          refreshBrowserDeliveryMovements(id)
        } else if (entity === 'statements') {
          refreshBrowserStatement(id)
        }
      }
      saveBrowserDb(browserDb)
      return enrichBrowserRow(entity, browserDb[entity].find((row) => row.id === id) ?? record) as T
    } catch (error) {
      restoreBrowserDb(before)
      throw error
    }
  }
  if (name === 'list_document_items') {
    return browserDocumentItems(String(args.document_type ?? ''), Number(args.document_id ?? 0)).map((row) => ({ ...row })) as T
  }
  if (name === 'save_document_items') {
    const documentType = String(args.document_type ?? '')
    const documentId = Number(args.document_id ?? 0)
    const items = Array.isArray(args.items) ? args.items as DocumentItemRecord[] : []
    const before = structuredClone(browserDb)
    try {
      const saved = saveBrowserDocumentItems(documentType, documentId, items)
      const total = saved.reduce((sum, item) => sum + Number(item.amount_cents ?? item.allocated_amount_cents ?? 0), 0)
      return { document_id: documentId, total_cents: total, items: saved } as T
    } catch (error) {
      restoreBrowserDb(before)
      throw error
    }
  }
  if (name === 'save_payment_allocations') {
    const paymentId = Number(args.payment_id ?? 0)
    const items = Array.isArray(args.allocations) ? args.allocations as DocumentItemRecord[] : []
    const before = structuredClone(browserDb)
    try {
      return { payment_id: paymentId, allocations: saveBrowserPaymentAllocations(paymentId, items) } as T
    } catch (error) {
      restoreBrowserDb(before)
      throw error
    }
  }
  if (name === 'download_template') {
    const entity = String(args.entity ?? '')
    const supported = ['customers', 'materials', 'quotations', 'contracts', 'orders', 'statements', 'purchases']
    if (!supported.includes(entity)) throw new Error('当前模块暂无导入模板')
    return { path: `./export/template-${entity}-demo.csv`, note_path: `./export/template-${entity}-demo-说明.txt` } as T
  }
  if (name === 'delete_entity') {
    const entity = args.entity as EntityName
    if (entity === 'follow_ups' || entity === 'sample_tests') throw new Error('历史记录不支持归档')
    if (['contract_items', 'quotation_items', 'order_items', 'delivery_items', 'statement_items', 'payment_allocations', 'inventory_movements', 'stock_reservations', 'attachments', 'cost_history', 'inventory'].includes(entity)) throw new Error('该记录由业务流程维护，不能直接归档')
    browserDb[entity] = (browserDb[entity] ?? []).filter((item) => item.id !== Number(args.id))
    saveBrowserDb(browserDb)
    return { ok: true } as T
  }
  if (name === 'transition_entity') {
    const before = structuredClone(browserDb)
    try {
      const entity = args.entity as EntityName
      const row = (browserDb[entity] ?? []).find((item) => item.id === Number(args.id))
      if (!row) throw new Error('记录不存在')
      const field = statusField(entity)
      if (!field) throw new Error('该对象没有可流转状态')
      const next = String(args.next_status ?? args.nextStatus ?? '')
      const current = String(row[field] ?? '')
      const reason = String(args.reason ?? '').trim()
      if (!canTransition(entity, current, next)) throw new Error(`不允许从“${current}”流转到“${next}”`)
      if (transitionNeedsReason(current, next) && !reason) throw new Error('该状态变更必须填写原因')
      if (entity === 'statements') throw new Error('对账状态由应收余额、承诺回款日和提醒参数自动计算，不能手工流转')
      if (entity === 'samples' && ['failed', 'retest'].includes(next)) row.fail_reason = reason
      validateBrowserStatusEntry(entity, Number(row.id), next)
      row[field] = next
      if (reason) row.transition_reason = reason
      if (entity === 'customers' && current !== next) {
        const rows = browserDb.follow_ups ?? []
        const id = Math.max(0, ...rows.map((item) => Number(item.id) || 0)) + 1
        const suffix = reason ? ` 原因：${reason}` : ''
        rows.unshift({
          id,
          customer_id: Number(row.id),
          content: `客户阶段由“${browserDictLabel('customer_stage', current)}”流转至“${browserDictLabel('customer_stage', next)}”。${suffix}`,
          follow_date: browserDate(),
          next_date: row.next_follow_date ?? null,
        })
        browserDb.follow_ups = rows
      }
      saveBrowserDb(browserDb)
      return row as T
    } catch (error) {
      restoreBrowserDb(before)
      throw error
    }
  }
  if (name === 'get_dashboard') return buildBrowserDashboard() as T
  if (name === 'backup_now') {
    const entry: BackupEntry = { path: './backup/demo-backup.db', checksum: `browser-demo-${Date.now()}`, size_bytes: 0, schema_version: 8, result: 'success', created_at: new Date().toISOString(), exists: true }
    browserBackups.unshift(entry)
    return entry as T
  }
  if (name === 'list_backups') return browserBackups as T
  if (name === 'set_protection') {
    if (String(args.password ?? '').length < 8) throw new Error('本地口令至少需要 8 个字符')
    browserProtectionEnabled = true
    return ({ enabled: true, recovery_key: 'DEMO-RECOVERY-KEY', warning: '浏览器演示模式不会修改本地文件。' } as T)
  }
  if (name === 'disable_protection') {
    browserProtectionEnabled = false
    return ({ enabled: false } as T)
  }
  if (name === 'restore_backup') throw new Error('浏览器演示模式不支持还原本地文件')
  if (name === 'integrity_check_command') return ({ ok: true, checked_at: new Date().toISOString() } as T)
  if (name === 'run_restore_drill') {
    const backupPath = String(args.backup_path ?? args.backupPath ?? '').trim()
    const backup = browserBackups.find((entry) => entry.path === backupPath) ?? browserBackups[0]
    if (!backup) throw new Error('暂无可用于恢复演练的备份')
    const started = new Date().toISOString()
    const record: RestoreDrillRecord = {
      id: Math.max(0, ...browserRestoreDrills.map((item) => Number(item.id) || 0)) + 1,
      backup_path: backup.path,
      backup_checksum: backup.checksum,
      backup_created_at: backup.created_at,
      started_at: started,
      finished_at: new Date(Date.now() + 42).toISOString(),
      duration_ms: 42,
      result: 'success',
      error_summary: '',
    }
    browserRestoreDrills.unshift(record)
    return ({ ...record, ok: true, demo_only: true } as T)
  }
  if (name === 'get_operational_metrics') return buildBrowserOperationalAssessment() as T
  if (name === 'export_entity') return ({ path: `./export/${String(args.entity)}.csv`, rows: (browserDb[args.entity as EntityName] ?? []).length, encrypted: false, demo_only: true } as T)
  if (name === 'import_rows' || name === 'import_file') {
    const strategy = String(args.strategy ?? 'rollback') === 'skip' ? 'skip' : 'rollback'
    return ({ batch_id: `DEMO-IMPORT-${Date.now()}`, inserted: 0, skipped: 0, failed: 1, errors: ['浏览器演示模式不写入本地文件'], failed_file: null, rolled_back: strategy === 'rollback', preflight_ok: false, strategy } as T)
  }
  throw new Error(`未知命令: ${name}`)
}

function buildBrowserDashboard(): DashboardData {
  const orders = browserDb.orders ?? []
  const statements = browserDb.statements ?? []
  const customers = browserDb.customers ?? []
  const followUps = latestBrowserFollowUps().map((row) => enrichBrowserRow('follow_ups', row))
  const month = today.slice(0, 7)
  const sales = orders.filter((order) => String(order.delivery_date ?? '').startsWith(month)).reduce((sum, order) => sum + Number(order.amount_cents ?? 0), 0)
  const received = (browserDb.payments ?? []).filter((payment) => String(payment.pay_date ?? '').startsWith(month)).reduce((sum, payment) => sum + Number(payment.amount_cents ?? 0), 0)
  const unpaid = statements.reduce((sum, statement) => sum + Number(statement.unpaid_cents ?? Number(statement.total_cents ?? 0) - Number(statement.received_cents ?? 0)), 0)
  const overdue = followUps.filter((followUp) => {
    const dueDate = String(followUp.next_date ?? '')
    return Boolean(dueDate) && dueDate < today
  }).length
  const stages = ['lead', 'contacted', 'sampling', 'quoting', 'won', 'paused_lost'].map((code) => ({ code, label: code, count: customers.filter((customer) => customer.stage === code).length, tone: (code === 'won' ? 'success' : code === 'quoting' ? 'warning' : code === 'sampling' ? 'teal' : code === 'paused_lost' ? 'danger' : code === 'contacted' ? 'info' : 'neutral') as DashboardData['stages'][number]['tone'], customers: customers.filter((customer) => customer.stage === code) }))
  const todos: DashboardData['todos'] = []
  followUps.filter((followUp) => {
    const dueDate = String(followUp.next_date ?? '')
    return Boolean(dueDate) && dueDate <= today
  }).forEach((followUp) => {
    const dueDate = String(followUp.next_date)
    todos.push({ id: `follow-${followUp.id}`, title: `跟进 ${followUp.customer_name}`, subtitle: dueDate === today ? '今日到期' : `已超期至 ${dueDate}`, due_date: dueDate, tone: dueDate === today ? 'warning' : 'danger', entity: 'follow_ups', entity_id: followUp.id })
  })
  statements.filter((statement) => statement.pay_status !== 'settled').forEach((statement) => todos.push({ id: `pay-${statement.id}`, title: `回款 ${statement.no}`, subtitle: `${statement.customer_name} · ${statement.pay_status === 'overdue' ? '已逾期' : '待跟进'}`, due_date: String(statement.promise_date ?? ''), tone: statement.pay_status === 'overdue' ? 'danger' : 'warning', entity: 'statements', entity_id: statement.id }))
  return { metrics: { monthly_sales_cents: sales, monthly_received_cents: received, unpaid_cents: unpaid, overdue_count: overdue }, todos: todos.slice(0, 8), stages }
}

function buildBrowserOperationalAssessment(): OperationalAssessment {
  const followUps = latestBrowserFollowUps()
  const due = followUps.filter((row) => String(row.next_date ?? '') && String(row.next_date) <= today)
  const overdue = due.filter((row) => String(row.next_date) < today)
  const overdueRate = due.length ? overdue.length * 100 / due.length : null
  const latestBackup = browserBackups[0]
  const latestDrill = browserRestoreDrills[0] ?? null
  const rpo = latestBackup ? Math.max(0, (Date.now() - new Date(latestBackup.created_at).getTime()) / 3_600_000) : null
  const rto = latestDrill?.duration_ms == null ? null : latestDrill.duration_ms / 60_000
  const allRecords = ['customers', 'materials', 'samples', 'quotations', 'contracts', 'orders', 'statements', 'suppliers', 'purchases', 'follow_ups']
    .reduce((sum, entity) => sum + (browserDb[entity as EntityName] ?? []).length, 0)
  const metric = (key: string, label: string, value: number | null, target: string, status: OperationalAssessment['metrics'][number]['status'], unit: string, formula: string, source: string, window: string, sampleSize: number) => ({ key, label, value, target, status, unit, formula, source, window, sample_size: sampleSize })
  const atMost = (value: number | null, target: number) => value == null ? 'insufficient_data' : value <= target ? 'pass' : 'fail'
  const atLeast = (value: number | null, target: number) => value == null ? 'insufficient_data' : value >= target ? 'pass' : 'fail'
  const metrics = [
    metric('follow_up_overdue_rate', '跟进超期率', overdueRate, '≤ 5%', atMost(overdueRate, 5), '%', '最新跟进记录中，已到期且未完成的任务数 / 已到期任务总数', '浏览器演示 follow_ups', '最近 30 天口径', due.length),
    metric('backup_success_rate', '备份成功率', latestBackup ? 100 : null, '≥ 99%', atLeast(latestBackup ? 100 : null, 99), '%', '通过校验的备份数 / 计划应执行备份数', '浏览器演示备份索引', '最近 7 天口径', browserBackups.length),
    metric('restore_success_rate', '还原成功率', browserRestoreDrills.length ? browserRestoreDrills.filter((row) => row.result === 'success').length * 100 / browserRestoreDrills.length : null, '100%', atLeast(browserRestoreDrills.length ? browserRestoreDrills.filter((row) => row.result === 'success').length * 100 / browserRestoreDrills.length : null, 100), '%', '恢复演练完整性检查通过次数 / 恢复演练次数', '浏览器演示恢复演练', '最近 30 天口径', browserRestoreDrills.length),
    metric('rpo_hours', '最近备份年龄（RPO）', rpo, '≤ 24 小时', atMost(rpo, 24), '小时', '当前时间 - 最近一次校验通过备份创建时间', '浏览器演示备份索引', '当前时点', latestBackup ? 1 : 0),
    metric('rto_minutes', '最近恢复耗时（RTO）', rto, '≤ 30 分钟', atMost(rto, 30), '分钟', '最近一次成功恢复演练的临时库校验耗时', '浏览器演示恢复演练', '当前时点', latestDrill ? 1 : 0),
    metric('data_completeness', '数据完整率', allRecords ? 100 : null, '≥ 98%', atLeast(allRecords ? 100 : null, 98), '%', '已填写必填字段的记录数 / 应填写记录数', '浏览器演示核心记录', '当前全量口径', allRecords),
  ]
  return {
    generated_at: new Date().toISOString(),
    window_start: new Date(Date.now() - 30 * 86_400_000).toISOString().slice(0, 10),
    window_end: today,
    metrics,
    latest_restore_drill: latestDrill,
    demo_only: true,
  }
}

function refreshBrowserStatement(statementId: number) {
  const statement = (browserDb.statements ?? []).find((row) => row.id === statementId)
  if (!statement) return
  const total = browserStatementTotal(statementId)
  const received = browserStatementReceived(statementId)
  statement.total_cents = total
  statement.received_cents = received
  statement.unpaid_cents = unpaidCents(statement)
  statement.pay_status = derivedPaymentStatus(String(statement.promise_date ?? '') || undefined, Number(statement.unpaid_cents), new Date(), browserReminderDays())
}
