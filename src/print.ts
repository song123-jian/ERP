import type { DocumentItemRecord, EntityRecord } from './types'

const chineseDigits = ['零', '壹', '贰', '叁', '肆', '伍', '陆', '柒', '捌', '玖']
const chineseUnits = ['', '拾', '佰', '仟']
const chineseBigUnits = ['', '万', '亿', '兆']

function integerToChinese(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return '零'
  const groups: string[] = []
  let rest = Math.floor(value)
  while (rest > 0) {
    groups.push(String(rest % 10000))
    rest = Math.floor(rest / 10000)
  }
  let output = ''
  let pendingZero = false
  for (let index = groups.length - 1; index >= 0; index -= 1) {
    const group = Number(groups[index])
    if (group === 0) {
      if (output) pendingZero = true
      continue
    }
    if (output && (pendingZero || group < 1000)) output += '零'
    pendingZero = false
    const digits = String(group).padStart(4, '0')
    let groupText = ''
    for (let position = 0; position < 4; position += 1) {
      const digit = Number(digits[position])
      const unitIndex = 3 - position
      if (digit === 0) {
        if (groupText) pendingZero = true
        continue
      }
      if (pendingZero && groupText) groupText += '零'
      pendingZero = false
      groupText += chineseDigits[digit] + chineseUnits[unitIndex]
    }
    output += groupText + chineseBigUnits[index]
  }
  return output || '零'
}

export function amountToChineseUpper(cents: unknown): string {
  const numeric = Number(cents ?? 0)
  const value = Number.isFinite(numeric) ? Math.max(0, Math.round(numeric)) : 0
  const yuan = Math.floor(value / 100)
  const remainder = value % 100
  const jiao = Math.floor(remainder / 10)
  const fen = remainder % 10
  let output = `${integerToChinese(yuan)}元`
  if (jiao === 0 && fen === 0) return `${output}整`
  if (jiao > 0) output += `${chineseDigits[jiao]}角`
  else if (fen > 0) output += '零'
  if (fen > 0) output += `${chineseDigits[fen]}分`
  return output
}

/**
 * Contract totals are derived from the saved line-item amounts so every view
 * (editor, detail dialog, and print preview) uses the same cent-level value.
 */
export function contractTotalCents(items: Array<Pick<DocumentItemRecord, 'amount_cents'>>): number {
  const total = items.reduce((sum, item) => {
    const amount = Number(item.amount_cents ?? 0)
    return Number.isFinite(amount) ? sum + Math.round(amount) : sum
  }, 0)
  return Math.max(0, total)
}

export function printMoney(cents: unknown): string {
  return (Number(cents ?? 0) / 100).toLocaleString('zh-CN', { minimumFractionDigits: 2, maximumFractionDigits: 2 })
}

export function printQuantity(grams: unknown): string {
  return (Number(grams ?? 0) / 1000).toLocaleString('zh-CN', { minimumFractionDigits: 3, maximumFractionDigits: 3 })
}

function compactNumber(value: number, maximumFractionDigits = 2): string {
  return Number(value || 0).toLocaleString('zh-CN', { minimumFractionDigits: 0, maximumFractionDigits, useGrouping: false })
}

function compactMoney(cents: unknown): string {
  return compactNumber(Number(cents ?? 0) / 100, 2)
}

function compactQuantity(grams: unknown): string {
  return compactNumber(Number(grams ?? 0) / 1000, 3)
}

function escapeHtml(value: unknown): string {
  return String(value ?? '')
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;')
}

const fallbackContractTerms = [
  '二、质量要求技术标准：质量要求按供方材料技术质量标准。',
  '三、交（提）货地点、方式：交货地点为供方仓库，供方代办运输。',
  '四、运输方式及目的地和费用负担：汽运至需方指定地点，供方负责运费。',
  '五、包装方式：25KG/袋。',
  '六、验收方法及提出异议期限：按条款（二）验收，提出异议期限为收货一周内。异议期内，需方不得批量投料，一经投料，供方将不承担任何经济损失。',
  '七、结算方式及期限：款到发货。',
  '八、违约责任：需方应按约付款，延迟一日承担货款总额万分之三的违约金。',
  '九、解决合同纠纷的方式：双方协商解决，协商不成，由供方所在地法院诉讼解决。',
  '十、本合同经双方签字或盖章后即生效，传真件具有同等法律效力。',
]

