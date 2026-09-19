# P0-05 Windows release 损坏主库还原记录

- 日期：2026-09-15
- 版本：V1.2.3 release
- 范围：本机 Windows，已验证的隔离临时账套；未触碰工作区账套。
- 现场边界：本轮未模拟权限拒绝、杀毒软件占用、U 盘拔出或磁盘耗尽。

## 操作与结果

1. 保留原始活动库作为临时回滚副本后，将 `data/erp.db` 替换为 45 字节无效内容；原有 WAL/SHM 保留，用于覆盖损坏库伴随日志文件的场景。
2. 启动 release，错误页显示“无法打开数据账套”，底层错误为 `file is not a database`，并提供“查看备份并恢复”。证据：`P0-05-damaged-db-error.png`。
3. 打开恢复列表，清单中的 `erp-20260915-194806.832.db` 显示“校验通过”；点击“还原”后出现二次确认，再确认还原。证据：`P0-05-damaged-db-recovery-picker.png`、`P0-05-damaged-db-confirm-restore.png`。
4. 还原完成后回到工作台并显示启动完整性检查正常。证据：`P0-05-damaged-db-restored.png`。
5. 退出并备份后，进程数为 0、`last_exit.status=clean`；活动库大小为 253,952 字节，SQLite `PRAGMA integrity_check` 返回 `ok`；`backup/` 下保留三份 `restore-source-*` 隔离文件，大小分别为 45、32,768、0 字节。

## 校验说明

- 还原前来源备份的 SHA-256 与 `backup/manifest.json` 一致；还原后清单中的两份备份均再次核验一致。
- 还原后的活动库与来源备份不要求字节级相同，SQLite checkpoint/迁移会改变文件布局；本次以来源 checksum 校验和目标 `integrity_check=ok` 作为通过条件。
- 现场截图：`P0-05-damaged-db-error.png`、`P0-05-damaged-db-recovery-picker.png`、`P0-05-damaged-db-confirm-restore.png`、`P0-05-damaged-db-restored.png`。
