# 改性塑料颗粒销售 ERP 交接文档

## 文档元数据

- 项目名称：改性塑料颗粒销售 ERP
- 最后更新日期：2026-10-07
- 当前阶段：V1.2.8 已增加本地优先的 Supabase 自动同步开关、SQLite 待同步事件队列、合法会话后台重试和云端幂等迁移；未登录或断网不阻断业务写入。V1.2.7 的合同/报价模板、金额自动填充、A4 打印预览、运营评估和恢复演练继续有效；目标 Windows 10/11 的系统无 WebView2、断网、标准账户、U 盘换机/换盘符、真实 Office/杀毒软件占用和原生打印机/PDF 驱动仍待现场
- 适用环境：Windows 开发机；Vue 3 + Vite 浏览器演示；Tauri 2 release 构建
- 版本与分支：应用版本 1.2.8；本地 `main` 分支跟踪 `origin/main`，2026-10-07 已将现有项目提交推送到 GitHub

## 当前项目状态

- 仓库状态：现有项目提交 `2c78ac959ea9f06be3607a18b2ad541e5f8869e9` 已上传至 `https://github.com/song123-jian/ERP.git` 的 `main`；远端提交 SHA 已回读核对一致。本次只上传既有提交并维护交接记录，应用版本和既有验收边界不变。
- 可运行状态：浏览器演示模式可启动并渲染工作台、基础设置、备份、保护模式和 Supabase 云端界面；V1.2.8 源码的前端生产构建、Rust 编译和自动化测试已通过，尚未重建 V1.2.8 Tauri release/绿色包。自动同步默认关闭，开启后业务审计事件与本地业务事务一同写入 `sync_events`；后台每 60 秒及业务写入后尝试同步，未配置、未登录、无工作区或网络失败时只保留队列和状态，不影响本地 SQLite。既有 V1.2.7 release/绿色包、固定 WebView2、恢复与性能验收仍是上一版本有效证据。
- 当前里程碑：完成离线单人 ERP 的销售、客户跟进、样品测试、报价、合同、订单、采购、对账回款和库存基线，以及本地保护、备份和还原流程。
- 关键入口：`src/main.ts`、`src/App.vue`、`src-tauri/src/main.rs`。
- 核心关系：Vue 页面通过 `src/api.ts` 调用 Tauri 命令；Tauri 后端使用 SQLite（WAL）保存账套、审计日志、导入批次、备份记录和附件索引；`follow_ups` 以每个客户最新记录同步 `customers.next_follow_date` 并驱动工作台待办；`sample_tests` 按样品追加保留测试与复测历史；报价、订单、送货、对账和回款通过明细表派生金额；库存通过流水、预占和在途采购派生；CSV 导入先按模块字段字典和类型做预检，再以行级保存点提交或回滚，并输出失败行文件；附件由 Rust 侧校验对象、文件名、路径、大小和 SHA-256，必要时加密后原子写入，下载前重新校验；浏览器演示模式在 `src/api.ts` 中提供等价反馈，并在保存失败时回滚整份演示快照。
- 主要依赖：Vue 3、Naive UI、Tauri 2、rusqlite（bundled/backup）、AES-GCM、Argon2id、CSV。

## 已完成需求

