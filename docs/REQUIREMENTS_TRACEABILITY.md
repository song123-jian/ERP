# V1.2 需求追溯矩阵

最后更新：2026-09-19

基线来源：`ERP_v1.2_基线补充版.docx` 第 3、4、5、8、9 章及附录 C/D；本轮附件依据 `工程塑料购销合同-TZJP202609-18-南京聚隆-台州骏普.pdf`、`零跑 PP-GF20 报价单0615.pdf`。
状态定义：`已自动验证` 只代表本地代码、构建或自动化用例通过；`部分现场验证` 表示发布版 Windows 仅覆盖了该项的部分场景；`待现场验证` 必须在发布版 Windows 环境取得证据；`部分实现` 表示已有实现但基线验收尚未闭合。

| 基线要求 | 代码与配置 | 自动验证证据 | 现场证据状态 | 结论 |
| --- | --- | --- | --- | --- |
| 3.1–3.2 单人、离线、分层与事务写库 | `src/api.ts`、`src-tauri/src/main.rs` 的 Tauri 命令和 SQLite 事务 | Rust 业务、导入、库存和回款用例 | 浏览器演示已覆盖核心路径；真实桌面待验 | 已自动验证 |
| 3.4 便携目录与相对路径 | `ensure_dirs`、`current_exe` 根目录解析、`data`/`backup`/`export`/`files`/`logs`/`state`/`config` 目录 | Rust 临时目录用例 | 本机 V1.2.4 release 已在隔离目录验证七个目录自动创建；U 盘换盘符、换机运行待验 | 已自动验证并完成本机 release 现场验证，目标设备场景待验 |
| 3.5、3.8 本地保护与恢复密钥 | Argon2id、AES-256-GCM、`protection.json`、加密库/备份/附件与 CSV 导出策略；`write_csv_payload` 统一保护导出 | 加密往返、保护备份、附件加密下载、`.csv.enc` 导出与失败行报告用例 | V1.2.4 Windows release 隔离临时账套已验证启用保护、加密备份、错误口令拒绝、正确本地口令/恢复密钥解锁和 clean 退出；导出文件现场保管、多设备和 U 盘迁移仍待验 | 已自动验证并完成本机 release 现场验证，目标设备场景待验 |
| 3.6 绿色分发与 WebView2 | `src-tauri/src/main.rs` 在创建 WebView 前选择 `runtime/webview2`；`scripts/fetch-webview2-runtime.ps1`、`package-portable.ps1`、`verify-portable.ps1`；README | V1.2.5 release 构建；固定 CAB SHA-256 与 Microsoft 签名；绿色目录 261 个静态文件逐项校验；ZIP 内容校验；临时账套 UI 就绪且 WebView2 进程路径来自随包运行时 | 已生成含 WebView2 `153.0.4234.32`、七个运行目录、版本/依赖/许可证/哈希清单的绿色目录与 313,654,024 字节 ZIP；目标 Windows 10/11 的断网、系统无 WebView2、无开发环境机器仍待现场 | 已实现并自动验证，目标环境待验 |
| 3.7 性能与可靠性指标 | `scripts/measure-release.ps1`、`state/ui_ready.json`、`list_entities_page`；分页响应最多 100 行 | `customers_ten_thousand_list_p95_stays_under_three_hundred_ms`：release 优化构建 10,000 条客户、20 次样本 P95；`pnpm run perf:release`：隔离 release 启动就绪与工作集采样 | 本机 V1.2.4 release 实测启动至前端就绪 777.055ms、峰值工作集 28.32MB；万级列表 P95 15.655ms；目标设备、U 盘写入慢和大附件场景仍待验 | 已自动验证并完成本机 release 指标采样，目标环境场景待验 |
| 7.8 表格分页与列表响应边界 | `list_entities_page`、`EntityPage.vue` 的 `page/pageSize/itemCount` 契约；导出、工作台和下拉选项继续使用全量接口 | Rust `paged_entity_results_preserve_filters_and_bound_response_size` 验证筛选、总数、页边界和 100 行上限；`pnpm exec vue-tsc --noEmit` 通过 | 浏览器演示和 release 构建均使用分页响应；真实万级文件库查询仍以性能门禁和目标设备采样为准 | 已自动验证 |
| 4.1 单实例与再次打开唤起 | `tauri_plugin_single_instance`、窗口 `show`/`unminimize`/`set_focus` | Rust 编译检查 | 本机 V1.2.4 release 同一可执行文件第二次启动后第二进程退出，原进程保持唯一；托盘唤起仍待验 | 已自动验证并完成单实例本机现场验证，托盘场景待验 |
| 4.2–4.4 启动时序、dirty 状态与异常恢复 | `mark_startup_in_progress`、`read_previous_exit`、`initialize`、`append_runtime_log`、`App.vue` 恢复提示与损坏库备份选择 | `startup_recovery_marks_existing_data_and_preserves_clean_state_for_bad_passwords`；SQLite `integrity_check`；损坏库恢复、独立清单和保护 shadow 回归用例；本机 release 强杀重启、恢复日志和损坏库还原核验 | 本机隔离便携目录已验证 `dirty`、恢复对话框、`unclean_exit_recovered` 日志、损坏库错误页、清单校验、`restore-source-*` 隔离及还原后 `integrity_check=ok`；真实权限、文件占用和 U 盘场景待验 | 已自动验证并完成本机异常退出及普通损坏库还原现场验证，保护账套/目标设备场景待验 |
| 4.5 本地身份策略 | `resolve_startup_master`、解锁页与恢复密钥 | 错误密钥拒绝、加密解密用例 | 本机 release 已验证错误口令拒绝并保持锁定，正确本地口令和恢复密钥重启后均进入工作台；恢复密钥保管和多设备迁移待验 | 已自动验证并完成本机 release 现场验证，目标设备场景待验 |
| 4.6 路径占用与 U 盘异常 | 根目录相对解析、SQLite 5 秒 busy timeout；文件替换统一按占用/权限分类 | `file_operation_errors_explain_lock_and_missing_file_recovery`、`locked_export_and_attachment_files_preserve_target_and_cleanup_temps` | 本机 release 已用独占句柄锁定 `backup/manifest.json`，验证登记失败提示、已生成文件保留和释放后重试；杀毒扫描、拔盘和真实介质故障待验 | 已自动验证并完成文件占用本机现场验证，U 盘/目标设备待验 |
| 5.2–5.3 正常退出、备份、WAL checkpoint 与 clean 标记 | `shutdown`、`backup_database`、`backup_database_to_paths`、`write_exit_state`；Windows `ReplaceFileW` 替换状态/配置/加密库文件 | 备份快照、保护备份、失败清理、登记失败保留路径、启动状态原子替换回归用例；本机 release `Ctrl+Q` 后核验进程为 0、`clean` 状态、备份完整性和 schema 记录 | 本机隔离便携目录已核验进程数为 0、`clean` 状态、253,952 字节备份、manifest checksum 一致和 SQLite `integrity_check=ok`；U 盘安全弹出、真实磁盘空间/权限故障待验 | 已自动验证并完成正常退出本机现场验证；失败闭环已补充 |
| 3.5 定时自动备份与备份保留 | `backup_schedule_enabled`、`backup_schedule_days` 设置；`src/domain.ts` 的过期判断；`App.vue` 启动/托盘定时检查复用 `backup_now`；schema v6 默认值与设置校验 | `shouldRunScheduledBackup` 覆盖停用、无备份、近期备份、过期备份和无 checksum；Rust schema v6 幂等迁移、设置边界和任务 RAII；V1.2.5 release 隔离账套首次触发、60 秒后抑制重复、manifest checksum 及任务释放 | UI 就绪后 24.605 ms 生成 253,952 字节、schema 6 的首份备份；126 秒后仍为一份，任务列表为空且无临时文件。托盘菜单、磁盘空间不足、目录权限和外部文件占用仍待现场 | 已实现并完成本机 release 部分现场验证 |
| 5.2 未保存表单拦截 | `src/drafts.ts`、`src/App.vue`、`src/components/EntityPage.vue`、`src/components/DocumentItemsModal.vue`；模块切换使用 `:key="activeKey"` 隔离页面实例 | `tests/domain.test.ts` 验证保存失败保留脏项、显式丢弃清理注册项；`pnpm exec vue-tsc --noEmit`、`pnpm build`、`pnpm tauri build`；本机 release 已现场验证客户表单取消守卫、继续使用、保存并退出和备注落库 | Naive UI 客户表单三分支及 `Ctrl+Q` 已现场验证；托盘菜单退出、保存失败和并发任务仍待现场验 | 已自动验证，部分现场验证 |
| 5.2 进行中导出/备份、后台任务与外部文件占用 | `src/tasks.ts`、`src/api.ts`、Rust `TaskRegistry` 和 `list_background_tasks`；长任务以 RAII 释放登记，退出前等待前端与原生任务，30 秒超时保留窗口并提示；原子替换统一返回占用/权限重试提示 | `tests/domain.test.ts` 任务等待/异常释放；Rust `task_registry_releases_raii_leases_and_keeps_snapshot_order`、`file_operation_errors_explain_lock_and_missing_file_recovery`、`locked_export_and_attachment_files_preserve_target_and_cleanup_temps`；`pnpm test`、`cargo test` | 本机 release 已现场验证备份清单被独占时登记失败提示、已生成文件保留、释放后重试成功和无临时残留；并发导出/备份、附件读写和 Office/杀毒软件占用仍待验 | 已自动验证并完成文件占用本机现场验证，真实并发/外部占用待验 |
| 5.4 托盘最小化、关闭行为配置与 Ctrl+Q | `close_behavior` 设置持久化；标题栏 × 按设置最小化到托盘或打开确认；托盘退出和 `Ctrl+Q` 统一执行草稿、任务、备份和 clean 状态流程；`src-tauri/capabilities/main-window.json` 提供最小原生权限 | Rust schema/值校验用例；前端类型检查、生产构建；Playwright 验证设置刷新持久化、`Ctrl+Q` 退出确认和任务标签；本机 release 验证标题栏隐藏、单实例唤回及退出后进程为 0 | 托盘菜单唤起/退出待验 | 已自动验证，部分现场验证 |
| 附录 C/D 业务状态、颜色和进入条件 | `src/domain.ts`、`src/modules.ts`、Rust `allowed_transition`/保存校验 | 前端领域测试与 Rust 状态机用例 | 核心业务演示已检查；真实业务数据待验 | 已自动验证 |
| 8.4、附录 P0 导入、还原、附件和退出验收 | `tests/acceptance/P0.md`、CSV/备份/附件/恢复命令、草稿守卫、后台任务登记、分页和性能门禁 | Rust 55 项常规用例及 1 项 release 性能门禁；前端 11 项领域测试、类型检查、生产构建和 release 构建；P0-02、P0-03、P0-04、P0-05、P0-08、P0-12、P0-14、P0-15 本机 release 验证；P0-11 文件占用自动化与局部现场验证；P0-16 浏览器合同/报价模板验收 | P0-01、P0-04、P0-05、P0-09 至 P0-11、P0-14 的断网、真实磁盘/权限故障、目标设备、托盘、并发、原生打印和其他未覆盖真实桌面场景待验 | 已自动验证并完成本机 release 与浏览器模板部分验收，仍需目标 Windows 现场验收 |
| 8.5、9 指标、风险和异地容灾 | 备份记录、审计日志、帮助页、`scripts/measure-release.ps1` 与性能证据 JSON | 备份/还原/日志代码用例；万级列表 P95 与 release 启动/内存门禁通过 | 本机性能指标已建立并采样；每月恢复演练、非 U 盘副本和真实数据分叉提示仍待运营现场建立 | 部分实现 |
| 8.5 运营评估与恢复演练 | `src/components/OperationalAssessmentPanel.vue`、`src/App.vue` 基础设置页签、`src/api.ts` 浏览器演示命令、`src-tauri/src/main.rs` 的 `get_operational_metrics`/`run_restore_drill` | `tests/api.test.ts` 覆盖无备份数据不足、备份成功率、恢复演练成功率和结果回读；`pnpm exec vue-tsc --noEmit`、`pnpm test` 12 项、`pnpm build`、`cargo test` 60 项通过；本地浏览器页签与按钮检查、控制台错误检查 | 浏览器演示已显示六项指标、数据不足状态和演练通过状态；Tauri 真实账套、按月演练制度和目标 Windows 现场仍待验证 | 已实现并自动验证，部分现场验证 |
| Supabase 可选云端备份 | `src/supabase.ts`、`src/components/CloudBackupPanel.vue`、`src-tauri/src/main.rs`、`supabase/migrations/20260915_erp_cloud_backup.sql`、`.env.example` | `pnpm test` 11 项、`pnpm build`、`cargo test` 55 项、`cargo check`、`cargo fmt --check`、`pnpm tauri build`；目标项目 SQL Editor 正式执行迁移并完成只读核验 | 目标项目已确认 3 张表均为 `true/true` 强制 RLS、2 个 `SECURITY DEFINER` 函数、6 条业务策略、3 条 Storage 策略和私有 `erp-backups` bucket；`erp_workspaces`、`erp_workspace_members`、`erp_cloud_backups` 均为 0 行；保护模式上传对象保持加密；真实账号登录、网络中断、上传/下载和云端还原仍待现场 | 已自动验证，部分现场验证 |
| 附件合同与报价单模板 | `src/modules.ts`、`src/components/DocumentItemsModal.vue`、`src/components/EntityPage.vue`、`src/print.ts`、`src/components/PrintPreviewModal.vue`、schema v7 合同/报价快照字段、`src/api.ts` 演示数据 | `contract_header_items_and_status_flow_preserve_print_snapshots`、`quotation_header_and_items_preserve_fax_snapshot_fields`；`pnpm test` 11 项、`cargo test` 55 项、`pnpm exec vue-tsc --noEmit`、`pnpm build`、`pnpm tauri build`、`cargo fmt --check`；新增浏览器演示合同明细保存后金额重算回归；Playwright P0-16 浏览器检查 | 合同 `TZJP202609-18` 和报价 `零跑PP-GF20-0615` 的关键附件字段、十项条款、明细金额、双方账号、传真抬头、勾选项和页脚均在 A4 预览可见；合同明细按数量×含税单价自动联动小写 `¥2,360.00` / 大写 `贰仟叁佰陆拾元整`，改价和不保存恢复已核验，主编辑窗可回填两种金额；`contractTotalCents` 统一明细弹窗和打印模板汇总，保存后重新读取仍为 `236000` 分；两份容器均为 `794×1123`，当前视口无横向溢出；真实 Windows 打印机/PDF 驱动和签章仍待现场 | 已实现并完成浏览器模板验收，原生打印场景待验 |

