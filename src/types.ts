export type EntityName =
  | 'customers'
  | 'follow_ups'
  | 'contacts'
  | 'projects'
  | 'materials'
  | 'samples'
  | 'sample_tests'
  | 'contracts'
  | 'contract_items'
  | 'quotations'
  | 'orders'
  | 'deliveries'
  | 'statements'
  | 'payments'
  | 'suppliers'
  | 'purchases'
  | 'inventory'
  | 'quotation_items'
  | 'order_items'
  | 'delivery_items'
  | 'statement_items'
  | 'payment_allocations'
  | 'inventory_movements'
  | 'stock_reservations'
  | 'attachments'
  | 'cost_history'

export type DocumentType = 'contracts' | 'quotations' | 'orders' | 'deliveries' | 'statements' | 'payments'

export type CloseBehavior = 'minimize_to_tray' | 'confirm_exit'

export type StatusTone = 'neutral' | 'info' | 'teal' | 'warning' | 'success' | 'danger'

export interface EntityRecord {
  id: number
  [key: string]: unknown
}

export interface EntityPage {
  rows: EntityRecord[]
  total: number
  page: number
  page_size: number
}

export interface DocumentItemRecord {
  id?: number
  [key: string]: unknown
  material_id?: number
  contract_id?: number
  quotation_id?: number
  order_item_id?: number
  order_id?: number
  statement_id?: number
  batch?: string
  qty_grams?: number
  unit_price_cents?: number
  allocated_amount_cents?: number
  amount_cents?: number
  material_name?: string
  material_category?: string
  material_grade?: string
  manufacturer?: string
  note?: string
}

export interface FieldDefinition {
  key: string
  label: string
  required?: boolean
  type?: 'text' | 'number' | 'date' | 'select' | 'textarea' | 'checkbox'
  options?: Array<{ label: string; value: string }>
  placeholder?: string
  defaultValue?: unknown
}

export interface ModuleDefinition {
  key: EntityName
  label: string
  group: string
  icon: string
  searchPlaceholder: string
  columns: Array<{ key: string; label: string; width?: number; kind?: 'status' | 'money' | 'number' | 'date' }>
  fields: FieldDefinition[]
  primaryField: string
  statusField?: string
  archiveable?: boolean
  readOnly?: boolean
  detailType?: DocumentType
}

export interface DashboardData {
  metrics: {
    monthly_sales_cents: number
    monthly_received_cents: number
    unpaid_cents: number
    overdue_count: number
  }
  todos: Array<{
    id: string
    title: string
    subtitle: string
    due_date?: string
    tone: StatusTone
    entity: EntityName
    entity_id: number
  }>
  stages: Array<{
    code: string
    label: string
    count: number
    tone: StatusTone
    customers: EntityRecord[]
  }>
}

export interface BootstrapInfo {
  app_version: string
  schema_version: number
  root: string
  previous_exit_dirty: boolean
  previous_exit_time: string | null
  startup_integrity: string
  recovery_log_path: string
  protection_enabled: boolean
  close_behavior: CloseBehavior
}

export interface NativeBackgroundTask {
  id: number
  kind: string
  label: string
  started_at: string
}

export interface BackupEntry {
  path: string
  checksum: string
  size_bytes: number
  schema_version: number
  result: string
  created_at: string
  exists: boolean
}

export interface RestoreDrillRecord {
  id: number
  backup_path: string
  backup_checksum: string
  backup_created_at: string | null
  started_at: string
  finished_at: string | null
  duration_ms: number | null
  result: string
  error_summary: string
}

export interface OperationalMetric {
  key: string
  label: string
  value: number | null
  target: string
  status: 'pass' | 'fail' | 'insufficient_data' | string
  unit: string
  formula: string
  source: string
  window: string
  sample_size: number
}

export interface OperationalAssessment {
  generated_at: string
  window_start: string
  window_end: string
  metrics: OperationalMetric[]
  latest_restore_drill: RestoreDrillRecord | null
  demo_only?: boolean
}

export interface AttachmentRecord extends EntityRecord {
  object_type: string
  object_id: number
  relative_path: string
  hash: string
  created_at: string
  exists: boolean
  size_bytes: number
  integrity: 'verified' | 'missing' | 'checksum_mismatch' | 'locked' | 'unreadable' | 'invalid_path' | 'demo_only' | string
  encrypted?: boolean
  file_name?: string
  deduplicated?: boolean
  repaired?: boolean
}

export interface ProtectionSetupResult {
  enabled: boolean
  recovery_key?: string
  warning?: string
}