| 日期 | 需求 | 主要变更 | 影响范围 | 验证结果 | 证据 |
| --- | --- | --- | --- | --- | --- |
| 2026-09-12 | 审查并补充 ERP 方案，落实下一步实施内容 | 形成 `ERP_v1.2_基线补充版.docx`；补齐离线账套、安全保护、备份恢复、状态机、CSV 导入导出、审计和界面反馈，并明确原生 PDF/xlsx 与外部路径备份属于后续版本 | 方案文档、Vue 前端、Tauri/Rust 后端、SQLite schema | 前端构建、Rust 构建和单元测试通过；浏览器核心界面检查通过 | `ERP_v1.2_基线补充版.docx`、`src/App.vue`、`src-tauri/src/main.rs` |
| 2026-09-12 | 修复还原异常路径并完成交付验收 | 还原前校验、临时文件清理、替换失败时恢复保护备份；补充无 favicon 请求的本地图标引用 | `src-tauri/src/main.rs`、`index.html` | 7 个 Rust 测试通过；Tauri release 构建成功；清洁浏览器控制台错误为 0 | `src-tauri/target/release/plastic-sales-erp.exe`、`index.html` |
| 2026-09-13 | 实施客户跟进与样品测试闭环 | 新增跟进记录和测试记录页面、SQLite 查询与保存命令；跟进记录保存后同步客户下次跟进日；工作台按客户最新跟进记录计算待办和逾期数；历史记录不提供归档入口；schema v2 迁移保留旧客户提醒 | `src/types.ts`、`src/modules.ts`、`src/api.ts`、`src/components/EntityPage.vue`、`src/components/DashboardView.vue`、`src-tauri/src/main.rs` | Rust 11 项测试通过（含迁移幂等性）；前端类型检查和生产构建通过；Playwright 演示完成跟进录入与工作台联动检查 | `tests` 模块、`pnpm build`、`.playwright-cli/` 快照 |
| 2026-09-13 | 实施交易履约与库存闭环补充 | 补齐旧浏览器演示数据的库存流水迁移；送货单自动生成剩余订单明细并登记 `sale_issue` 出库；库存不足拦截；出库后释放订单预占并同步订单状态；回款禁止重复分配同一对账单；浏览器 `save_entity` 增加快照回滚 | `src/api.ts`、`src/components/EntityPage.vue`、`src-tauri/src/main.rs`、`src-tauri/Cargo.toml` | `pnpm exec vue-tsc --noEmit`、`pnpm test`、`pnpm build`、Rust 16 项测试和 `cargo check` 通过；Playwright 验证正常送货、库存/预占/订单联动、库存不足失败后无残留，控制台错误为 0 | `src/api.ts`、`src/components/EntityPage.vue`、`.playwright-cli/` 快照 |
| 2026-09-13 | 补齐 CSV 导入 P0 与业务参数维护 | 增加导入字段白名单、必填字段、数值/日期预检；每行 SQLite 保存点；默认任一失败整批回滚，也支持失败行跳过；生成带行号和原因的失败 CSV；新增基础设置提醒天数、备份保留份数和状态字典维护；修正设置时间戳重复写入 | `src-tauri/src/main.rs`、`src/api.ts`、`src/components/EntityPage.vue`、`src/styles.css`、`tests/acceptance/P0.md` | 前端 3 项测试、Vite 构建、Rust 25 项测试和 `cargo check` 通过；Playwright 验证设置/字典持久化、默认回滚策略、策略切换和错误导入提示，控制台错误/警告为 0 | `tests/acceptance/P0.md`、`.playwright-cli/page-2026-09-13T05-45-53-597Z.yml`、`.playwright-cli/page-2026-09-13T05-48-44-206Z.yml` |
| 2026-09-13 | 完成 GitHub 发布准备 | 补充公开仓库 README；排除本地运行数据、数据库文件、环境文件和 Playwright 日志；初始化本地 `main` 分支并配置 `origin` 远端，32 个源码与文档文件已暂存待发布 | `.gitignore`、`README.md`、全部源码与文档 | 暂存清单已复核；未执行外部推送；命令行连接 GitHub 443 超时，网页上传入口同样未完成 | `README.md`、`.gitignore`、`git status --short` |
| 2026-09-13 | 实施附件闭环补充 | 新增附件索引和业务记录附件入口；Rust 侧完成对象白名单、对象存在校验、文件名净化、`files/` 相对路径隔离、25 MB 上限、临时文件原子写入、SHA-256 登记、重复内容去重、缺失/篡改检测；保护模式下使用 AES-GCM 加密并在下载前解密校验；浏览器演示只保存索引元数据 | `src-tauri/src/main.rs`、`src/api.ts`、`src/components/EntityPage.vue`、`src/modules.ts`、`src/types.ts`、`src/styles.css`、版本配置 | Rust 29 项测试、前端测试/类型检查/构建、Tauri release 构建通过；启用保护模式会校验并加密历史明文附件，篡改或缺失附件再次上传可原位修复并保留登记 ID；浏览器完成上传、重复去重、刷新持久化和附件索引检查，控制台错误/警告 0 | `tests/acceptance/P0.md`、`src-tauri/target/release/plastic-sales-erp.exe`、`README.md` |
| 2026-09-13 | 实施启动异常恢复闭环 | 启动时读取可追溯退出状态；保护账套先验证本地口令或恢复密钥，再写入 `dirty`；状态、保护配置和加密库在 Windows 使用 `ReplaceFileW` 替换；异常退出后完整性检查、恢复日志和备份入口在界面可见；新增需求追溯矩阵 | `src-tauri/src/main.rs`、`src/App.vue`、`src/types.ts`、`src/api.ts`、`docs/REQUIREMENTS_TRACEABILITY.md` | Rust 30 项测试、前端类型/领域测试/生产构建、Tauri 1.2.3 release 构建通过；回归用例验证首次启动不误报、已有账套缺失状态会告警、错误口令不把 `clean` 改成 `dirty`、成功验证后标记运行中并写恢复日志 | `tests/acceptance/P0.md`、`docs/REQUIREMENTS_TRACEABILITY.md`、`src-tauri/target/release/plastic-sales-erp.exe` |
| 2026-09-13 | 实施未保存表单安全退出守卫 | 新增统一草稿注册表；实体编辑、状态流转、单据明细和回款分配支持保存/不保存/继续编辑；导航、标题栏关闭、托盘退出和 `Ctrl+Q` 先拦截脏草稿；模块键隔离页面实例，明细全局保存避免父列表刷新竞态 | `src/drafts.ts`、`src/App.vue`、`src/components/EntityPage.vue`、`src/components/DocumentItemsModal.vue`、`tests/domain.test.ts` | 前端领域测试 5 项、Vue 类型检查、Vite 构建和 Tauri 1.2.3 release 构建通过；保存失败保持注册项、显式丢弃清理注册项；本机 Windows release 已现场验证 Naive UI 客户表单的取消守卫、继续使用、保存并退出，客户备注已落库并随退出备份保留；托盘菜单退出、保存失败和并发任务场景仍待验 | `tests/acceptance/P0.md`、`docs/REQUIREMENTS_TRACEABILITY.md`、`tests/acceptance/evidence/P0-10-dirty-draft-guard.jpg` |
| 2026-09-13 | 实施后台任务登记与可配置托盘关闭行为 | 新增前端任务注册表和 Rust `TaskRegistry` RAII 租约；启动、备份、导入导出、还原、附件读写、保护迁移等长任务统一登记；退出前等待本地与原生任务，30 秒超时则保留窗口并提示；新增 `close_behavior` 设置，标题栏 × 可最小化到托盘或进入确认，托盘退出与 `Ctrl+Q` 统一走草稿、任务、备份和 clean 状态流程；Vite 忽略 `src-tauri/target`，避免 release 构建的 Windows 文件锁终止开发服务器 | `src/tasks.ts`、`src/api.ts`、`src/App.vue`、`src/types.ts`、`src-tauri/src/main.rs`、`src/styles.css`、`vite.config.ts`、`tests/domain.test.ts` | 前端领域测试 5 项、Rust 33 项、Vue 类型检查、Vite 构建、`cargo check` 和 Tauri 1.2.3 release 构建通过；Playwright 验证工作台加载、关闭行为刷新持久化、`Ctrl+Q` 退出确认和任务标签显示，控制台错误/警告 0；本机 Windows 已验证标题栏隐藏、单实例唤回、强杀恢复和正常退出；托盘菜单、文件占用和真实并发仍待验 | `tests/acceptance/P0.md`、`docs/REQUIREMENTS_TRACEABILITY.md`、`src-tauri/target/release/plastic-sales-erp.exe` |
| 2026-09-13 | 修复原生关闭权限并完成异常恢复现场验收 | 为主窗口新增 Tauri 事件监听、隐藏和销毁的最小 capability；在初始化前注册标题栏关闭与托盘退出监听，原生注册失败时显示错误；以隔离账套验证标题栏 ×、单实例唤回、强杀恢复及正常退出 | `src-tauri/capabilities/main-window.json`、`src/App.vue`、`tests/acceptance/P0.md` | Tauri 1.2.3 release 构建通过；标题栏 × 后进程保持、二次启动仍为唯一实例；强杀重启显示异常退出和 SQLite 正常提示并写入 `unclean_exit_recovered`；`Ctrl+Q` 退出后进程为 0、状态为 `clean`，新备份 253,952 字节、`integrity_check=ok`、记录的 schema 版本为 5 | `tests/acceptance/P0.md`、`tests/acceptance/evidence/P0-02-unclean-exit-recovery.jpg` |
| 2026-09-13 | 完成保护模式 Windows release 现场验收 | 在隔离临时账套验证保护启用后的加密主库/备份、锁定页、错误口令拒绝、正确本地口令解锁及恢复密钥重启解锁；最终正常退出并核验 clean 状态、加密文件头和 SQLite 完整性 | `src-tauri/src/main.rs`、`tests/acceptance/P0.md`、`docs/REQUIREMENTS_TRACEABILITY.md` | 本机 release 现场通过：主库和 4 份备份均为 `ERPENC1` 加密文件；错误口令保持锁定；两种凭据进入工作台；最终进程 0、`last_exit.json.status=clean`、明文主库/WAL/SHM 不存在；解密核验副本 `integrity_check=ok`；日志未发现测试口令或恢复密钥 | `tests/acceptance/evidence/P0-08-protection-lock.jpg`、`P0-08-wrong-password.jpg`、`P0-08-unlocked-workbench.jpg`、`P0-08-recovery-unlocked-workbench.jpg` |
| 2026-09-13 | 补齐保护模式敏感 CSV 导出 | `export_entity` 先在内存生成 CSV；普通账套输出 `.csv`，保护账套使用当前主密钥输出 `ERPENC1` `.csv.enc`；导入失败行报告复用同一策略；统一临时文件、同步和原子替换，前端提示加密状态，并在保护账套未解锁时拒绝导出/导入 | `src-tauri/src/main.rs`、`src/components/EntityPage.vue`、`src/api.ts`、`tests/acceptance/P0.md`、`docs/REQUIREMENTS_TRACEABILITY.md`、`README.md` | Rust 38 项测试、前端 5 项领域测试、`pnpm exec vue-tsc --noEmit`、`pnpm build` 和 `pnpm tauri build` 全部通过；`csv_export_is_atomic_and_protected_exports_are_encrypted`、`protected_failed_import_report_is_encrypted` 与 `protected_export_requires_an_unlocked_master` 验证普通导出、保护导出解密一致性、无临时残留、未解锁拒绝和加密失败行报告 | `src-tauri/src/main.rs`、`tests/acceptance/P0.md`、`docs/REQUIREMENTS_TRACEABILITY.md`、`src-tauri/target/release/plastic-sales-erp.exe` |
| 2026-09-13 | 补齐备份失败闭环 | `backup_database` 为尝试生成独立快照/目标名；快照读取到内存后通过 `sync_all` 和原子替换落盘；失败路径清理临时文件并返回重试提示；登记失败保留已验证备份路径；设置页增加失败提示和重试按钮 | `src-tauri/src/main.rs`、`src/App.vue`、`src/styles.css`、`tests/acceptance/P0.md`、`docs/REQUIREMENTS_TRACEABILITY.md`、`README.md` | Rust 38 项测试、前端 5 项领域测试、`pnpm exec vue-tsc --noEmit`、`pnpm test` 通过；`backup_write_failure_cleans_snapshot_and_keeps_blocking_target_untouched` 验证写入失败清理快照、临时文件和阻塞目标；`backup_metadata_failure_preserves_verified_file_and_reports_retry_path` 验证登记失败保留文件并核验 SQLite 完整性 | `src-tauri/src/main.rs`、`tests/acceptance/P0.md`、`docs/REQUIREMENTS_TRACEABILITY.md` |
| 2026-09-15 | 补齐损坏主库备份恢复闭环 | 启动完整性失败时保留已解锁主密钥；备份写入独立 `backup/manifest.json`；数据库未打开时仍扫描备份；还原前校验 checksum、隔离损坏文件、验证临时库并原子替换；保护账套同步更新 `data/erp.db.enc`；错误页提供备份选择入口 | `src-tauri/src/main.rs`、`src/App.vue`、`src/styles.css`、`tests/acceptance/P0.md`、`docs/REQUIREMENTS_TRACEABILITY.md` | Rust 43 项测试、`pnpm test` 5 项、`pnpm exec vue-tsc --noEmit` 通过；新增 `damaged_uninitialized_database_restores_from_manifest_and_keeps_quarantine`、`protected_restore_rewrites_encrypted_database_shadow`、`manifest_checksum_mismatch_is_rejected_before_restore`、`backup_scan_works_without_an_open_database`、`failed_restore_keeps_corrupt_source_and_cleans_restore_temps` 均通过；真实 Windows 文件占用、权限、损坏库现场仍待验 | `tests/acceptance/P0.md`、`docs/REQUIREMENTS_TRACEABILITY.md`、`src-tauri/src/main.rs` |
| 2026-09-15 | 补齐 P0-11 文件占用错误闭环 | Windows `ReplaceFileW` 与非 Windows 重命名失败统一按文件占用/权限分类；失败时保留目标文件并清理临时文件；新增导出目标和附件目标独占句柄回归用例，释放占用后验证可重试 | `src-tauri/src/main.rs`、`tests/acceptance/P0.md`、`docs/REQUIREMENTS_TRACEABILITY.md` | `cargo fmt --check` 通过；`cargo test --manifest-path src-tauri/Cargo.toml` 45 项通过；真实 Office/杀毒软件占用、并发导出/备份、托盘退出仍待现场 | `tests/acceptance/P0.md`、`docs/REQUIREMENTS_TRACEABILITY.md`、`src-tauri/src/main.rs` |
| 2026-09-15 | 完成隔离便携目录与 release 退出现场验收 | 将 V1.2.3 release 复制到临时目录，验证七个运行目录自动创建、同一可执行文件二次启动保持单实例、强杀后 dirty 恢复提示与日志、`Ctrl+Q` 退出并备份 | 隔离临时账套、`tests/acceptance/evidence/P0-12-portable-release.md`、`P0-12-portable-unclean-recovery.png`、`P0-12-portable-exit-dialog.png` | 本机 Windows 现场通过：第二进程退出且原进程保持唯一；恢复对话框显示完整性正常和日志目录；正常退出后进程数为 0，`last_exit.status=clean`，253,952 字节备份的 manifest checksum 与实际文件一致，SQLite `integrity_check=ok` | `tests/acceptance/P0.md`、`docs/REQUIREMENTS_TRACEABILITY.md`、`tests/acceptance/evidence/P0-12-portable-release.md` |
| 2026-09-15 | 完成普通损坏主库 release 还原现场验收 | 在隔离账套中写入无效 SQLite 内容，保留 WAL/SHM，启动错误页选择备份并确认还原；验证来源 checksum、损坏文件隔离和恢复库完整性 | `src-tauri/src/main.rs`、`src/App.vue`、`tests/acceptance/P0.md`、`docs/REQUIREMENTS_TRACEABILITY.md` | 本机现场通过：错误页显示 `file is not a database`；备份列表显示校验通过；还原后生成 45/32,768/0 字节三份 `restore-source-*`，活动库 `integrity_check=ok`，正常退出后状态为 `clean`、进程数为 0 | `tests/acceptance/evidence/P0-05-damaged-db-release.md`、`P0-05-damaged-db-error.png`、`P0-05-damaged-db-recovery-picker.png`、`P0-05-damaged-db-confirm-restore.png`、`P0-05-damaged-db-restored.png` |
| 2026-09-15 | 完成备份清单占用重试现场验收 | 以独占句柄锁定 `backup/manifest.json`，执行备份并核对失败提示、已生成文件保留；释放锁后点击重试并检查清单、校验值和临时文件 | `src-tauri/src/main.rs`、`src/App.vue`、`tests/acceptance/P0.md`、`docs/REQUIREMENTS_TRACEABILITY.md` | 本机现场通过：设置页显示备份未完成、登记失败原因和重试入口；释放锁后新备份进入列表且 SHA-256 一致，无临时残留，正常退出后状态为 `clean`、进程数为 0 | `tests/acceptance/evidence/P0-04-manifest-lock-release.md`、`P0-04-manifest-lock-backup-error.png`、`P0-04-manifest-lock-backup-retry.png` |
| 2026-09-19 | 复核并完成 Supabase 迁移落库 | 保留 `@supabase/supabase-js` 登录/注册、工作区和私有 Storage 备份实现；在目标项目 `qgsmlvjofwmyjdqjtvum` 的 SQL Editor 正式执行 `supabase/migrations/20260915_erp_cloud_backup.sql`，建立工作区、成员、备份元数据、RLS、安全函数和私有 `erp-backups` bucket | `src/supabase.ts`、`src/components/CloudBackupPanel.vue`、`src/api.ts`、`src-tauri/src/main.rs`、`supabase/`、`.env.example` | 页面返回 `Success. No rows returned`；只读核验确认 3 张业务表均为 `true/true` 强制 RLS、2 个 `SECURITY DEFINER` 函数、6 条业务策略、3 条 Storage 策略和私有 bucket；`erp_workspaces`、`erp_workspace_members`、`erp_cloud_backups` 均为 0 行；本地 `pnpm test` 11 项、`pnpm build` 和既有 Rust/release 验证继续通过 | `supabase/migrations/20260915_erp_cloud_backup.sql`、`supabase/README.md`、Supabase SQL Editor 最终只读核验结果 |
| 2026-09-16 | 补齐列表分页与 V1.2 性能门禁 | 新增 `list_entities_page` 分页响应契约，实体列表接入页码、总数和页大小；新增 `state/ui_ready.json` 前端就绪信号、Rust 10,000 条客户列表 P95 门禁和 Windows release 启动/内存采样脚本；保留全量接口供导出、工作台和查找下拉 | `src-tauri/src/main.rs`、`src/components/EntityPage.vue`、`src/api.ts`、`src/types.ts`、`src/App.vue`、`scripts/measure-release.ps1`、`package.json`、`tests/acceptance/evidence/performance-release.json` | `cargo fmt --check`、`vue-tsc`、`pnpm test`（6 项）、`cargo test`（48 项通过、1 项性能门禁按需执行）、`pnpm tauri build` 通过；release 性能门禁输出 10,000 行/20 样本 P95 15.655ms；隔离 release 启动至 UI 就绪 777.055ms、峰值工作集 28.32MB，报告状态 `passed` | `tests/acceptance/evidence/performance-release.json`、`tests/acceptance/P0.md`、`docs/REQUIREMENTS_TRACEABILITY.md` |
| 2026-09-18 | 补齐 V1.2.5 绿色发布与固定版 WebView2 | 应用启动前识别 `runtime/webview2`；新增官方固定运行时下载、绿色目录/ZIP 打包及逐文件、归档、签名和真实进程路径验证；生成七个运行目录、版本/依赖/许可证及 SHA-256 清单 | `src-tauri/src/main.rs`、`scripts/fetch-webview2-runtime.ps1`、`scripts/package-portable.ps1`、`scripts/verify-portable.ps1`、`README.md`、P0 与追溯文档 | 新增 Rust 定向测试通过；`pnpm tauri build` 通过；261 个静态文件、711,639,352 字节逐项验证，ZIP 内容一致；临时账套 UI 就绪且观察到 6 个 WebView2 进程来自随包目录。当前 ZIP SHA-256 `02F62120BABAB5ED63F0F1D4076C2FD57F6B1F34008C0CFBCFD5EA4FAAA1CB1C` | `tests/acceptance/evidence/P0-01-portable-fixed-runtime.json`、`artifacts/portable/plastic-sales-erp-1.2.5-windows-x64-portable.zip` |
| 2026-09-18 | 统一 V1.2.5 前端版本并完成定时备份 release 验收 | Vite 从 `package.json` 注入版本，演示初始化、UI 和云备份元数据复用同一常量；重建绿色目录/ZIP；隔离账套验证首次自动备份、间隔抑制重复、manifest checksum、任务释放和 clean 退出 | `vite.config.ts`、`src/version.ts`、`src/api.ts`、`src/App.vue`、`src/components/CloudBackupPanel.vue`、P0 与追溯文档 | 类型检查、6 项前端测试、生产构建、Tauri release、绿色包与启动校验通过；UI 就绪版本 `1.2.5`；定时备份在 24.605 ms 后生成，126 秒后仍仅一份，任务列表为空且无临时文件 | `tests/acceptance/evidence/P0-01-portable-fixed-runtime.json`、`tests/acceptance/evidence/P0-14-scheduled-backup-release.json` |
| 2026-09-18 | 按附件补齐合同与报价单模板，并实现合同金额自动填充 | 新增合同/报价字段与明细快照、附件示例数据、合同条款和 A4 打印预览；报价单支持传真抬头、审阅/批注选项、调价说明和页脚联系方式；合同明细按数量×含税单价实时汇总，小写与中文大写自动填充，放弃未保存修改后恢复已保存值，主编辑窗回填已保存汇总；应用版本升至 1.2.7 | `src-tauri/src/main.rs`、`src/api.ts`、`src/modules.ts`、`src/components/DocumentItemsModal.vue`、`src/components/EntityPage.vue`、`src/components/PrintPreviewModal.vue`、`src/print.ts`、`src/styles.css`、`tests/acceptance/P0.md`、`tests/acceptance/evidence/P0-16-contract-quotation.md`、`docs/REQUIREMENTS_TRACEABILITY.md` | 本次 Rust 55 项、前端 10 项、类型检查、生产构建、V1.2.7 Tauri release 构建和格式检查通过；合同金额验证为 `¥2,360.00` / `贰仟叁佰陆拾元整`，改价后联动并在不保存时恢复；此前临时绿色包/ZIP 和隔离启动校验已通过；Playwright 核验两份 A4 预览尺寸 `794×1123`、关键字段和报价编辑复选框可见性 | `tests/acceptance/evidence/P0-16-contract-quotation.md`、`src/print.ts`、`tests/domain.test.ts`、`src-tauri/target/release/plastic-sales-erp.exe` |
| 2026-09-19 | 加固合同总金额大小写自动填充 | 统一合同明细金额汇总函数，明细弹窗和合同打印模板共用同一金额计算；非法金额按 `0` 处理并显示“零元整”；补充浏览器演示保存后重新读取的合同金额回归测试 | `src/print.ts`、`src/components/DocumentItemsModal.vue`、`tests/domain.test.ts`、`tests/api.test.ts`、`docs/REQUIREMENTS_TRACEABILITY.md`、`tests/acceptance/P0.md` | `pnpm test` 11 项通过；`pnpm exec vue-tsc --noEmit` 通过；`pnpm build` 通过；合同示例保存后仍返回 `236000` 分并可重新读取，小写 `¥2,360.00` 与大写 `贰仟叁佰陆拾元整` 的转换回归通过 | `tests/domain.test.ts`、`tests/api.test.ts`、`src/print.ts` |
| 2026-09-19 | 接入运营评估与恢复演练 | 将 `OperationalAssessmentPanel` 挂载到“基础设置 → 运营评估”；展示跟进超期率、备份成功率、还原成功率、RPO、RTO 和数据完整率；提供校验通过备份选择、临时 SQLite 恢复演练、错误提示和最近结果；补充响应式样式与浏览器演示命令回归 | `src/App.vue`、`src/components/OperationalAssessmentPanel.vue`、`src/styles.css`、`src/api.ts`、`tests/api.test.ts`、`tests/acceptance/P0.md` | `pnpm exec vue-tsc --noEmit`、`pnpm test`（12 项）、`pnpm build`、`cargo test`（60 通过、1 项性能门禁忽略）通过；本地浏览器可打开运营评估页签并完成一次演练，显示“演练通过”且主库未被替换；页面控制台错误为 0；`cargo fmt --check` 仍受既有 Rust 未格式化差异影响，本轮未格式化无关文件 | `tests/acceptance/evidence/P0-17-operational-assessment.md`、`src/components/OperationalAssessmentPanel.vue`、`tests/api.test.ts` |
| 2026-09-19 | 增加无需立即登录的自动同步并准备仓库发布 | SQLite schema v9 新增 `cloud_sync_enabled` 与可靠事件队列；设置页提供自动同步开关、待登录/同步中/已同步/失败状态和可选登录入口；合法 Supabase 会话按工作区上传追加式事件，使用设备与本地事件幂等键和强制 RLS；正文采用微软雅黑，合同/报价标题及重点金额采用宋体加粗；版本升至 1.2.8 | `src-tauri/src/main.rs`、`src/cloudSync.ts`、`src/supabase.ts`、`src/api.ts`、`src/App.vue`、`src/components/CloudBackupPanel.vue`、`src/styles.css`、`supabase/migrations/20260919_erp_business_sync.sql` | `cargo test` 62 项通过、1 项按需性能门禁忽略；`cargo check`、`cargo fmt --check`、`pnpm test` 12 项、`pnpm exec vue-tsc --noEmit` 和 `pnpm build` 通过；远端新增迁移尚未执行，真实账号上传仍未验收 | `automatic_sync_queue_is_opt_in_and_keeps_retry_state`、`supabase/migrations/20260919_erp_business_sync.sql`、`README.md` |
| 2026-10-07 | 上传当前项目到 GitHub 仓库 | 将现有项目提交 `2c78ac959ea9f06be3607a18b2ad541e5f8869e9` 常规推送到 `origin/main`，建立分支跟踪并更新本交接记录 | 原有 75 个变更文件、`docs/HANDOFF.md`、本地分支跟踪配置 | 完成进度：100%，按“现有项目推送且远端回读一致、交接记录写入核验”两项验收（2/2）；`git push --set-upstream origin main` 退出 0，`git ls-remote --refs origin refs/heads/main` 与代码提交 SHA 一致；交接记录回读、定向差异、UTF-8 无 BOM 和 CRLF 核验通过；本次未改程序，不重跑既有应用测试；上传需求无未完成项或阻塞 | 仓库 `https://github.com/song123-jian/ERP`、上述代码提交 SHA、推送与远端回读命令、本文件；既有应用限制和待验项保留 |