## 绿色便携发布本轮补充

- `scripts/fetch-webview2-runtime.ps1` 固定下载官方 x64 WebView2 Fixed Version Runtime `153.0.4234.32`，CAB SHA-256 为 `2CB653A74426F0AA802C2396775C6BC674FD662D5396BD677F47BFA6E12EBA9C`，解包后复核版本和 Microsoft Authenticode 签名。
- 应用在创建 Tauri WebView 前检查可执行文件同级的 `runtime/webview2/msedgewebview2.exe`，存在时设置 `WEBVIEW2_BROWSER_EXECUTABLE_FOLDER`；开发目录未提供固定运行时时继续使用系统 WebView2。
- V1.2.5 绿色目录包含应用、固定运行时、七个空运行目录、依赖/许可证清单和逐文件 SHA-256；统一前端版本来源后重建的 ZIP SHA-256 为 `02F62120BABAB5ED63F0F1D4076C2FD57F6B1F34008C0CFBCFD5EA4FAAA1CB1C`，发布清单 SHA-256 为 `4A92FDD42F5EBF87F1387A72DE3F285F6A5DA0DA20B0DA6AFA5BE8B8444DDBE4`。
- `verify-portable.ps1 -LaunchCheck` 使用临时账套启动交付可执行文件，UI 就绪版本为 `1.2.5`，实际观察到的 WebView2 进程路径为绿色目录内 `runtime/webview2/msedgewebview2.exe`；本轮没有禁用系统 WebView2 或执行断网抓包，因此 P0-01 的目标设备部分保持待验。
- V1.2.7 合同/报价变更后重新执行 `pnpm tauri build`、绿色包生成和 `verify-portable.ps1 -LaunchCheck`；临时包的 261 个静态文件共 711,737,656 字节，UI 就绪版本为 `1.2.7`，ZIP 归档校验通过，观察到的 WebView2 进程路径均位于临时包 `runtime/webview2`。