function contractTerms(document: EntityRecord): string[] {
  const raw = String(document.terms ?? '').trim()
  const lines = (raw || fallbackContractTerms.join('\n')).split(/\r?\n/).map((line) => line.trim()).filter(Boolean)
  const packaging = String(document.packaging ?? '').trim() || '25KG/袋'
  const settlement = String(document.settlement_method ?? '').trim() || '款到发货'
  const settlementChoices = ['款到发货', '月结30天', '月结60天']
  const settlementLine = settlementChoices.map((choice) => `${settlement === choice ? '■' : '□'}${choice}`).join('　')
  return lines.map((line) => {
    if (/^五、/.test(line)) return `五、包装方式：${packaging}。`
    if (/^七、/.test(line)) return `七、结算方式及期限：${settlementChoices.includes(settlement) ? settlementLine : `■${settlement}`}`
    return line
  })
}

function contractParty(title: string, prefix: 'seller' | 'buyer', document: EntityRecord): string {
  const fields = [
    ['单位名称', document[`${prefix}_name`]],
    ['单位地址', document[`${prefix}_address`]],
    ['法定代表人', document[`${prefix}_legal_representative`]],
    ['委托代理人', document[`${prefix}_agent`]],
    ['电话', document[`${prefix}_phone`]],
    ['传真', document[`${prefix}_fax`]],
    ['开户银行', document[`${prefix}_bank`]],
    ['账号', document[`${prefix}_account`]],
  ]
  return `<section class="contract-party"><h3>${title}</h3><dl>${fields.map(([label, value]) => `<div><dt>${label}：</dt><dd>${escapeHtml(value)}</dd></div>`).join('')}</dl></section>`
}

function quotationDate(value: unknown): string {
  const match = String(value ?? '').match(/^(\d{4})-(\d{2})-(\d{2})$/)
  if (!match) return escapeHtml(value)
  return `${Number(match[2])}/${Number(match[3])}/${match[1].slice(2)}`
}

function boolValue(value: unknown, fallback: boolean): boolean {
  if (value == null || value === '') return fallback
  if (typeof value === 'boolean') return value
  if (typeof value === 'number') return value !== 0
  return ['1', 'true', 'yes', 'on'].includes(String(value).trim().toLowerCase())
}

function checkMark(value: boolean): string {
  return value ? '☑' : '□'
}

export function buildContractPrintHtml(document: EntityRecord, items: DocumentItemRecord[]): string {
  const total = contractTotalCents(items)
  const rows = items.length
    ? items.map((item) => `<tr><td>${escapeHtml(item.material_name)}</td><td>${escapeHtml(item.model)}</td><td>${escapeHtml(item.manufacturer)}</td><td>${compactQuantity(item.qty_grams)}</td><td>${compactMoney(item.unit_price_cents)}</td><td>${compactMoney(item.amount_cents)}</td><td>${escapeHtml(item.note || '具体交期双方协商')}</td></tr>`).join('')
    : '<tr><td colspan="7">暂无产品明细</td></tr>'
  return `<article class="print-sheet contract-sheet"><header class="contract-header"><div class="contract-brand"><span class="contract-brand-mark">JL</span><strong>Contract</strong></div><div class="contract-title"><h1>工程塑料购销合同</h1><small>JL-AA-X11</small></div></header><section class="contract-summary"><div><p><b>供方：</b>${escapeHtml(document.seller_name)}</p><p><b>需方：</b>${escapeHtml(document.buyer_name)}</p></div><div><p><b>合同编号：</b>${escapeHtml(document.no)}</p><p><b>合同履行地点：</b>${escapeHtml(document.execution_place)}</p><p><b>签订时间：</b>${escapeHtml(document.contract_date)}</p></div></section><h2 class="contract-section-title">一、产品名称、商标、厂家、金额、供货时间及数量</h2><table class="contract-items"><thead><tr><th>产品名称</th><th>规格型号</th><th>生产厂家</th><th>数量<br><small>KG</small></th><th>含税单价<br><small>元/KG</small></th><th>金额<br><small>元</small></th><th>交（提）货<br>时间及数量</th></tr></thead><tbody>${rows}</tbody><tfoot><tr><td colspan="3" class="contract-total-label">合计人民币金额（小写）：</td><td colspan="4" class="contract-total-value">${compactMoney(total)}</td></tr><tr><td colspan="3" class="contract-total-label">合计人民币金额（大写）：</td><td colspan="4" class="contract-total-value">${amountToChineseUpper(total)}</td></tr></tfoot></table><section class="contract-terms">${contractTerms(document).map((line) => `<p>${escapeHtml(line)}</p>`).join('')}</section><section class="contract-parties">${contractParty('供方', 'seller', document)}${contractParty('需方', 'buyer', document)}</section><p class="contract-return-note">请确认后回传，以便我司及时安排生产交货。</p></article>`
}

