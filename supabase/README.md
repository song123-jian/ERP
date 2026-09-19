# Supabase 可选云端备份与自动同步

本目录负责 ERP 的可选云端备份和追加式业务事件同步，不把 Supabase 变成本地账套的在线主库。当前业务写入仍由 Tauri + SQLite 完成；未登录或断网时记录进入本地待同步队列并继续正常录入。本地启用保护模式后，备份文件是 `ERPENC1` 加密文件并在上传到 Supabase Storage 时保持原样。

## 初始化

1. 在目标 Supabase 项目的 SQL Editor 依次执行 `migrations/20260915_erp_cloud_backup.sql` 和 `migrations/20260919_erp_business_sync.sql`。
2. 在 Authentication 中启用 Email 登录，并为实际使用者创建账号。应用使用 publishable key，禁止把 secret/service-role key 放入前端、绿色目录或 Git。
3. 复制 `.env.example` 为本地 `.env.local`，填写项目 URL 和 publishable key；不要提交 `.env.local`。
4. 启动应用后可直接开启“基础设置 → 云端备份 → 自动同步”。当前无需登录；状态显示“待登录同步”，已有合法会话并创建工作区后会自动上传。

## 数据边界

- 本地 SQLite 是唯一业务事实来源；云端保存本地备份对象、校验元数据和追加式业务事件，不直接改写本地业务表。
- 云端对象路径格式为 `<workspace_uuid>/<device_id>/<backup_file_name>`，Storage bucket 为私有的 `erp-backups`。
- RLS 同时限制工作区、备份元数据和 Storage 对象；所有策略以 `auth.uid()` 和工作区成员关系为准。
- 上传前必须先在本地完成备份；上传失败不影响本地账套。下载后先校验 SHA-256，再写入本地 `backup/`，最后仍通过现有还原流程执行 SQLite 完整性检查。
- 业务事件以 `workspace_id + device_id + local_event_id` 幂等写入 `erp_sync_events`，仅 `authenticated` 工作区成员可读写；未提供匿名写入或前端 service-role 权限。
- 当前版本不从云端下载业务事件，也不做多设备业务记录合并；需要多人协作时应另立同步冲突与权限方案。

## 回滚

关闭“自动同步”即可停止上传，已有待同步事件和本地业务数据都会保留。删除云端备份对象和 `erp_cloud_backups` 元数据不会影响本地账套；迁移脚本创建的表、函数和 bucket 由管理员按项目变更流程单独清理。