## 启动恢复本轮补充

- 读取状态文件时，已有账套的状态文件缺失或无法解析会被识别为异常退出；首次创建账套不会误报。
- 保护账套先验证本地口令或恢复密钥，验证失败时不改写此前的 `clean` 状态，避免错误口令造成虚假异常提示。
- 验证成功后才写入 `dirty`；启动完成后会执行 SQLite 完整性检查，并在检测到异常退出时将恢复事件写入 `logs/erp-YYYYMMDD.log`。
- 完整性通过但上次异常退出时，界面恢复对话框显示上次状态时间、检查结果和日志目录，并提供“查看备份”入口；完整性失败时，错误页提供“查看备份并恢复”入口。系统不会自动选择或覆盖任何账套。

## 未保存表单本轮补充

- `src/drafts.ts` 提供统一草稿注册表；实体编辑、状态流转、单据明细和回款分配在打开后注册，保存成功或明确丢弃后注销。
- 页面导航、原生窗口关闭、托盘退出和 `Ctrl+Q` 会先检查脏草稿；保存失败、校验失败或丢弃异常都会保留当前现场并阻止后续动作。
- 明细保存由全局退出守卫调用时不触发父列表异步刷新，避免退出备份与刷新查询并行；显式保存仍刷新父列表。
- `EntityPage` 按模块键重建，避免干净但仍打开的旧编辑器或明细状态跨模块复用。
- 本机 Windows release 现场验证客户表单：取消时显示未保存提示；“继续使用”保留名称和备注；再次 `Ctrl+Q` 选择“保存并退出”后进程退出、退出状态为 `clean`，退出备份完整性为 `ok`，客户备注已在备份数据库中核验。

