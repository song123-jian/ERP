-- V1.2.3 optional cloud-backup layer.
-- The local SQLite file remains the system of record. This migration stores
-- backup objects and their metadata (protected accounts remain encrypted); it does not mirror business
-- tables or expose the local database schema through PostgREST.

create extension if not exists pgcrypto;

create schema if not exists private;

create table if not exists public.erp_workspaces (
  id uuid primary key default gen_random_uuid(),
  owner_id uuid not null references auth.users(id) on delete cascade,
  name text not null check (char_length(btrim(name)) between 1 and 120),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);

create table if not exists public.erp_workspace_members (
  workspace_id uuid not null references public.erp_workspaces(id) on delete cascade,
  user_id uuid not null references auth.users(id) on delete cascade,
  role text not null default 'member' check (role in ('owner', 'member')),
  created_at timestamptz not null default now(),
  primary key (workspace_id, user_id)
);

create table if not exists public.erp_cloud_backups (
  id uuid primary key default gen_random_uuid(),
  workspace_id uuid not null references public.erp_workspaces(id) on delete cascade,
  uploaded_by uuid not null references auth.users(id) on delete restrict,
  device_id text not null check (char_length(btrim(device_id)) between 1 and 120),
  object_path text not null check (object_path like '%/%/%'),
  checksum text not null check (checksum ~ '^[0-9a-f]{64}$'),
  size_bytes bigint not null check (size_bytes >= 0),
  schema_version integer not null check (schema_version > 0),
  app_version text not null check (char_length(btrim(app_version)) between 1 and 40),
  created_at timestamptz not null default now(),
  metadata jsonb not null default '{}'::jsonb,
  unique (workspace_id, checksum),
  unique (workspace_id, object_path)
);

create index if not exists erp_workspace_members_user_idx
  on public.erp_workspace_members (user_id, workspace_id);
create index if not exists erp_cloud_backups_workspace_created_idx
  on public.erp_cloud_backups (workspace_id, created_at desc);
create index if not exists erp_cloud_backups_uploaded_by_idx
  on public.erp_cloud_backups (uploaded_by);

create or replace function private.is_erp_workspace_member(p_workspace_id uuid)
returns boolean
language sql
stable
security definer
set search_path = ''
as $$
  select exists (
    select 1
    from public.erp_workspace_members as member
    where member.workspace_id = p_workspace_id
      and member.user_id = (select auth.uid())
  );
$$;

revoke all on function private.is_erp_workspace_member(uuid) from public, anon, authenticated, service_role;
grant execute on function private.is_erp_workspace_member(uuid) to authenticated;

create or replace function public.create_erp_workspace(p_name text)
returns uuid
language plpgsql
security definer
set search_path = ''
as $$
declare
  new_workspace_id uuid;
  current_user_id uuid := auth.uid();
  normalized_name text := btrim(coalesce(p_name, ''));
begin
  if current_user_id is null then
    raise exception 'authentication required' using errcode = '42501';
  end if;
  if char_length(normalized_name) not between 1 and 120 then
    raise exception 'workspace name must contain 1 to 120 characters' using errcode = '22023';
  end if;

  insert into public.erp_workspaces (owner_id, name)
  values (current_user_id, normalized_name)
  returning id into new_workspace_id;

  insert into public.erp_workspace_members (workspace_id, user_id, role)
  values (new_workspace_id, current_user_id, 'owner');

  return new_workspace_id;
end;
$$;

revoke all on function public.create_erp_workspace(text) from public, anon;
grant execute on function public.create_erp_workspace(text) to authenticated;

alter table public.erp_workspaces enable row level security;
alter table public.erp_workspaces force row level security;
alter table public.erp_workspace_members enable row level security;
alter table public.erp_workspace_members force row level security;
alter table public.erp_cloud_backups enable row level security;
alter table public.erp_cloud_backups force row level security;

drop policy if exists erp_workspaces_select on public.erp_workspaces;
create policy erp_workspaces_select
  on public.erp_workspaces
  for select
  to authenticated
  using ((select private.is_erp_workspace_member(id)));

drop policy if exists erp_workspaces_update on public.erp_workspaces;
create policy erp_workspaces_update
  on public.erp_workspaces
  for update
  to authenticated
  using (owner_id = (select auth.uid()))
  with check (owner_id = (select auth.uid()));

drop policy if exists erp_workspace_members_select on public.erp_workspace_members;
create policy erp_workspace_members_select
  on public.erp_workspace_members
  for select
  to authenticated
  using (
    user_id = (select auth.uid())
    or (select private.is_erp_workspace_member(workspace_id))
  );

drop policy if exists erp_cloud_backups_select on public.erp_cloud_backups;
create policy erp_cloud_backups_select
  on public.erp_cloud_backups
  for select
  to authenticated
  using ((select private.is_erp_workspace_member(workspace_id)));

drop policy if exists erp_cloud_backups_insert on public.erp_cloud_backups;
create policy erp_cloud_backups_insert
  on public.erp_cloud_backups
  for insert
  to authenticated
  with check (
    uploaded_by = (select auth.uid())
    and (select private.is_erp_workspace_member(workspace_id))
  );

drop policy if exists erp_cloud_backups_delete on public.erp_cloud_backups;
create policy erp_cloud_backups_delete
  on public.erp_cloud_backups
  for delete
  to authenticated
  using ((select private.is_erp_workspace_member(workspace_id)));

-- A private bucket keeps encrypted SQLite backups out of public URLs. The
-- first path segment is the workspace UUID and is checked by storage RLS.
insert into storage.buckets (id, name, public)
values ('erp-backups', 'erp-backups', false)
on conflict (id) do update set public = false;

drop policy if exists erp_backup_objects_select on storage.objects;
create policy erp_backup_objects_select
  on storage.objects
  for select
  to authenticated
  using (
    bucket_id = 'erp-backups'
    and exists (
      select 1
      from public.erp_workspace_members as member
      where member.workspace_id::text = split_part(name, '/', 1)
        and member.user_id = (select auth.uid())
    )
  );

drop policy if exists erp_backup_objects_insert on storage.objects;
create policy erp_backup_objects_insert
  on storage.objects
  for insert
  to authenticated
  with check (
    bucket_id = 'erp-backups'
    and exists (
      select 1
      from public.erp_workspace_members as member
      where member.workspace_id::text = split_part(name, '/', 1)
        and member.user_id = (select auth.uid())
    )
  );

drop policy if exists erp_backup_objects_delete on storage.objects;
create policy erp_backup_objects_delete
  on storage.objects
  for delete
  to authenticated
  using (
    bucket_id = 'erp-backups'
    and exists (
      select 1
      from public.erp_workspace_members as member
      where member.workspace_id::text = split_part(name, '/', 1)
        and member.user_id = (select auth.uid())
    )
  );

grant select, insert, delete on public.erp_cloud_backups to authenticated;
grant select on public.erp_workspaces, public.erp_workspace_members to authenticated;

comment on table public.erp_cloud_backups is
  'Metadata for optional encrypted SQLite backups; local SQLite remains authoritative.';
