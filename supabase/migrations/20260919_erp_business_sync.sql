-- Authenticated, append-only business event synchronization.
-- Local SQLite remains authoritative and retries are idempotent per device/event.

create table if not exists public.erp_sync_events (
  id uuid primary key default gen_random_uuid(),
  workspace_id uuid not null references public.erp_workspaces(id) on delete cascade,
  uploaded_by uuid not null references auth.users(id) on delete restrict,
  device_id text not null check (char_length(btrim(device_id)) between 1 and 120),
  local_event_id text not null check (char_length(btrim(local_event_id)) between 1 and 120),
  entity_type text not null check (char_length(btrim(entity_type)) between 1 and 120),
  entity_id bigint not null,
  operation text not null check (char_length(btrim(operation)) between 1 and 80),
  payload jsonb not null default '{}'::jsonb,
  occurred_at timestamptz not null,
  received_at timestamptz not null default now(),
  unique (workspace_id, device_id, local_event_id)
);

create index if not exists erp_sync_events_workspace_received_idx
  on public.erp_sync_events (workspace_id, received_at desc);
create index if not exists erp_sync_events_workspace_entity_idx
  on public.erp_sync_events (workspace_id, entity_type, entity_id, occurred_at desc);

alter table public.erp_sync_events enable row level security;
alter table public.erp_sync_events force row level security;

drop policy if exists erp_sync_events_select on public.erp_sync_events;
create policy erp_sync_events_select
  on public.erp_sync_events
  for select
  to authenticated
  using ((select private.is_erp_workspace_member(workspace_id)));

drop policy if exists erp_sync_events_insert on public.erp_sync_events;
create policy erp_sync_events_insert
  on public.erp_sync_events
  for insert
  to authenticated
  with check (
    uploaded_by = (select auth.uid())
    and (select private.is_erp_workspace_member(workspace_id))
  );

grant select, insert on public.erp_sync_events to authenticated;

comment on table public.erp_sync_events is
  'Append-only authenticated ERP business events. SQLite remains authoritative; workspace/device/event is idempotent.';