## 后台任务与托盘行为本轮补充

- `src/tasks.ts` 为前端异步命令登记任务标签、类型和开始时间；`runTrackedTask` 在成功或异常路径都释放登记，退出守卫会等待登记清空。
- Rust `TaskRegistry` 为启动、备份、导入导出、还原、附件读写和保护迁移命令建立 RAII 租约，并通过 `list_background_tasks` 提供当前任务快照；退出等待超时不会强杀任务。
- `close_behavior` 默认值为 `minimize_to_tray`，可在基础设置切换为 `confirm_exit`。标题栏 × 遵循该设置，托盘退出和 `Ctrl+Q` 始终进入真正退出流程。
- 本轮自动与本机 release 证据覆盖任务登记、异常释放、关闭行为值校验、标题栏隐藏、单实例唤回、强杀恢复、`Ctrl+Q` 正常退出和 Windows 独占文件错误闭环；托盘菜单、真实并发、Office/杀毒软件占用和脏草稿交互仍须按 P0-10、P0-11 现场验证。

## 原生发布版现场验收补充

- 在本机 Windows 的隔离账套运行 V1.2.3 release：点击标题栏 × 后原进程保持运行，再次启动同一可执行文件后仍只有一个进程，主窗口成功恢复。
- 进程被强制结束前 `last_exit.json` 为 `dirty`；再次启动后界面显示“检测到异常退出”和“已完成启动完整性检查”，检查结果为“正常”，恢复日志写入 `unclean_exit_recovered`。截图见 `tests/acceptance/evidence/P0-02-unclean-exit-recovery.jpg`。
- 选择继续使用并通过 `Ctrl+Q` 执行“退出并备份”后，进程数为 0，状态恢复为 `clean`；新备份为 253,952 字节，SQLite `integrity_check` 返回 `ok`，`backup_records` 记录的 schema 版本为 5。
- 在客户 `P0-10 保存客户B` 编辑表单中修改跟进备注后，`Ctrl+Q` 退出守卫显示 1 个未保存录入和三种处理按钮；选择继续使用后内容保留，随后选择保存并退出，进程数为 0，`last_exit.json` 为 `clean`，备份大小为 253,952 字节、`integrity_check=ok`，备份数据库中的备注为 `P0-10 保存退出验证`。证据见 `tests/acceptance/evidence/P0-10-dirty-draft-guard.jpg`。