## 合同与报价附件模板本轮补充

- 合同示例 `TZJP202609-18` 已保存供方南京聚隆、需方台州骏普、PP/`PI0-S27A[BK16452]`、200 KG、11.8 元/KG、2,360 元、款到发货、25KG/袋、十项条款、双方地址和账号等字段；合同明细保存为独立快照，金额由明细派生，编辑时自动显示小写和中文大写汇总。`src/print.ts` 的 `contractTotalCents` 统一明细弹窗与打印模板的计算，非法输入按零金额显示“零元整”。
- 报价示例 `零跑PP-GF20-0615` 已保存 PP-GF20/`PG4-S01A`、10.6 元/KG、未税含运费、传真抬头、页数、审阅/批注、调价说明和页脚联系方式；报价明细保存材料类别、牌号、厂家和备注快照。
- `src/print.ts` 负责 HTML 模板、金额大写和字段转义，`DocumentItemsModal.vue` 与 `EntityPage.vue` 负责合同金额实时回填，`PrintPreviewModal.vue` 以已保存业务快照渲染 A4 预览；浏览器验收记录在 `tests/acceptance/evidence/P0-16-contract-quotation.md`。

## 关键决策与约束

- 采用单人、离线、便携账套架构，数据根目录包含 `data`、`backup`、`export`、`files`、`logs`、`state` 和 `config`。
- 保护模式使用 Argon2id 派生口令密钥、AES-256-GCM 加密主库和备份，并生成独立恢复密钥；恢复密钥只在首次设置时展示。
- 备份使用 SQLite 一致性快照并登记 SHA-256；校验不匹配的已登记备份禁止还原，还原前自动创建保护备份。
- 备份失败闭环使用独立临时快照、内存序列化、`sync_all` 和原子替换；快照或目标写入失败会清理本次临时文件且不覆盖既有目标，登记失败会保留已生成文件并返回完整路径与重试提示。
- 每次成功备份同步写入 `backup/manifest.json`，保存路径、checksum、大小、schema 版本和创建时间；数据库未打开时恢复流程优先使用清单和文件扫描，清单/数据库记录不一致时拒绝还原。
- 损坏主库还原先隔离 `data/erp.db`、WAL/SHM 和 `data/erp.db.enc`，目标备份在临时 SQLite 中完成迁移、完整性检查和 checkpoint 后才替换；保护账套还原后重新生成加密 shadow，替换失败会恢复 active/encrypted 快照。
- 业务状态必须通过允许的状态机流转；取消、拒绝、失败和复测等变更需要原因。直接保存和 CSV 导入沿用相同约束。
- 当前版本明确支持 CSV 导入导出；Excel/WPS 文件需要先另存为 UTF-8 CSV。浏览器演示模式不写入真实本地文件，也不执行真实还原。
- `follow_ups` 和 `sample_tests` 允许更正编辑，但不提供归档入口；每次新增样品测试或复测保留为独立记录。工作台只使用每个客户按跟进日期排序后的最新记录，避免历史计划重复计入逾期。
- SQLite schema v2 为已有客户的 `next_follow_date` 生成一条历史跟进记录，并建立跟进与样品测试的查询索引；已存在跟进记录的客户不重复回填。
- 库存流水以 `opening`、采购入库、销售出库和调整类型作为现有库存的唯一派生来源；旧浏览器演示快照缺少流水时按当前库存重建期初流水，并校正 `opening_grams` 避免重复累计。
- 送货单保存时若填写送货日期，会按订单剩余数量自动生成明细并登记销售出库；校验包含在手、在途、其他订单预占和当前订单预占，成功后释放当前订单预占并同步订单为已送货或已回签。
- 浏览器演示模式的 `save_entity` 使用保存前快照，送货、明细、回款分配或派生校验任一失败时恢复内存和 `localStorage`，避免半成品记录残留。
- 同一笔回款在浏览器演示模式下不能重复分配到同一张对账单；对账余额在初始化、明细和回款变化后重新派生。
- 附件对象类型限定为已登记业务表；文件名只保留安全文件名，路径必须位于 `files/` 下并按 `plain/` 或 `encrypted/` 存储；单文件上限 25 MB。登记前计算 SHA-256，同一对象和相同内容只保留一份；列表读取文件并显示 `verified`、`missing`、`checksum_mismatch`、`locked` 等状态，内部生成名不会替代原始文件名。启用保护模式时先校验并加密历史明文附件，再写入保护状态；保护模式下附件保存为 AES-GCM 密文，下载必须解锁并通过哈希校验；关闭保护模式时先逐个解密、校验并原子迁移到 `plain/`，迁移失败会清理暂存明文且不移除保护配置；数据库事务失败会清理已写入的文件。浏览器演示模式只保存索引，不伪造本地文件，状态显示为 `demo_only`。
- 已有账套的退出状态文件缺失、损坏或不是 `clean` 时，启动恢复流程会标记为异常；首次创建账套不会因此误报。保护账套在验证口令或恢复密钥之前不写入 `dirty`，避免错误口令污染此前的正常退出记录。验证成功后才写运行中状态，SQLite 完整性检查通过时将恢复事件写入按日滚动的 `logs/erp-YYYYMMDD.log`；恢复对话框展示上次状态时间、检查结果、日志目录和备份入口，但不会自动还原。
- 未保存表单守卫使用进程内草稿注册表覆盖实体编辑、状态流转、单据明细和回款分配；全局保存失败或校验失败时保持原页面和注册项，明确丢弃后才允许导航或退出。模块切换按 `activeKey` 重建页面实例；全局明细保存不触发父列表异步刷新。
- 长任务使用前端 `src/tasks.ts` 与 Rust `TaskRegistry` 双层登记；命令异常和正常返回都会释放租约，退出流程先等待前端任务再轮询原生任务，超时不强制终止任务而是保留窗口并提示用户。
- 关闭行为写入 `settings` 表的 `close_behavior` 键，默认 `minimize_to_tray`；标题栏 × 按设置最小化到托盘或打开退出确认，托盘菜单退出和 `Ctrl+Q` 始终执行草稿检查、任务等待、备份和 clean 状态写入。
- Windows 上的退出状态、保护配置和加密库替换使用 `ReplaceFileW`，避免先删除旧文件再改名造成状态文件短暂缺失；其他平台保持同目录重命名行为。
- CSV 导入仅对客户、材料、报价、订单、对账和采购提供模板与导入；首行字段必须匹配白名单。`skip` 策略只提交有效行，`rollback` 策略遇到任一错误整体回滚；失败行报告在普通账套输出 `export/import-failed-<batch_id>.csv`，保护账套输出 `.csv.enc` 并使用 `ERPENC1` 封装。业务导出同样先在内存生成，保护账套不留下明文 CSV 临时文件；启用保护前已经生成的旧版明文导出不会自动迁移。
- 保护模式敏感导出由 `write_csv_payload` 统一处理：先序列化到内存，再使用当前账套主密钥加密，采用 `sync_all` 与原子替换写入；命令返回 `encrypted`/`failed_file_encrypted`，前端明确提示加密状态。
- 业务列表与导出分离：`list_entities_page` 为界面提供页码、总数和 1–100 的页大小上限，避免表格一次接收全量响应；导出、工作台统计和关联下拉仍调用全量查询，保持业务结果一致。筛选条件在当前后端列表路径统一应用后再分页。
- V1.2 性能门禁使用 release 优化构建、10,000 条客户数据和 20 个样本计算列表 P95；启动通过 `state/ui_ready.json` 标记前端完成初始化，`scripts/measure-release.ps1` 在隔离副本采样启动时延和工作集，报告以 JSON 保存并在结束时清理临时进程与目录。
- 基础设置中的提醒天数（0–365）参与对账状态派生；备份保留份数（1–100）参与旧备份清理；字典 code 为稳定标识，只能维护显示名、排序和启用状态。
- 本轮修正 `save_settings` 使用同一个 `updated_at` 值写入数据库和返回结果，避免一次保存产生两个不同时间戳。
- 公开仓库只提交源码、文档和构建所需锁文件；`.gitignore` 排除本地账套、备份、导出、日志、测试产物、数据库文件和环境文件；Supabase 配置仅通过本地 `.env.local` 注入 publishable key，云端只保存本地备份对象和校验元数据，保护模式下对象保持加密。
- Supabase 采用“本地 SQLite 为主、云端可选备份与追加式事件同步”边界；自动同步默认关闭，未登录或断网时队列留在本地；云端事件以 `workspace_id + device_id + local_event_id` 幂等写入并由强制 RLS 限制为工作区成员。当前不从云端回放事件，也不做多设备合并或冲突解决。
- Supabase 迁移脚本已在目标项目执行；2026-09-15 经整份脚本事务回滚演练后按用户确认正式执行，并通过最终只读核验；当前没有创建应用账号、上传本地备份或执行真实下载还原，现场验证需在填写 `.env.local` 后完成。
- 绿色发布采用固定版 WebView2 随包方案：源码不保存约 700 MB 的运行时，下载脚本固定版本、官方 URL 和 CAB SHA-256，打包时复核 Microsoft 签名；应用仅在同级运行时主程序存在时设置 `WEBVIEW2_BROWSER_EXECUTABLE_FOLDER`，开发环境可继续使用系统运行时。
- 本次以全局用户级 `AGENTS.md` 为规则来源，未采用项目目录中的局部 `AGENTS.md`。

