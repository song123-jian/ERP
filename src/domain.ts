import type { EntityName, EntityRecord } from './types'

type BackupScheduleEntry = {
  created_at?: string
  checksum?: string
  result?: string
  exists?: boolean
}

const transitions: Record<string, Record<string, string[]>> = {
  customers: { lead: ['contacted'], contacted: ['sampling', 'paused_lost'], sampling: ['quoting', 'paused_lost'], quoting: ['won', 'paused_lost'], won: ['paused_lost'], paused_lost: ['contacted', 'sampling', 'quoting', 'won'] },
  projects: { lead: ['contacted'], contacted: ['sampling', 'paused_lost'], sampling: ['quoting', 'paused_lost'], quoting: ['won', 'paused_lost'], won: ['paused_lost'], paused_lost: ['contacted', 'sampling', 'quoting', 'won'] },
  quotations: { draft: ['sent', 'rejected'], sent: ['accepted', 'expired', 'rejected'], accepted: [], expired: [], rejected: [] },
  contracts: { draft: ['signed', 'cancelled'], signed: ['fulfilled', 'cancelled'], fulfilled: [], cancelled: [] },
  samples: { pending_send: ['sent'], sent: ['testing'], testing: ['passed', 'failed', 'retest'], failed: ['retest'], retest: ['sent', 'testing'], passed: [] },
  orders: { pending_confirm: ['preparing', 'cancelled'], preparing: ['delivered', 'cancelled'], delivered: ['signed'], signed: ['completed'], completed: [], cancelled: [] },
  purchases: { pending_quote: ['ordered', 'cancelled'], ordered: ['in_transit', 'cancelled'], in_transit: ['received'], received: ['closed'], closed: [], cancelled: [] },
  statements: { not_due: ['near_due', 'overdue', 'settled'], near_due: ['overdue', 'settled'], overdue: ['settled'], settled: [] },
}

export function statusField(entity: EntityName): string | undefined {
  return ['customers', 'projects'].includes(entity) ? 'stage' : entity === 'statements' ? 'pay_status' : ['contracts', 'quotations', 'samples', 'orders', 'purchases'].includes(entity) ? 'status' : undefined
}

export function canTransition(entity: EntityName, current: string, next: string): boolean {
  return current === next || Boolean(transitions[entity]?.[current]?.includes(next))
}

export function transitionNeedsReason(current: string, next: string): boolean {
  return ['paused_lost', 'failed', 'retest', 'cancelled', 'rejected'].includes(next) || (current === 'won' && next === 'paused_lost')
}

export function orderAmountCents(row: EntityRecord): number {
  return Math.round(Number(row.qty_grams ?? 0) * Number(row.price_cents ?? 0) / 1000)
}

export function unpaidCents(row: EntityRecord): number {
  return Math.max(0, Number(row.total_cents ?? 0) - Number(row.received_cents ?? 0))
}

export function derivedPaymentStatus(promiseDate: string | undefined, unpaid: number, now = new Date(), reminderDays = 3): string {
  if (unpaid <= 0) return 'settled'
  if (!promiseDate) return 'not_due'
  const due = new Date(`${promiseDate}T00:00:00`)
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate())
  const days = Math.round((due.getTime() - today.getTime()) / 86400000)
  if (days < 0) return 'overdue'
  if (days <= Math.max(0, Math.min(365, Math.round(reminderDays)))) return 'near_due'
  return 'not_due'
}

/**
 * Decide whether the background backup should run without coupling the rule to
 * the Vue lifecycle. Only an existing, checksum-verified successful backup is
 * considered a valid schedule checkpoint.
 */
export function shouldRunScheduledBackup(
  backups: BackupScheduleEntry[],
  enabled: boolean,
  intervalDays: number,
  now = new Date(),
): boolean {
  if (!enabled) return false
  const days = Math.max(1, Math.min(30, Math.round(intervalDays)))
  const latest = backups
    .filter((entry) => entry.result === 'success' && entry.exists !== false && Boolean(entry.checksum))
    .map((entry) => new Date(String(entry.created_at ?? '')).getTime())
    .filter((time) => Number.isFinite(time))
    .sort((a, b) => b - a)[0]
  if (latest == null) return true
  return now.getTime() - latest >= days * 86_400_000
}