## 保护模式现场验收补充

- 在 V1.2.3 Windows release 的隔离临时账套中启用保护口令，退出后 `data/erp.db` 被移除并保留 `data/erp.db.enc`；备份目录新增加密 `.db.enc` 文件，文件头为 `ERPENC1`。
- 重启先显示“账套已锁定”。输入错误口令后显示“口令或恢复密钥不正确，无法解密数据”，页面仍锁定；输入正确本地口令后进入工作台。再次正常退出并重启，输入恢复密钥后同样进入工作台。
- 最终退出核验进程数为 0、`state/last_exit.json.status` 为 `clean`，数据目录无明文主库、WAL 或 SHM；主库和最新备份解密到临时核验副本后 `PRAGMA integrity_check` 均为 `ok`，核验副本已清理。现场 `logs/` 未发现口令或恢复密钥。
- 证据：`tests/acceptance/evidence/P0-08-protection-lock.jpg`、`P0-08-wrong-password.jpg`、`P0-08-unlocked-workbench.jpg`、`P0-08-recovery-unlocked-workbench.jpg`。

## 保护模式敏感导出补充

- `export_entity` 先在内存生成 UTF-8 CSV；未启用保护时写入 `.csv`，保护模式使用当前账套主密钥加密并写入 `.csv.enc`，文件头为 `ERPENC1`。
- 加密导出和导入失败行报告均通过临时文件、`sync_all` 和原子替换写入；加密路径不会在磁盘留下明文 CSV 临时文件。命令结果返回 `encrypted` 或 `failed_file_encrypted`，前端会明确提示保护模式已加密。
- Rust 定向用例 `csv_export_is_atomic_and_protected_exports_are_encrypted`、`protected_failed_import_report_is_encrypted` 与 `protected_export_requires_an_unlocked_master` 验证普通导出可读、保护导出可解密且内容一致、未解锁时拒绝导出、加密失败行报告可解密，并检查导出目录无 `.tmp` 残留。启用保护前已经生成的旧版明文导出不会自动迁移，仍需按现场保管流程处理。