## 运行与验证

必要环境：开发和构建需要 Node.js/npm、pnpm、Rust/Cargo 及系统 Windows WebView2；V1.2.5 绿色交付目录自带固定版 WebView2，不要求目标机预装开发工具。

| 命令或检查 | 范围 | 状态与关键结果 |
| --- | --- | --- |
| `pnpm exec vue-tsc --noEmit` | 前端 TypeScript 类型检查 | 通过 |
| `pnpm test` | 前端领域测试、浏览器 API、草稿守卫、任务等待和合同/报价打印模板 | 通过，12 个测试通过 |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 迁移、自动同步队列启停与重试、状态机、金额派生、加密、备份与失败清理、跟进与样品测试历史、工作台待办计算、库存与单据闭环、CSV、附件、合同/报价快照、启动恢复、任务登记、损坏主库恢复、分页及 Windows 文件占用回归 | 通过，62 个测试通过；1 个 release 性能门禁按需执行 |
| `cargo check` | Rust 编译检查 | 通过 |
| `pnpm build` | `vue-tsc` 与 Vite 生产构建 | 通过；主 JS chunk 1,147.75 KB，存在单个 chunk 超过 500 KB 的非阻断警告 |
| `pnpm tauri build` | 前端打包与 Tauri release 编译 | 通过；生成 V1.2.7 `src-tauri/target/release/plastic-sales-erp.exe`，文件版本和产品版本均为 `1.2.7` |
| `cargo test --manifest-path src-tauri/Cargo.toml tests::packaged_webview2_runtime_requires_the_runtime_executable -- --exact` | 随包运行时目录识别边界 | 通过，实际执行 1 项；缺少主程序时不选用，存在 `runtime/webview2/msedgewebview2.exe` 时返回固定目录 |
| `pnpm run runtime:webview2` | 官方固定版运行时下载、CAB 哈希、版本和 Authenticode 签名 | 通过；版本 `153.0.4234.32`、x64、257 个文件，CAB SHA-256 固定且签名为 Microsoft/Valid |
| `pnpm run package:portable` | 绿色目录、七个运行目录、依赖/许可证与发布哈希清单、ZIP | V1.2.7 临时隔离包通过；ZIP 313,684,446 字节，归档 SHA-256 `22DDE81BEC3FE6B229AC9F6BFDC5456C33F94C61085AFEB494CDCDB5DDEBD19F`，清单 SHA-256 `EFCE31DB5C08834901416209BB8E8D68F14D7FA5C2F3D55D62CEDD4C4942FF38` |
| `verify-portable.ps1 -LaunchCheck` | 目录逐文件哈希、ZIP 同内容、空运行目录、可执行文件/运行时版本与签名、临时账套启动及实际 WebView2 进程路径 | V1.2.7 临时隔离包通过；261 个静态文件共 711,737,656 字节，UI 就绪版本 `1.2.7`，观察到 7 个运行时进程来自随包目录，归档校验为 `true` |
| 隔离 V1.2.5 release 定时备份探针 | 默认设置、首次调度、60 秒后重复抑制、manifest checksum、原生任务快照与 clean 退出 | 通过；UI 就绪后 24.605 ms 生成 253,952 字节 schema 6 备份，126 秒后仍为一份；实际 SHA-256 一致、无临时文件，调度前后及退出后的任务列表为空 |
| `cargo test --manifest-path src-tauri/Cargo.toml --release customers_ten_thousand_list_p95_stays_under_three_hundred_ms -- --ignored --nocapture` | release 优化构建、10,000 条客户列表、20 次查询样本和分页响应 | 通过；输出 `dataset_rows=10000`、`page_size=20`、`samples=20`、`p95_ms=15.655`、`target_ms=300` |
| `pnpm run perf:release` | 隔离 release 目录启动、前端 UI 就绪信号和 20 次工作集采样 | 通过；`startup_ms=777.055`、`peak_working_set_mb=28.32`、目标 3,000ms/200MB；报告见 `tests/acceptance/evidence/performance-release.json` |
| 本机 Windows release P0-02/P0-03/P0-04/P0-05/P0-10/P0-12 | 隔离便携目录、单实例唤回、强杀重启、损坏库还原、文件占用重试、恢复提示与日志、`Ctrl+Q` 正常退出及脏草稿三分支 | 通过；隔离目录自动创建七个运行目录；第二次启动后第二进程退出；损坏库还原保留三份隔离文件并通过 `integrity_check`；manifest 占用时显示登记失败并支持释放后重试；退出后状态为 `clean`，备份 checksum 一致、进程为 0 |
| 侧面浏览器本地演示 | 客户附件入口、文件选择、重复去重、刷新后索引、全局附件索引 | 通过：上传 `ERP.docx` 后显示 `demo_only`，重复选择提示“相同附件已存在，未重复复制”，刷新后记录保留；控制台错误/警告 0 |
| `tests/acceptance/P0.md` | P0-01 至 P0-16 验收场景、证据和现场验证边界 | P0-01 已取得随包固定运行时启动证据；P0-02、P0-03、P0-04、P0-05、P0-12、P0-14 已完成本机 release 现场验收或其主要路径；P0-16 已完成浏览器合同/报价模板验收；其余条目保留各自自动化或局部现场状态；断网且系统无 WebView2、磁盘故障、权限/Office/杀毒软件占用、托盘退出、保存失败、并发任务和原生打印仍待目标环境执行 |
| `cargo fmt --manifest-path src-tauri/Cargo.toml --check` | Rust 格式检查 | 通过 |
| `list_entities_page` 分页契约 | 实体列表页码、总数、筛选、页大小上限和浏览器演示兼容 | 通过；Rust 分页边界测试、Vue 类型检查和 release 构建通过；导出/工作台全量路径保持不变 |
| Supabase SQL Editor 迁移与只读核验 | 目标项目 `qgsmlvjofwmyjdqjtvum` 的云备份 schema、RLS、函数和 Storage bucket | 整份迁移在 `BEGIN ... ROLLBACK` 中演练成功，正式执行返回 `Success. No rows returned`；最终只读核验确认 `pgcrypto`、`private` schema、3 张强制 RLS 表、2 个固定空 `search_path` 的 `SECURITY DEFINER` 函数、6 条业务策略、3 条 Storage 策略和私有 `erp-backups` bucket；三张业务表均为 0 行 |