export function buildQuotationPrintHtml(document: EntityRecord, items: DocumentItemRecord[]): string {
  const rows = items.length
    ? items.map((item) => `<tr><td>${escapeHtml(item.material_category)}</td><td>${escapeHtml(item.material_grade || item.material_code)}</td><td>${compactMoney(item.unit_price_cents)}</td><td>${escapeHtml(item.note || document.price_note || document.freight)}</td></tr>`).join('')
    : '<tr><td colspan="4">暂无报价明细</td></tr>'
  const review = boolValue(document.request_review, true)
  const comment = boolValue(document.request_comment, true)
  return `<article class="print-sheet quotation-sheet"><header class="quotation-header"><h1>${escapeHtml(document.seller_name || '南京聚隆科技股份有限公司')}</h1></header><table class="quotation-meta"><tbody><tr><td><b>收件单位（Company）：</b>${escapeHtml(document.recipient_name || document.customer_name)}</td><td><b>发件人（From）：</b>${escapeHtml(document.sender_name)}</td></tr><tr><td><b>收件人（To）：</b>${escapeHtml(document.recipient_contact)}</td><td><b>日期（Date）：</b>${quotationDate(document.quote_date)}</td></tr><tr><td><b>传真号（Fax No）：</b>${escapeHtml(document.recipient_fax)}</td><td><b>页数（Page-Inc this one）：</b>${escapeHtml(document.page_count || 1)}</td></tr><tr><td><b>主题（Subject）：</b>${escapeHtml(document.subject || '材料报价')}</td><td><b>抄送（CC）：</b>${escapeHtml(document.cc)}</td></tr></tbody></table><section class="quotation-actions"><span>□ <b>紧急</b></span><span>${checkMark(review)} <b>请审阅</b></span><span>${checkMark(comment)} <b>请批注</b></span><span>□ <b>请答复</b></span><span>□ <b>请传阅</b></span><small>★ 如不清晰烦请电话</small></section><section class="quotation-body"><p>您好！</p><p>目前贵司所需材料现报价如下：</p><table class="quotation-items"><thead><tr><th>材料类别</th><th>牌号</th><th>价格（元/KG）</th><th>备注</th></tr></thead><tbody>${rows}</tbody></table><p class="quotation-adjustment">${escapeHtml(document.adjustment_note || '为保证贵我双方的利益，我公司将根据原材料价格的上下波动及时调整供货价格。')}</p><div class="quotation-closing"><p>顺祝</p><p>商祺！</p></div></section><footer class="quotation-footer"><span><b>ADD：</b>${escapeHtml(document.footer_address)}</span><span><b>TEL：</b>${escapeHtml(document.footer_phone)}</span><span><b>FAX：</b>${escapeHtml(document.footer_fax)}</span><span><b>E-mail：</b>${escapeHtml(document.footer_email)}</span></footer></article>`
}