## 备份失败闭环补充

- `backup_database` 为每次尝试生成独立快照名和目标名，不再先删除同名备份；SQLite 快照先读取到内存，最终文件通过 `sync_all` 和原子替换写入。
- 快照创建、读取、临时快照清理、加密和目标写入任一环节失败时，命令返回明确的磁盘空间、目录权限或文件占用提示，并清理本次临时文件；不会把半成品当作成功备份。
- 备份文件写入成功但 `backup_records` 登记失败时，命令明确返回“文件已生成但登记失败”及完整路径，保留可校验文件供备份列表核验和人工风险处置，不伪报成功。
- 设置页在备份失败后显示原因和“重试备份”按钮；用户可先关闭占用程序、检查空间与权限，再重试或按现场流程复制整个账套目录到其他介质。
- Rust 用例 `backup_write_failure_cleans_snapshot_and_keeps_blocking_target_untouched` 与 `backup_metadata_failure_preserves_verified_file_and_reports_retry_path` 覆盖目标写入失败、临时文件清理、阻塞目标不被覆盖和登记失败保留路径；真实磁盘空间、权限、杀毒软件占用仍须在目标 Windows 设备现场演练。

## 损坏主库恢复本轮补充

- 启动完整性检查失败时保留已经验证的保护主密钥，并把数据库保持在未打开状态；前端错误页提供“查看备份并恢复”入口，不把损坏库误报为错误口令。
- 每次成功备份额外写入 `backup/manifest.json`，记录路径、大小、schema 版本、创建时间和 SHA-256。数据库未打开时，`list_backups` 仍可扫描清单和 `erp-*.db`/`erp-*.db.enc` 文件；清单或数据库记录的 checksum 不匹配时还原会在替换前拒绝。
- 数据库不可打开时，还原前会将 `data/erp.db`、WAL/SHM 和 `data/erp.db.enc` 复制为 `backup/restore-source-*` 隔离副本；数据库可打开时仍先生成当前账套的保护备份。目标备份先写入临时库，执行迁移、`integrity_check` 和 WAL checkpoint 后才替换当前库。
- 保护账套还原成功后使用当前主密钥重新生成 `data/erp.db.enc`；任何替换或重新打开失败都会尝试恢复还原前的 active/encrypted 文件，并清理还原临时文件。
- Rust 用例 `damaged_uninitialized_database_restores_from_manifest_and_keeps_quarantine`、`protected_restore_rewrites_encrypted_database_shadow` 和 `manifest_checksum_mismatch_is_rejected_before_restore` 覆盖上述路径；真实 Windows 权限、文件占用、损坏文件和 U 盘拔出仍需现场演练。