## 便携 release 隔离现场验收补充

- 使用 V1.2.3 release 的独立副本启动临时账套，自动创建 `backup`、`config`、`data`、`export`、`files`、`logs`、`state` 七个目录；未使用工作区已有账套。
- 同一可执行文件第二次启动后第二进程退出，原进程保持唯一；该轮只验证单实例，不等同于托盘菜单唤起已验收。
- 强制结束进程前 `last_exit.json` 为 `dirty`；再次启动显示“检测到异常退出”和“已完成启动完整性检查”，检查结果为“正常”，日志写入 `unclean_exit_recovered`。现场截图为 `tests/acceptance/evidence/P0-12-portable-unclean-recovery.png`。
- 选择“退出并备份”后进程数为 0，`last_exit.json.status=clean`；生成 253,952 字节备份，`backup/manifest.json` 的 SHA-256 与实际文件一致，SQLite `PRAGMA integrity_check` 返回 `ok`。退出对话框和可复核命令记录在 `tests/acceptance/evidence/P0-12-portable-release.md` 与 `P0-12-portable-exit-dialog.png`。
- 该轮环境是本机 Windows，未验证断网、无 WebView2、换机/U 盘、真实磁盘空间不足、目录权限拒绝、杀毒软件或 Office 文件占用；这些边界仍保持待现场状态。

