import { describe, expect, it } from 'vitest'
import { canTransition, derivedPaymentStatus, orderAmountCents, shouldRunScheduledBackup, transitionNeedsReason } from '../src/domain'
import { amountToChineseUpper, buildContractPrintHtml, buildQuotationPrintHtml, contractTotalCents } from '../src/print'
import { dirtyDrafts, discardDirtyDrafts, registerDraft, saveDirtyDrafts, unregisterDraft } from '../src/drafts'
import { activeTasks, runTrackedTask, startTask, waitForTasks } from '../src/tasks'

describe('ERP domain rules', () => {
  it('keeps money and quantity calculations deterministic', () => {
    expect(orderAmountCents({ id: 1, qty_grams: 12500, price_cents: 2180 })).toBe(27_250)
    expect(orderAmountCents({ id: 1, qty_grams: 0, price_cents: 2180 })).toBe(0)
  })

  it('accepts only the documented state transitions', () => {
    expect(canTransition('customers', 'lead', 'contacted')).toBe(true)
    expect(canTransition('customers', 'lead', 'won')).toBe(false)
    expect(canTransition('projects', 'lead', 'contacted')).toBe(true)
    expect(canTransition('projects', 'lead', 'won')).toBe(false)
    expect(canTransition('quotations', 'draft', 'sent')).toBe(true)
    expect(canTransition('quotations', 'draft', 'accepted')).toBe(false)
    expect(canTransition('contracts', 'draft', 'signed')).toBe(true)
    expect(canTransition('contracts', 'draft', 'fulfilled')).toBe(false)
    expect(canTransition('orders', 'preparing', 'delivered')).toBe(true)
    expect(canTransition('orders', 'pending_confirm', 'completed')).toBe(false)
    expect(transitionNeedsReason('quoting', 'paused_lost')).toBe(true)
    expect(transitionNeedsReason('sent', 'rejected')).toBe(true)
    expect(transitionNeedsReason('lead', 'contacted')).toBe(false)
  })

  it('formats Chinese uppercase money and builds escaped printable snapshots', () => {
    expect(amountToChineseUpper(0)).toBe('零元整')
    expect(amountToChineseUpper(Number.NaN)).toBe('零元整')
    expect(amountToChineseUpper(236000)).toBe('贰仟叁佰陆拾元整')
    expect(amountToChineseUpper(100101)).toBe('壹仟零壹元零壹分')
    expect(contractTotalCents([{ amount_cents: 236000 }, { amount_cents: 1001 }, { amount_cents: Number.NaN }])).toBe(237001)
    expect(contractTotalCents([{ amount_cents: -1 }])).toBe(0)
    const contract = buildContractPrintHtml({ id: 1, no: 'TZJP202609-18', seller_name: '南京聚隆', buyer_name: '台州骏普', contract_date: '2026-09-18', seller_account: '7329210182800049261', buyer_account: '530166084800015', terms: '<不得解释为标签>' }, [{ material_name: 'PP', model: 'PI0-S27A[BK16452]', qty_grams: 200000, unit_price_cents: 1180, amount_cents: 236000 }])
    expect(contract).toContain('工程塑料购销合同')
    expect(contract).toContain('贰仟叁佰陆拾元整')
    expect(contract).toContain('&lt;不得解释为标签&gt;')
    expect(contract).toContain('7329210182800049261')
    expect(contract).toContain('530166084800015')
    const quote = buildQuotationPrintHtml({ id: 2, no: 'Q-0615', seller_name: '南京聚隆', recipient_name: '浙江零跑', sender_name: '销售部', recipient_contact: '采购部', recipient_fax: '0571-00000000', page_count: 1, request_review: true, request_comment: false, quote_date: '2026-06-15', subject: '材料报价', price_note: '未税、含运费' }, [{ material_category: 'PP-GF20', material_grade: 'PG4-S01A', unit_price_cents: 1060, qty_grams: 1000, amount_cents: 1060 }])
    expect(quote).toContain('收件单位（Company）')
    expect(quote).toContain('6/15/26')
    expect(quote).toContain('☑ <b>请审阅</b>')
    expect(quote).toContain('□ <b>请批注</b>')
    expect(quote).toContain('PG4-S01A')
    expect(quote).toContain('未税、含运费')
  })

  it('derives payment status from the due date and unpaid balance', () => {
    const now = new Date('2026-09-12T12:00:00')
    expect(derivedPaymentStatus('2026-09-20', 100, now)).toBe('not_due')
    expect(derivedPaymentStatus('2026-09-14', 100, now)).toBe('near_due')
    expect(derivedPaymentStatus('2026-09-10', 100, now)).toBe('overdue')
    expect(derivedPaymentStatus('2026-09-10', 0, now)).toBe('settled')
  })

  it('runs scheduled backup only when no recent verified backup exists', () => {
    const now = new Date('2026-09-15T12:00:00Z')
    expect(shouldRunScheduledBackup([], true, 1, now)).toBe(true)
    expect(shouldRunScheduledBackup([], false, 1, now)).toBe(false)
    expect(shouldRunScheduledBackup([{ created_at: '2026-09-15T11:00:00Z', checksum: 'abc', result: 'success', exists: true }], true, 1, now)).toBe(false)
    expect(shouldRunScheduledBackup([{ created_at: '2026-09-13T11:00:00Z', checksum: 'abc', result: 'success', exists: true }], true, 1, now)).toBe(true)
    expect(shouldRunScheduledBackup([{ created_at: '2026-09-15T11:00:00Z', checksum: '', result: 'success', exists: true }], true, 30, now)).toBe(true)
  })

  it('keeps an unsaved draft guarded when saving fails and permits explicit discard', async () => {
    let dirty = true
    let saveAttempts = 0
    let discarded = false
    const id = 'test-unsaved-draft'
    registerDraft({
      id,
      label: '测试录入',
      isDirty: () => dirty,
      save: async () => { saveAttempts += 1; return false },
      discard: () => { dirty = false; discarded = true },
    })

    expect(dirtyDrafts().map((draft) => draft.id)).toContain(id)
    expect(await saveDirtyDrafts()).toBe(false)
    expect(saveAttempts).toBe(1)
    expect(dirtyDrafts().map((draft) => draft.id)).toContain(id)
    expect(discardDirtyDrafts()).toBe(true)
    expect(discarded).toBe(true)
    expect(dirtyDrafts().map((draft) => draft.id)).not.toContain(id)
    unregisterDraft(id)
  })

  it('waits for tracked work and releases handles after success or failure', async () => {
    const handle = startTask('测试后台任务', 'write')
    expect(activeTasks().some((task) => task.id === handle.id)).toBe(true)
    const waiting = waitForTasks(1_000)
    handle.finish()
    await expect(waiting).resolves.toEqual([])
    expect(activeTasks().some((task) => task.id === handle.id)).toBe(false)

    await expect(runTrackedTask('失败任务', async () => {
      throw new Error('expected failure')
    }, 'backup')).rejects.toThrow('expected failure')
    expect(activeTasks().some((task) => task.label === '失败任务')).toBe(false)
  })
})
