// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const browserKey = 'plastic-sales-erp-demo-v1'
const browserDictsKey = `${browserKey}-dicts`

function seedCustomer(stage = 'lead') {
  localStorage.setItem(browserKey, JSON.stringify({
    customers: [{ id: 901, name: '浏览器流转测试客户', stage, next_follow_date: '2026-10-06' }],
    follow_ups: [],
    inventory: [],
    inventory_movements: [],
  }))
  localStorage.setItem(browserDictsKey, JSON.stringify([
    { id: 1, type: 'customer_stage', value: 'lead', label: '线索池', sort_order: 10, enabled: true },
    { id: 2, type: 'customer_stage', value: 'contacted', label: '已建立联系', sort_order: 20, enabled: true },
    { id: 3, type: 'customer_stage', value: 'won', label: '定点量产', sort_order: 50, enabled: true },
  ]))
}

describe('browser demo customer transitions', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date(2026, 8, 18, 23, 59))
    localStorage.clear()
    vi.resetModules()
    delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('persists an automatic follow-up with dictionary labels', async () => {
    seedCustomer()
    const { command } = await import('../src/api')
    vi.setSystemTime(new Date(2026, 8, 19, 0, 1))

    await command('transition_entity', {
      entity: 'customers',
      id: 901,
      nextStatus: 'contacted',
      reason: '首次电话沟通',
    })

    const stored = JSON.parse(localStorage.getItem(browserKey) || '{}')
    expect(stored.customers[0].stage).toBe('contacted')
    expect(stored.customers[0].next_follow_date).toBe('2026-10-06')
    expect(stored.follow_ups).toHaveLength(1)
    expect(stored.follow_ups[0]).toMatchObject({
      customer_id: 901,
      content: '客户阶段由“线索池”流转至“已建立联系”。 原因：首次电话沟通',
      follow_date: '2026-09-19',
      next_date: '2026-10-06',
    })
  })

  it('restores the complete snapshot after an illegal jump', async () => {
    seedCustomer()
    const { command } = await import('../src/api')
    const before = localStorage.getItem(browserKey)

    await expect(command('transition_entity', {
      entity: 'customers',
      id: 901,
      nextStatus: 'won',
    })).rejects.toThrow('不允许')

    expect(localStorage.getItem(browserKey)).toBe(before)
  })
})

describe('browser demo quotation actions', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date(2026, 8, 18, 12, 0))
    localStorage.clear()
    vi.resetModules()
    delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('duplicates a quote, compares its snapshot, converts it, and rolls back duplicate order numbers', async () => {
    const { command } = await import('../src/api')
    const copy = await command<{ id: number; no: string; version: number; status: string }>('duplicate_quotation', { quotation_id: 3 })
    expect(copy.no).toBe('零跑PP-GF20-0615-V2')
    expect(copy.version).toBe(2)
    expect(copy.status).toBe('draft')

    await command('save_document_items', {
      document_type: 'quotations',
      document_id: copy.id,
      items: [{ material_id: 4, material_category: 'PP-GF20', material_grade: 'PG4-S01A', manufacturer: '南京聚隆科技股份有限公司', note: '新版本价格', qty_grams: 1000, unit_price_cents: 1120 }],
    })
    const comparison = await command<{ same: boolean; item_differences: unknown[] }>('compare_quotations', { left_id: 3, right_id: copy.id })
    expect(comparison.same).toBe(false)
    expect(comparison.item_differences).toHaveLength(1)

    const order = await command<{ id: number; quotation_id: number; no: string; status: string }>('convert_quotation_to_order', { quotation_id: copy.id, order_no: 'SO-BROWSER-001', delivery_date: '2026-10-01' })
    expect(order).toMatchObject({ quotation_id: copy.id, no: 'SO-BROWSER-001', status: 'pending_confirm' })
    const before = localStorage.getItem(browserKey)
    await expect(command('convert_quotation_to_order', { quotation_id: copy.id, order_no: 'SO-BROWSER-001', delivery_date: '2026-10-02' })).rejects.toThrow('订单号已存在')
    expect(localStorage.getItem(browserKey)).toBe(before)
  })
})

describe('browser demo contract totals', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date(2026, 8, 18, 12, 0))
    localStorage.clear()
    vi.resetModules()
    delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('recomputes the lowercase total after saving contract lines', async () => {
    const { command } = await import('../src/api')
    const contracts = await command<Array<{ id: number; no: string; item_total_cents: number }>>('list_entities', { entity: 'contracts', search: '', status: null })
    const contract = contracts.find((row) => row.no === 'TZJP202609-18')
    expect(contract).toBeDefined()

    const saved = await command<{ total_cents: number }>('save_document_items', {
      document_type: 'contracts',
      document_id: contract?.id,
      items: [{ material_name: 'PP', model: 'PI0-S27A[BK16452]', qty_grams: 200000, unit_price_cents: 1180 }],
    })
    expect(saved.total_cents).toBe(236000)

    const refreshed = await command<Array<{ id: number; item_total_cents: number }>>('list_entities', { entity: 'contracts', search: '', status: null })
    expect(refreshed.find((row) => row.id === contract?.id)?.item_total_cents).toBe(236000)
  })
})

describe('browser demo operational assessment', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date(2026, 8, 18, 12, 0))
    localStorage.clear()
    vi.resetModules()
    delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('tracks backup and restore drill results without replacing browser data', async () => {
    const { command } = await import('../src/api')
    type Assessment = {
      metrics: Array<{ key: string; value: number | null; status: string }>
      latest_restore_drill: { result: string; backup_path: string } | null
    }

    const initial = await command<Assessment>('get_operational_metrics')
    const initialBackupMetric = initial.metrics.find((metric) => metric.key === 'backup_success_rate')
    expect(initialBackupMetric).toMatchObject({ value: null, status: 'insufficient_data' })
    await expect(command('run_restore_drill', { backup_path: './backup/missing.db' })).rejects.toThrow('暂无可用于恢复演练的备份')

    const backup = await command<{ path: string; result: string }>('backup_now')
    expect(backup).toMatchObject({ path: './backup/demo-backup.db', result: 'success' })

    const afterBackup = await command<Assessment>('get_operational_metrics')
    expect(afterBackup.metrics.find((metric) => metric.key === 'backup_success_rate')).toMatchObject({ value: 100, status: 'pass' })

    const drill = await command<{ ok: boolean; result: string; backup_path: string }>('run_restore_drill', { backup_path: backup.path })
    expect(drill).toMatchObject({ ok: true, result: 'success', backup_path: backup.path })

    const afterDrill = await command<Assessment>('get_operational_metrics')
    expect(afterDrill.latest_restore_drill).toMatchObject({ result: 'success', backup_path: backup.path })
    expect(afterDrill.metrics.find((metric) => metric.key === 'restore_success_rate')).toMatchObject({ value: 100, status: 'pass' })
  })
})