## V1.2.5 绿色发布补充

- `scripts/fetch-webview2-runtime.ps1` 固定获取 Microsoft Edge WebView2 Fixed Version Runtime `153.0.4234.32` x64；CAB SHA-256 为 `2CB653A74426F0AA802C2396775C6BC674FD662D5396BD677F47BFA6E12EBA9C`，运行时主程序 Authenticode 状态为 `Valid`、签发者为 Microsoft Corporation。
- 绿色目录包含 `plastic-sales-erp.exe`、`runtime/webview2/`、七个空运行目录、`PORTABLE-README.txt`、`licenses/DEPENDENCIES.json`、`THIRD-PARTY-NOTICES.txt`、`release-manifest.json` 及其 SHA-256。源码仓库通过 `.gitignore` 排除约 700 MB 运行时和发布产物。
- 最终目录共有 263 个文件、711,709,416 字节；其中清单覆盖 261 个静态文件、711,639,352 字节。统一前端版本来源后重建的发布清单 SHA-256 为 `4A92FDD42F5EBF87F1387A72DE3F285F6A5DA0DA20B0DA6AFA5BE8B8444DDBE4`，ZIP 为 313,654,024 字节，SHA-256 为 `02F62120BABAB5ED63F0F1D4076C2FD57F6B1F34008C0CFBCFD5EA4FAAA1CB1C`。
- 启动验证在临时目录复制应用并以目录联接引用交付运行时，未写入交付账套；`state/ui_ready.json` 返回版本 `1.2.5`，观察到 6 个 `msedgewebview2.exe` 进程的实际路径位于交付目录 `runtime/webview2`。本机仍安装系统 WebView2，且该轮未断网，因此系统无 WebView2 和断网目标机验收仍待执行。