## Supabase 云端备份本轮补充

- 迁移脚本只创建云备份层：`erp_workspaces`、`erp_workspace_members`、`erp_cloud_backups`、`private.is_erp_workspace_member`、`public.create_erp_workspace`、业务与 Storage RLS 策略以及私有 `erp-backups` bucket；不镜像客户、订单、库存等本地业务表。
- 只读核验结果：三张云备份表存在且 `relrowsecurity=true`、`relforcerowsecurity=true`；两个函数均为 `SECURITY DEFINER` 且 `search_path` 为空；工作区、成员、备份计数均为 0；bucket `public=false`。
- 前端上传前读取本地已经过 checksum 校验的备份；下载先在浏览器重新计算 SHA-256，Rust 再校验文件名、大小、保护模式边界并原子写入本地 `backup/`。元数据插入失败时会清理 Storage 对象。
- 真实登录、邮箱确认、网络中断、Storage 上传/下载、删除失败回滚和跨设备恢复尚未取得现场证据，保持“待现场验证”。未配置 `.env.local` 时浏览器演示和本地 SQLite 功能不受影响。

## 合同与报价附件模板本轮补充

- 合同数据按附件建立独立 `contracts`/`contract_items` 快照：合同号 `TZJP202609-18`、PP、`PI0-S27A[BK16452]`、200 KG、11.8 元/KG、2,360 元、款到发货、25KG/袋、十项条款、双方地址和账号均可编辑并参与打印模板；明细金额按数量×含税单价实时汇总，小写和中文大写自动填充，放弃未保存修改后恢复已保存值。
- 报价数据按附件建立 `quotations`/`quotation_items` 快照：PP-GF20、`PG4-S01A`、10.6 元/KG、未税含运费，以及传真抬头、页数、请审阅/请批注、调价说明和页脚联系方式均可编辑并参与打印模板。
- `src/print.ts` 对合同金额生成中文大写、对字段做 HTML 转义，并将合同与报价单渲染为 A4 预览；`PrintPreviewModal.vue` 提供打印/另存为 PDF 入口。
- Playwright 验收记录在 `tests/acceptance/evidence/P0-16-contract-quotation.md`：合同金额联动验证为 `¥2,360.00` / `贰仟叁佰陆拾元整`，两份预览均为 `794×1123`，关键内容可见，报价编辑表单的两个复选框各只有一个可访问标签。
- 当前只验证浏览器演示和 HTML 预览，不把该证据外推为真实打印机、PDF 驱动或签章流程已通过。

## 运营评估与恢复演练本轮补充

- “基础设置”新增“运营评估”页签，统一展示跟进超期率、备份成功率、还原成功率、最近备份年龄（RPO）、最近恢复耗时（RTO）和数据完整率；没有样本时显示“数据不足”，不以零值冒充通过。
- 恢复演练只允许选择存在、校验通过且有 checksum 的备份；Rust 端在临时 SQLite 副本中执行校验、迁移和完整性检查，完成后清理临时文件，不替换当前主库。浏览器演示模式保留同样的命令形状，并明确标记 `demo_only`。
- `tests/api.test.ts` 覆盖备份前的数据不足、`backup_now`、`get_operational_metrics` 和 `run_restore_drill` 的顺序回归；`tests/acceptance/evidence/P0-17-operational-assessment.md` 记录本地浏览器检查和本轮验证边界。
- 本轮未向 Supabase SQL Editor 写入业务数据；现有云端迁移与只读核验结果继续有效。按月恢复演练、真实保护账套和目标 Windows 现场仍需运营验收。

## 现场验收边界

本矩阵不能替代真实设备验收。使用发布版前，仍需按 `tests/acceptance/P0.md` 在目标 Windows 10/11、断网、U 盘、文件占用和敏感导出条件下保留一次可追溯证据；本次 P0-08 证据仅覆盖本机隔离临时账套。
