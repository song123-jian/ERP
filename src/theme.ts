import type { GlobalThemeOverrides } from 'naive-ui'

export const tokens = {
  primary: '#2B6CB0',
  primaryHover: '#245B96',
  primarySoft: '#E8F1F9',
  page: '#F5F7FA',
  surface: '#FFFFFF',
  border: '#DCE3EA',
  text: '#202B37',
  textMuted: '#667482',
  neutral: '#8A9099',
  neutralSoft: '#F0F1F3',
  info: '#2B6CB0',
  infoSoft: '#E8F1F9',
  teal: '#0E8A8A',
  tealSoft: '#E4F4F4',
  warning: '#D98A09',
  warningSoft: '#FDF3E2',
  success: '#2E9E5B',
  successSoft: '#E8F6EE',
  danger: '#D54941',
  dangerSoft: '#FCEBEA',
}

export const naiveTheme: GlobalThemeOverrides = {
  common: {
    primaryColor: tokens.primary,
    primaryColorHover: tokens.primaryHover,
    primaryColorPressed: '#1D4E80',
    infoColor: tokens.info,
    successColor: tokens.success,
    warningColor: tokens.warning,
    errorColor: tokens.danger,
    borderRadius: '4px',
    borderRadiusSmall: '4px',
    fontFamily: 'Microsoft YaHei, Segoe UI, sans-serif',
    fontFamilyMono: 'Cascadia Mono, Consolas, monospace',
    textColorBase: tokens.text,
    textColor1: tokens.text,
    textColor2: '#3A4754',
    textColor3: tokens.textMuted,
    dividerColor: tokens.border,
  },
  Layout: {
    color: tokens.page,
    siderColor: tokens.surface,
    headerColor: tokens.surface,
    siderBorderColor: tokens.border,
    headerBorderColor: tokens.border,
  },
  Card: {
    borderRadius: '6px',
    borderColor: tokens.border,
  },
  DataTable: {
    borderColor: tokens.border,
    thColor: '#F8FAFC',
    thTextColor: '#526170',
    tdColor: tokens.surface,
    tdColorHover: '#F7FAFD',
  },
}

export const statusTone: Record<string, { tone: string; label: string }> = {
  lead: { tone: 'neutral', label: '潜在客户' },
  contacted: { tone: 'info', label: '初步接触' },
  sampling: { tone: 'teal', label: '送样测试' },
  quoting: { tone: 'warning', label: '报价谈判' },
  won: { tone: 'success', label: '定点量产' },
  paused_lost: { tone: 'danger', label: '暂停 / 流失' },
  pending_send: { tone: 'neutral', label: '待送样' },
  sent: { tone: 'info', label: '已送样' },
  testing: { tone: 'teal', label: '测试中' },
  passed: { tone: 'success', label: '测试通过' },
  failed: { tone: 'danger', label: '不通过' },
  retest: { tone: 'warning', label: '待复测' },
  pending_confirm: { tone: 'neutral', label: '待确认' },
  preparing: { tone: 'info', label: '备货中' },
  delivered: { tone: 'teal', label: '已送货' },
  signed: { tone: 'success', label: '已回签' },
  completed: { tone: 'success', label: '已完成' },
  cancelled: { tone: 'danger', label: '已取消' },
  not_due: { tone: 'neutral', label: '未到期' },
  near_due: { tone: 'warning', label: '临近到期' },
  overdue: { tone: 'danger', label: '已逾期' },
  settled: { tone: 'success', label: '已结清' },
  draft: { tone: 'neutral', label: '草稿' },
  accepted: { tone: 'success', label: '已接受' },
  expired: { tone: 'danger', label: '已过期' },
  rejected: { tone: 'danger', label: '已拒绝' },
  fulfilled: { tone: 'success', label: '已履行' },
  pending: { tone: 'neutral', label: '待回签' },
  pending_quote: { tone: 'neutral', label: '待询价' },
  ordered: { tone: 'info', label: '已下单' },
  in_transit: { tone: 'info', label: '在途' },
  received: { tone: 'success', label: '已入库' },
  closed: { tone: 'neutral', label: '关闭' },
}

export function toneForStatus(value: unknown): string {
  return statusTone[String(value ?? '')]?.tone ?? 'neutral'
}

export function labelForStatus(value: unknown): string {
  return statusTone[String(value ?? '')]?.label ?? String(value ?? '未设置')
}

export function formatMoney(cents: unknown): string {
  const value = Number(cents ?? 0) / 100
  return `¥${value.toLocaleString('zh-CN', { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`
}

export function formatQuantity(grams: unknown): string {
  const value = Number(grams ?? 0) / 1000
  return `${value.toLocaleString('zh-CN', { minimumFractionDigits: 3, maximumFractionDigits: 3 })} kg`
}