## 损坏主库 release 现场验收补充

- 在同一隔离账套中将活动 `data/erp.db` 替换为 45 字节无效内容并保留 WAL/SHM；release 启动错误页显示 `file is not a database`，提供“查看备份并恢复”。
- 恢复列表读取 `backup/manifest.json`，来源备份显示校验通过；确认还原后系统生成三份 `restore-source-*` 隔离文件，分别保留损坏主库及 32,768/0 字节 WAL/SHM。
- 还原后的活动库为 253,952 字节，SQLite `PRAGMA integrity_check` 返回 `ok`；正常退出后进程数为 0、`last_exit.status=clean`。来源备份和清单 checksum 均复核一致。
- 证据：`tests/acceptance/evidence/P0-05-damaged-db-release.md`、`P0-05-damaged-db-error.png`、`P0-05-damaged-db-recovery-picker.png`、`P0-05-damaged-db-confirm-restore.png`、`P0-05-damaged-db-restored.png`。本轮未覆盖权限拒绝、杀毒软件占用和 U 盘拔出。

## 备份清单占用现场验收补充

- 使用辅助进程以共享读方式锁定 `backup/manifest.json`，独占打开检查确认锁定有效；执行“立即备份”后，新的数据库备份文件已生成并保留，但清单登记失败。
- “基础设置 → 备份与恢复”显示“备份未完成”，同时说明文件已生成、清单写入失败、文件路径和“重试备份”入口；备份列表仍能读取并显示该文件的校验值。
- 释放锁定进程后点击“重试备份”，新文件进入 `manifest.json` 和备份列表，SHA-256 与实际文件一致；目录无 `.tmp`、`.snapshot`、`.part` 残留，正常退出后 `clean` 状态和 SQLite 完整性均通过。
- 证据：`tests/acceptance/evidence/P0-04-manifest-lock-release.md`、`P0-04-manifest-lock-backup-error.png`、`P0-04-manifest-lock-backup-retry.png`。该模拟不替代 Office/杀毒软件、权限或磁盘空间故障注入。

## Supabase 云端备份本轮补充

