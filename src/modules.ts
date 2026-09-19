import type { FieldDefinition, ModuleDefinition } from './types'

const stageOptions = [
  { label: '潜在客户', value: 'lead' },
  { label: '初步接触', value: 'contacted' },
  { label: '送样测试', value: 'sampling' },
  { label: '报价谈判', value: 'quoting' },
  { label: '定点量产', value: 'won' },
  { label: '暂停 / 流失', value: 'paused_lost' },
]
const sampleOptions = [
  { label: '待送样', value: 'pending_send' },
  { label: '已送样', value: 'sent' },
  { label: '测试中', value: 'testing' },
  { label: '通过', value: 'passed' },
  { label: '不通过', value: 'failed' },
  { label: '待复测', value: 'retest' },
]
const orderOptions = [
  { label: '待确认', value: 'pending_confirm' },
  { label: '备货中', value: 'preparing' },
  { label: '已送货', value: 'delivered' },
  { label: '已回签', value: 'signed' },
  { label: '已完成', value: 'completed' },
  { label: '已取消', value: 'cancelled' },
]
const payOptions = [
  { label: '未到期', value: 'not_due' },
  { label: '临近到期', value: 'near_due' },
  { label: '已逾期', value: 'overdue' },
  { label: '已结清', value: 'settled' },
]
const purchaseOptions = [
  { label: '待询价', value: 'pending_quote' },
  { label: '已下单', value: 'ordered' },
  { label: '在途', value: 'in_transit' },
  { label: '已入库', value: 'received' },
  { label: '关闭', value: 'closed' },
  { label: '已取消', value: 'cancelled' },
]

const select = (key: string, label: string, options: FieldDefinition['options'], required = false): FieldDefinition => ({ key, label, type: 'select', options, required })
const text = (key: string, label: string, required = false, type: FieldDefinition['type'] = 'text'): FieldDefinition => ({ key, label, type, required })
const date = (key: string, label: string, required = false): FieldDefinition => ({ key, label, type: 'date', required, placeholder: 'YYYY-MM-DD' })
const amount = (key: string, label: string, required = false): FieldDefinition => ({ key, label, type: 'number', required, placeholder: '元，保留两位小数' })
const quantity = (key: string, label: string, required = false): FieldDefinition => ({ key, label, type: 'number', required, placeholder: 'kg' })
const checkbox = (key: string, label: string, defaultValue = false): FieldDefinition => ({ key, label, type: 'checkbox', defaultValue })

const contractTerms = [
  '二、质量要求技术标准：质量要求按供方材料技术质量标准。',
  '三、交（提）货地点、方式：交货地点为供方仓库，供方代办运输。',
  '四、运输方式及目的地和费用负担：汽运至需方指定地点，供方负责运费。',
  '五、包装方式：25KG/袋。',
  '六、验收方法及提出异议期限：按条款（二）验收，提出异议期限为收货一周内。异议期内，需方不得批量投料，一经投料，供方将不承担任何经济损失。',
  '七、结算方式及期限：款到发货。',
  '八、违约责任：需方应按约付款，延迟一日承担货款总额万分之三的违约金。',
  '九、解决合同纠纷的方式：双方协商解决，协商不成，由供方所在地法院诉讼解决。',
  '十、本合同经双方签字或盖章后即生效，传真件具有同等法律效力。',
].join('\n')

export const modules: ModuleDefinition[] = [
  {
    key: 'customers', label: '客户跟进', group: '销售主线', icon: 'users', searchPlaceholder: '搜索客户、主机厂或材料体系', primaryField: 'name', statusField: 'stage',
    columns: [
      { key: 'name', label: '客户名称', width: 190 }, { key: 'main_host', label: '对接主机厂 / Tier1', width: 170 }, { key: 'direction', label: '产品方向', width: 140 }, { key: 'material_system', label: '材料体系', width: 120 }, { key: 'stage', label: '销售阶段', width: 120, kind: 'status' }, { key: 'next_follow_date', label: '下次跟进', width: 120, kind: 'date' },
    ],
    fields: [text('name', '客户名称', true), text('main_host', '对接主机厂 / Tier1'), text('direction', '产品方向'), text('material_system', '材料体系'), select('stage', '销售阶段', stageOptions, true), date('next_follow_date', '下次跟进日期'), { ...text('notes', '跟进备注'), type: 'textarea' }],
  },
  {
    key: 'follow_ups', label: '跟进记录', group: '销售主线', icon: 'contact', searchPlaceholder: '搜索客户、跟进内容或日期', primaryField: 'content', archiveable: false,
    columns: [{ key: 'customer_name', label: '客户', width: 180 }, { key: 'follow_date', label: '跟进日期', width: 120, kind: 'date' }, { key: 'content', label: '跟进内容', width: 280 }, { key: 'next_date', label: '下次跟进', width: 120, kind: 'date' }],
    fields: [text('customer_id', '客户', true, 'number'), date('follow_date', '跟进日期', true), { ...text('content', '跟进内容', true), type: 'textarea' }, date('next_date', '下次跟进日期')],
  },
  {
    key: 'contacts', label: '联系人', group: '销售主线', icon: 'contact', searchPlaceholder: '搜索联系人、客户或电话', primaryField: 'name',
    columns: [{ key: 'name', label: '联系人', width: 140 }, { key: 'customer_name', label: '客户', width: 190 }, { key: 'role', label: '职务', width: 120 }, { key: 'phone', label: '电话', width: 150 }, { key: 'preferred_channel', label: '首选渠道', width: 110 }],
    fields: [text('name', '姓名', true), text('customer_id', '客户 ID', true, 'number'), text('role', '职务'), text('phone', '电话'), text('email', '邮箱'), select('preferred_channel', '首选渠道', [{ label: '微信', value: '微信' }, { label: '电话', value: '电话' }, { label: '邮件', value: '邮件' }])],
  },
  {
    key: 'projects', label: '项目 / 商机', group: '销售主线', icon: 'briefcase', searchPlaceholder: '搜索项目、零件或主机厂', primaryField: 'name', statusField: 'stage',
    columns: [{ key: 'name', label: '项目名称', width: 190 }, { key: 'customer_name', label: '客户', width: 160 }, { key: 'main_host', label: '主机厂', width: 160 }, { key: 'part_name', label: '零件', width: 140 }, { key: 'expected_volume_grams', label: '预计量', width: 120, kind: 'number' }, { key: 'stage', label: '项目阶段', width: 120, kind: 'status' }],
    fields: [text('name', '项目名称', true), text('customer_id', '客户 ID', true, 'number'), text('main_host', '主机厂'), text('part_name', '零件名称'), quantity('expected_volume_grams', '预计量（kg）'), select('stage', '项目阶段', stageOptions, true), date('next_follow_date', '下次跟进')],
  },
  {
    key: 'materials', label: '材料牌号库', group: '产品与样品', icon: 'package', searchPlaceholder: '搜索牌号、基材或改性方式', primaryField: 'code',
    columns: [{ key: 'code', label: '牌号', width: 180 }, { key: 'base_resin', label: '基材', width: 110 }, { key: 'modification', label: '改性方式', width: 120 }, { key: 'mi', label: '熔指', width: 110 }, { key: 'impact', label: '冲击', width: 110 }, { key: 'hdt', label: '热变形', width: 110 }, { key: 'supplier_name', label: '供应来源', width: 150 }, { key: 'cost_cents', label: '参考成本', width: 120, kind: 'money' }],
    fields: [text('code', '牌号', true), text('base_resin', '基材', true), select('modification', '改性方式', [{ label: 'TD', value: 'TD' }, { label: 'GF', value: 'GF' }, { label: '阻燃', value: '阻燃' }, { label: '遮光', value: '遮光' }, { label: '光扩散', value: '光扩散' }]), text('mi', '熔指'), text('impact', '冲击'), text('hdt', '热变形'), text('density', '密度'), text('supplier_id', '供应商 ID', false, 'number'), amount('cost_cents', '参考成本'), date('cost_date', '成本更新日期')],
  },
  {
    key: 'samples', label: '样品管理', group: '产品与样品', icon: 'flask', searchPlaceholder: '搜索样品编号、客户或牌号', primaryField: 'code', statusField: 'status',
    columns: [{ key: 'code', label: '样品编号', width: 145 }, { key: 'customer_name', label: '客户', width: 170 }, { key: 'material_code', label: '牌号', width: 160 }, { key: 'batch', label: '批次', width: 110 }, { key: 'weight_grams', label: '克重', width: 100, kind: 'number' }, { key: 'sent_date', label: '送样日期', width: 120, kind: 'date' }, { key: 'status', label: '样品状态', width: 120, kind: 'status' }],
    fields: [text('code', '样品编号', true), text('customer_id', '客户 ID', true, 'number'), text('material_id', '牌号 ID', true, 'number'), text('batch', '批次'), quantity('weight_grams', '送样克重（kg）'), date('sent_date', '送样日期'), text('test_items', '测试项目'), select('status', '样品状态', sampleOptions, true), { ...text('fail_reason', '不通过 / 复测原因'), type: 'textarea' }],
  },
  {
    key: 'sample_tests', label: '测试记录', group: '产品与样品', icon: 'flask', searchPlaceholder: '搜索样品、测试项目、结论或日期', primaryField: 'test_item', archiveable: false,
    columns: [{ key: 'sample_code', label: '样品编号', width: 150 }, { key: 'customer_name', label: '客户', width: 170 }, { key: 'test_date', label: '测试日期', width: 120, kind: 'date' }, { key: 'test_item', label: '测试项目', width: 160 }, { key: 'conclusion', label: '结论', width: 220 }, { key: 'next_action', label: '下步动作', width: 220 }],
    fields: [text('sample_id', '样品', true, 'number'), date('test_date', '测试日期', true), text('test_item', '测试项目', true), { ...text('result', '测试结果'), type: 'textarea' }, { ...text('conclusion', '结论'), type: 'textarea' }, { ...text('next_action', '下步动作'), type: 'textarea' }],
  },
  {
    key: 'quotations', label: '报价管理', group: '交易履约', icon: 'file-text', searchPlaceholder: '搜索报价单号、客户、主题或牌号', primaryField: 'no', statusField: 'status', detailType: 'quotations',
    columns: [{ key: 'no', label: '报价单号', width: 150 }, { key: 'recipient_name', label: '收件单位', width: 190 }, { key: 'subject', label: '主题', width: 140 }, { key: 'material_code', label: '牌号', width: 160 }, { key: 'price_cents', label: '单价', width: 110, kind: 'money' }, { key: 'quote_date', label: '报价日期', width: 120, kind: 'date' }, { key: 'item_count', label: '明细行', width: 80, kind: 'number' }, { key: 'status', label: '状态', width: 110, kind: 'status' }],
    fields: [
      { ...text('no', '报价单号', true), placeholder: '内部管理编号，不在附件版式中显示' },
      text('customer_id', '客户', true, 'number'),
      { ...text('seller_name', '报价方'), defaultValue: '南京聚隆科技股份有限公司' },
      text('recipient_name', '收件单位（Company）'),
      text('sender_name', '发件人（From）'),
      text('recipient_contact', '收件人（To）'),
      text('recipient_fax', '传真号（Fax No）'),
      text('cc', '抄送（CC）'),
      { ...text('page_count', '页数', false, 'number'), defaultValue: 1 },
      checkbox('request_review', '请审阅', true),
      checkbox('request_comment', '请批注', true),
      date('quote_date', '报价日期'),
      { ...text('subject', '主题'), defaultValue: '材料报价' },
      text('material_id', '默认牌号', true, 'number'),
      amount('price_cents', '默认单价', true),
      quantity('moq_grams', '参考数量（kg）'),
      select('freight', '运费承担', [{ label: '买方', value: '买方' }, { label: '卖方', value: '卖方' }, { label: '含运费', value: '含运费' }]),
      date('valid_until', '有效期至'),
      { ...text('price_note', '价格说明'), type: 'textarea', defaultValue: '未税、含运费' },
      { ...text('adjustment_note', '调价说明'), type: 'textarea', defaultValue: '为保证贵我双方的利益，我公司将根据原材料价格的上下波动及时调整供货价格。' },
      { ...text('footer_address', '页脚地址'), defaultValue: '南京高新技术开发区聚龙路 8 号' },
      { ...text('footer_phone', '页脚电话'), defaultValue: '025-58840064' },
      { ...text('footer_fax', '页脚传真'), defaultValue: '025-58746904' },
      { ...text('footer_email', '页脚邮箱'), defaultValue: 'julong@publicl.ptt.js.cn' },
      { ...text('version', '版本号', false, 'number'), defaultValue: 1 },
      select('status', '报价状态', [{ label: '草稿', value: 'draft' }, { label: '已发送', value: 'sent' }, { label: '已接受', value: 'accepted' }, { label: '已过期', value: 'expired' }, { label: '已拒绝', value: 'rejected' }], true),
    ],
  },
  {
    key: 'contracts', label: '合同管理', group: '交易履约', icon: 'file-signature', searchPlaceholder: '搜索合同号、供方、需方或型号', primaryField: 'no', statusField: 'status', detailType: 'contracts',
    columns: [{ key: 'no', label: '合同号', width: 170 }, { key: 'seller_name', label: '供方', width: 210 }, { key: 'buyer_name', label: '需方', width: 210 }, { key: 'contract_date', label: '合同日期', width: 120, kind: 'date' }, { key: 'item_total_cents', label: '合同金额', width: 130, kind: 'money' }, { key: 'item_count', label: '明细行', width: 80, kind: 'number' }, { key: 'status', label: '状态', width: 110, kind: 'status' }],
    fields: [
      text('no', '合同号', true),
      { ...text('seller_name', '供方', true), defaultValue: '南京聚隆科技股份有限公司' },
      text('buyer_name', '需方', true),
      text('customer_id', '关联客户', false, 'number'),
      { ...text('execution_place', '合同履行地点'), defaultValue: '南京' },
      date('contract_date', '签订时间', true),
      { ...text('settlement_method', '结算方式'), defaultValue: '款到发货' },
      { ...text('packaging', '包装方式'), defaultValue: '25KG/袋' },
      { ...text('terms', '合同条款'), type: 'textarea', defaultValue: contractTerms },
      { ...text('seller_address', '供方单位地址'), defaultValue: '南京市高新技术开发区聚龙路8号' },
      { ...text('seller_legal_representative', '供方法定代表人'), defaultValue: '刘曙阳' },
      text('seller_agent', '供方委托代理人'),
      text('seller_phone', '供方电话'),
      text('seller_fax', '供方传真'),
      { ...text('seller_bank', '供方开户银行'), defaultValue: '中信银行建邺支行' },
      { ...text('seller_account', '供方账号'), defaultValue: '7329210182800049261' },
      text('buyer_address', '需方单位地址'),
      text('buyer_legal_representative', '需方法定代表人'),
      text('buyer_agent', '需方委托代理人'),
      text('buyer_phone', '需方电话'),
      text('buyer_fax', '需方传真'),
      text('buyer_bank', '需方开户银行'),
      text('buyer_account', '需方账号'),
      select('status', '合同状态', [{ label: '草稿', value: 'draft' }, { label: '已签署', value: 'signed' }, { label: '已履行', value: 'fulfilled' }, { label: '已取消', value: 'cancelled' }], true),
    ],
  },
  {
    key: 'orders', label: '订单送货', group: '交易履约', icon: 'truck', searchPlaceholder: '搜索订单号、客户或牌号', primaryField: 'no', statusField: 'status', detailType: 'orders',
    columns: [{ key: 'no', label: '订单号', width: 155 }, { key: 'customer_name', label: '客户', width: 180 }, { key: 'material_code', label: '牌号', width: 160 }, { key: 'qty_grams', label: '数量', width: 110, kind: 'number' }, { key: 'price_cents', label: '单价', width: 110, kind: 'money' }, { key: 'amount_cents', label: '订单金额', width: 130, kind: 'money' }, { key: 'delivery_date', label: '交期', width: 120, kind: 'date' }, { key: 'item_count', label: '明细行', width: 80, kind: 'number' }, { key: 'status', label: '订单状态', width: 120, kind: 'status' }],
    fields: [text('no', '订单号', true), text('customer_id', '客户 ID', true, 'number'), text('material_id', '牌号 ID', true, 'number'), quantity('qty_grams', '数量（kg）', true), amount('price_cents', '含税单价', true), date('delivery_date', '交期', true), select('status', '订单状态', orderOptions, true), select('sign_status', '回签状态', [{ label: '待回签', value: 'pending' }, { label: '已回签', value: 'signed' }]), text('quotation_id', '来源报价 ID', false, 'number')],
  },
  {
    key: 'deliveries', label: '送货单', group: '交易履约', icon: 'file-output', searchPlaceholder: '搜索送货单号或订单号', primaryField: 'no', detailType: 'deliveries',
    columns: [{ key: 'no', label: '送货单号', width: 160 }, { key: 'order_no', label: '订单号', width: 160 }, { key: 'customer_name', label: '客户', width: 180 }, { key: 'sent_date', label: '送货日期', width: 120, kind: 'date' }, { key: 'sign_date', label: '回签日期', width: 120, kind: 'date' }],
    fields: [text('no', '送货单号', true), text('order_id', '订单 ID', true, 'number'), date('sent_date', '送货日期', true), date('sign_date', '回签日期')],
  },
  {
    key: 'statements', label: '对账回款', group: '资金与采购', icon: 'wallet', searchPlaceholder: '搜索对账单号、客户或回款状态', primaryField: 'no', statusField: 'pay_status', detailType: 'statements',
    columns: [{ key: 'no', label: '对账单号', width: 155 }, { key: 'customer_name', label: '客户', width: 180 }, { key: 'period_start', label: '期间起', width: 110, kind: 'date' }, { key: 'period_end', label: '期间止', width: 110, kind: 'date' }, { key: 'total_cents', label: '对账金额', width: 130, kind: 'money' }, { key: 'received_cents', label: '已收', width: 120, kind: 'money' }, { key: 'unpaid_cents', label: '未收', width: 120, kind: 'money' }, { key: 'pay_status', label: '回款状态', width: 120, kind: 'status' }],
    fields: [text('no', '对账单号', true), text('customer_id', '客户 ID', true, 'number'), date('period_start', '期间开始', true), date('period_end', '期间结束', true), text('account_days', '账期（天）', false, 'number'), date('promise_date', '承诺回款日'), select('pay_status', '回款状态', payOptions, true)],
  },
  {
    key: 'payments', label: '回款登记', group: '资金与采购', icon: 'circle-dollar-sign', searchPlaceholder: '搜索对账单号或收款方式', primaryField: 'statement_no', detailType: 'payments',
    columns: [{ key: 'statement_no', label: '对账单号', width: 170 }, { key: 'amount_cents', label: '回款金额', width: 140, kind: 'money' }, { key: 'pay_date', label: '回款日期', width: 120, kind: 'date' }, { key: 'method', label: '收款方式', width: 110 }],
    fields: [text('statement_id', '对账单 ID', true, 'number'), amount('amount_cents', '回款金额', true), date('pay_date', '回款日期', true), select('method', '收款方式', [{ label: '转账', value: '转账' }, { label: '承兑', value: '承兑' }, { label: '现金', value: '现金' }], true)],
  },
  {
    key: 'suppliers', label: '供应商', group: '资金与采购', icon: 'factory', searchPlaceholder: '搜索供应商或联系人', primaryField: 'name',
    columns: [{ key: 'name', label: '供应商名称', width: 190 }, { key: 'contact', label: '联系方式', width: 180 }, { key: 'price_ref_cents', label: '最新询价', width: 120, kind: 'money' }],
    fields: [text('name', '供应商名称', true), text('contact', '联系方式'), amount('price_ref_cents', '最新询价')],
  },
  {
    key: 'purchases', label: '采购单', group: '资金与采购', icon: 'shopping-cart', searchPlaceholder: '搜索采购单号、供应商或牌号', primaryField: 'no', statusField: 'status',
    columns: [{ key: 'no', label: '采购单号', width: 155 }, { key: 'supplier_name', label: '供应商', width: 170 }, { key: 'material_code', label: '牌号', width: 170 }, { key: 'qty_grams', label: '采购量', width: 110, kind: 'number' }, { key: 'eta', label: '预计到货', width: 120, kind: 'date' }, { key: 'status', label: '采购状态', width: 120, kind: 'status' }],
    fields: [text('no', '采购单号', true), text('supplier_id', '供应商 ID', true, 'number'), text('material_id', '牌号 ID', true, 'number'), quantity('qty_grams', '采购量（kg）', true), select('status', '采购状态', purchaseOptions, true), date('eta', '预计到货')],
  },
  {
    key: 'inventory', label: '库存', group: '资金与采购', icon: 'boxes', searchPlaceholder: '搜索牌号', primaryField: 'material_code',
    columns: [{ key: 'material_code', label: '牌号', width: 200 }, { key: 'on_hand_grams', label: '现有库存', width: 130, kind: 'number' }, { key: 'in_transit_grams', label: '在途量', width: 120, kind: 'number' }, { key: 'safety_grams', label: '安全库存', width: 120, kind: 'number' }, { key: 'available_grams', label: '可用量', width: 130, kind: 'number' }],
    fields: [text('material_id', '牌号 ID', true, 'number'), quantity('on_hand_grams', '现有库存（kg）'), quantity('in_transit_grams', '在途量（kg）'), quantity('safety_grams', '安全库存（kg）')],
  },
  {
    key: 'inventory_movements', label: '库存流水', group: '库存与追溯', icon: 'warehouse', searchPlaceholder: '搜索牌号、批次或流水类型', primaryField: 'material_code', archiveable: false, readOnly: true,
    columns: [{ key: 'material_code', label: '牌号', width: 180 }, { key: 'movement_type', label: '流水类型', width: 130 }, { key: 'qty_grams', label: '数量', width: 120, kind: 'number' }, { key: 'batch', label: '批次', width: 130 }, { key: 'warehouse', label: '仓库', width: 120 }, { key: 'unit_cost_cents', label: '单位成本', width: 120, kind: 'money' }, { key: 'source_id', label: '来源 ID', width: 90, kind: 'number' }, { key: 'created_at', label: '发生时间', width: 180 }],
    fields: [],
  },
  {
    key: 'stock_reservations', label: '库存预占', group: '库存与追溯', icon: 'layers', searchPlaceholder: '搜索牌号、订单或批次', primaryField: 'material_code', archiveable: false, readOnly: true,
    columns: [{ key: 'material_code', label: '牌号', width: 180 }, { key: 'order_no', label: '订单号', width: 160 }, { key: 'reserved_grams', label: '预占数量', width: 120, kind: 'number' }, { key: 'batch', label: '批次', width: 130 }, { key: 'warehouse', label: '仓库', width: 120 }, { key: 'status', label: '状态', width: 110, kind: 'status' }, { key: 'created_at', label: '创建时间', width: 180 }],
    fields: [],
  },
  {
    key: 'attachments', label: '附件索引', group: '库存与追溯', icon: 'paperclip', searchPlaceholder: '搜索对象、路径或哈希', primaryField: 'relative_path', archiveable: false, readOnly: true,
    columns: [{ key: 'object_type', label: '对象类型', width: 130 }, { key: 'object_id', label: '对象 ID', width: 90, kind: 'number' }, { key: 'relative_path', label: '相对路径', width: 300 }, { key: 'hash', label: 'SHA-256', width: 300 }, { key: 'size_bytes', label: '文件大小', width: 110, kind: 'number' }, { key: 'integrity', label: '校验状态', width: 120 }, { key: 'created_at', label: '登记时间', width: 180 }],
    fields: [],
  },
  {
    key: 'cost_history', label: '成本历史', group: '库存与追溯', icon: 'chart-no-axes-combined', searchPlaceholder: '搜索牌号、来源或备注', primaryField: 'material_code', archiveable: false, readOnly: true,
    columns: [{ key: 'material_code', label: '牌号', width: 180 }, { key: 'cost_cents', label: '成本', width: 120, kind: 'money' }, { key: 'cost_date', label: '成本日期', width: 120, kind: 'date' }, { key: 'source', label: '来源', width: 120 }, { key: 'note', label: '备注', width: 260 }, { key: 'created_at', label: '记录时间', width: 180 }],
    fields: [],
  },
]

export const moduleMap = Object.fromEntries(modules.map((item) => [item.key, item])) as Record<string, ModuleDefinition>

export const navigationGroups = [
  { label: '工作区', keys: ['dashboard'] },
  { label: '销售主线', keys: ['customers', 'follow_ups', 'contacts', 'projects'] },
  { label: '产品与样品', keys: ['materials', 'samples', 'sample_tests'] },
  { label: '交易履约', keys: ['quotations', 'contracts', 'orders', 'deliveries'] },
  { label: '资金与采购', keys: ['statements', 'payments', 'suppliers', 'purchases', 'inventory'] },
  { label: '库存与追溯', keys: ['inventory_movements', 'stock_reservations', 'attachments', 'cost_history'] },
]