- `supabase/migrations/20260915_erp_cloud_backup.sql` 已在目标 Supabase 项目的 SQL Editor 完成事务回滚演练和一次正式执行，两次均返回 `Success. No rows returned`；脚本只创建或更新 `erp_workspaces`、`erp_workspace_members`、`erp_cloud_backups`、安全函数、RLS 策略和私有 `erp-backups` bucket，不写入业务记录或本地备份文件。
- 最终远程只读核验确认 `pgcrypto` 扩展与 `private` schema 存在；3 张表均存在、启用并强制 RLS；`private.is_erp_workspace_member` 与 `public.create_erp_workspace` 均为 `SECURITY DEFINER` 并固定空 `search_path`；6 条业务表策略和 3 条 Storage 对象策略齐全；bucket `public=false`，其 `file_size_limit` 和 `allowed_mime_types` 仍为 `null`；工作区、成员和备份表计数均为 0。
- 前端通过 `src/supabase.ts` 提供 Email 登录/注册、工作区、上传、下载 checksum 校验和删除；上传对象必须来自本地已校验备份，下载经 SHA-256 校验后才由 Rust `stage_cloud_backup` 原子写入本地 `backup/`。
- 运行前需复制 `.env.example` 为本地 `.env.local` 并填写 publishable key；禁止把 secret/service-role key 写入前端或仓库。真实账号登录、网络中断、云端上传/下载、删除回滚和跨设备恢复仍须按 P0-13 现场验证。
- V1.2.8 新增 `supabase/migrations/20260919_erp_business_sync.sql`，定义仅认证成员可读写的追加式 `erp_sync_events` 表和幂等约束；该迁移尚未在目标 Supabase 项目正式执行，因此现阶段只验证了本地队列、类型、构建和 SQL 静态内容，未把远端同步描述为可用。
- 自动同步开关默认关闭；开启后未登录状态显示“待登录同步”，事件保留在 SQLite。登录、工作区、网络和远端表均可用时批量上传并标记本地事件完成，失败会累计重试次数和最近错误；本地业务提交不依赖远端响应。

## 列表分页与性能门禁本轮补充

- 实体列表页面通过 `list_entities_page` 获取 `rows`、`total`、`page` 和 `page_size`，页大小限制为 1–100；搜索或状态筛选会将页码重置为 1，翻页和改变页大小会重新加载当前模块。
- 原有 `list_entities` 全量命令保留给 CSV 导出、工作台汇总和表单关联下拉，避免改变导出内容和业务派生口径；分页命令对同一筛选结果再做窗口切片，当前实现保证响应大小边界，后续如数据规模继续增长可将筛选和窗口进一步下推 SQL。
- Rust release 性能门禁使用 10,000 条客户记录，预热后采集 20 次列表调用并计算 P95；2026-09-16 输出 `15.655ms`，目标为 `≤300ms`。测试命令为 `cargo test --manifest-path src-tauri/Cargo.toml --release customers_ten_thousand_list_p95_stays_under_three_hundred_ms -- --ignored --nocapture`。
- `initialize` 成功后由前端调用 `mark_ui_ready` 写入 `state/ui_ready.json`；`scripts/measure-release.ps1` 将 release 可执行文件复制到临时隔离目录，等待该信号后采样 20 次工作集并清理进程和目录。2026-09-16 启动至 UI 就绪 `777.055ms`、峰值工作集 `28.32MB`，报告为 `passed`，证据见 `tests/acceptance/evidence/performance-release.json`。
- 该轮只代表当前 Windows 机器、当前 release 和 10,000 条客户数据集；不同 Windows/WebView2、真实 U 盘写入速度、大附件和目标设备仍需复测，不能由本机样本外推全部环境。

## 已知限制、风险、阻塞和待办

- 尚未实现原生 PDF 文件生成；合同和报价单已提供浏览器 A4 打印预览，可通过系统打印/另存为 PDF，真实打印机和 PDF 驱动仍需现场验收。
- CSV 导入的桌面文件选择器、真实文件权限、整批回滚报告和导入大文件性能尚未在发布版现场演练；自动化证据覆盖内存数据库与浏览器演示入口。
- 附件的真实 Windows 文件权限、杀毒软件占用、保护模式解锁后的桌面上传/下载和跨设备拷贝尚未现场演练；浏览器演示只验证索引，不代表本地文件已写入。保护模式主库、备份、错误口令拒绝及本地口令/恢复密钥解锁已在本机隔离临时账套验证，敏感 CSV 导出已自动验证加密和无明文临时文件，但多设备、U 盘保管和导出文件现场保管仍待演练。
- 未在真实 Windows 10/11 多台设备、断网切换、U 盘换盘符和实际用户数据迁移场景做现场验证；当前证据来自本机浏览器演示、Rust 测试和 Windows release 编译。
- `src-tauri/tauri.conf.json` 的 `bundle.active` 当前为 `false`，不生成安装包或自动更新包；V1.2.5 已通过项目脚本生成绿色目录和 ZIP，并随包提供固定版 WebView2。
- Vite 报告主 JS chunk 1,093.77 KB；目前不影响构建和运行，但会增加首次加载体积。
- `cargo clippy --all-targets -- -D warnings` 仍受既有压缩式业务代码的 Clippy 告警影响，未作为本次构建门禁；应在单独代码质量任务中处理。
- 损坏主库还原已增加独立清单、checksum 拒绝、原文件隔离、临时库校验、保护 shadow 同步和失败回滚自动化覆盖，但没有操作系统级故障注入测试；实际磁盘权限、杀毒软件锁文件等场景仍需现场演练。备份失败的快照/写入/登记路径已有自动化覆盖，真实磁盘空间不足、目录权限和杀毒软件占用仍需现场演练。
- 异常退出恢复的状态、日志、界面提示和真实 release 强杀恢复已经验证；普通损坏库的真实备份选择与还原、manifest 文件占用重试已完成本机现场验证，权限拒绝、U 盘拔出和真实 Office/杀毒软件文件占用仍需按 P0-04、P0-05 现场演练。
- 未保存表单守卫和后台任务等待已有注册表、测试和构建级证据；本机已验证 Naive UI 客户表单的取消守卫、继续使用、保存并退出、备注落库以及退出备份；Windows 独占句柄回归已验证导出/附件占用时不覆盖目标、清理临时文件并返回重试提示。托盘菜单退出、保存失败、并发导出/备份和真实 Office/杀毒软件占用仍需按 P0-10、P0-11 及第 5.2 节现场演练。等待超时不会强制终止任务，窗口会保持打开。
- Supabase 既有云端备份 schema 已建立；`20260919_erp_business_sync.sql` 仍待在目标项目执行。尚未创建应用账号、上传真实业务事件、本地备份或执行云端下载还原现场验证。
- 未执行生产部署或真实业务数据写入；V1.2.8 Tauri release、绿色包及目标 Windows 现场验收尚未生成。

## 运行恢复与回滚说明

1. 启动时若发现保护配置，先使用本地口令或恢复密钥验证；验证失败不会改写已有 `clean` 状态，也不会打开账套。验证成功后才写入 `dirty` 并继续加载。
2. 启动完整性失败时，在错误页打开备份列表；列表可在数据库未初始化时读取 `backup/manifest.json` 和备份文件。选择备份后先校验 SHA-256，再在临时 SQLite 中执行迁移和 `integrity_check`。
3. 数据库可打开时还原前自动生成当前账套保护备份；数据库损坏时保留 `backup/restore-source-*` 隔离副本。保护账套还原成功后同步生成 `data/erp.db.enc`。
4. 目标数据库替换或重新打开失败时，系统尝试从内存快照恢复还原前的 active/encrypted 文件；恢复失败会返回明确错误，需保留 `backup` 目录后人工处理。
5. 退出流程先备份并写入干净退出状态；异常退出会在下次启动显示脏退出标记，便于检查。
- 云端备份回滚只删除对应 Storage 对象和 `erp_cloud_backups` 元数据，不触碰本地 SQLite；下载写回失败或 checksum 不匹配时保留当前本地账套和原有备份。
- 草稿守卫只覆盖当前已注册的实体编辑、状态流转、单据明细和回款分配；长任务登记已覆盖导出、备份、还原、附件和保护迁移。定时备份调度已实现并完成本机 release 首次/间隔检查，托盘菜单驻留、外部 Office/杀毒软件占用及磁盘/权限故障仍需现场验证。
