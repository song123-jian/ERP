# 改性塑料颗粒销售 ERP

面向改性塑料颗粒销售场景的单人、离线桌面 ERP（当前版本 V1.2.8）。应用使用 Vue 3 + Vite 构建界面，使用 Tauri 2 和 SQLite 保存本地账套。

## 已覆盖业务

- 客户、跟进记录、样品测试与报价
- 订单、送货、采购、库存流水与库存预占
- 对账、回款分配和提醒状态
- 本地保护模式、备份、还原与审计记录
- 异常退出恢复提示：启动完整性检查、恢复事件日志和最近备份入口
- 备份失败保护：快照、写入和登记失败均有明确错误与重试入口，不留下半成品并保留可核验的已生成备份
- 定时自动备份：首次打开和托盘驻留期间按配置间隔检查最近校验通过的备份，达到间隔后自动生成一致性快照
- 损坏主库恢复：数据库未打开时仍可读取备份清单，恢复前校验 checksum、隔离原文件，保护账套同步更新加密 shadow
- 后台任务状态显示、退出前等待，以及可配置的标题栏关闭和托盘退出行为
- 业务对象附件：便携 `files/` 目录、SHA-256 校验、重复内容去重、篡改检测和保护模式加密
- CSV 模板、预检、行级回滚和失败行报告；保护模式下业务导出与失败行报告使用加密 CSV
- 可选 Supabase 自动同步：业务事务写入本地 SQLite 时同步登记可靠事件队列，未登录或断网不影响录入；合法会话恢复后按工作区 RLS 幂等上传

## 本地运行

需要 Node.js、pnpm、Rust/Cargo 和 Windows WebView2。

```powershell
pnpm install
pnpm dev
```

常用验证命令：

```powershell
pnpm test
pnpm build
Set-Location src-tauri
cargo test
cargo check
```

构建 Windows 可执行文件：

```powershell
pnpm tauri build
```

生成文件位于 `src-tauri/target/release/plastic-sales-erp.exe`。当前 `src-tauri/tauri.conf.json` 中 `bundle.active` 为 `false`，因此不会生成安装包。

## Windows 绿色便携发布

便携包固定使用官方 Microsoft Edge WebView2 Fixed Version Runtime `153.0.4234.32`（x64）。获取脚本会校验 CAB 的固定 SHA-256 和 Microsoft 签名；打包脚本要求 release 可执行文件、三处应用版本及运行时版本一致。

```powershell
pnpm run runtime:webview2
pnpm tauri build
pnpm run package:portable
pnpm run verify:portable
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\verify-portable.ps1 -LaunchCheck
```

产物位于 `artifacts/portable/plastic-sales-erp-<版本>-windows-x64-portable/` 及同名 ZIP。目录包含应用、`runtime/webview2/`、`data`、`backup`、`export`、`files`、`logs`、`state`、`config`、依赖许可证清单和逐文件 SHA-256 清单。`-LaunchCheck` 会在临时账套启动应用，等待 `state/ui_ready.json`，并确认实际 `msedgewebview2.exe` 来自随包运行时；不会把测试账套写入交付目录。

## 数据与边界

业务数据保存在本机 SQLite 账套中，附件保存在账套根目录的 `files/` 下，数据库只登记相对路径和 SHA-256。单个附件上限 25 MB；启用保护模式时历史明文附件会先校验并迁移为 AES-GCM 密文，下载前会再次校验哈希，检测到缺失或篡改后可通过重新上传同一内容修复原登记。业务 CSV 导出先在内存生成；未启用保护时输出 `.csv`，保护模式输出带 `ERPENC1` 封装的 `.csv.enc`，导入失败行报告也遵循同一策略。保护模式的写入使用临时文件和原子替换，不留下明文临时文件。启用保护前已经生成的旧版明文导出仍由用户自行保管或清理。运行数据、备份、导出文件、日志和本地配置均已由 `.gitignore` 排除。浏览器演示模式只保存附件索引，不写入本地文件，也不提供下载。当前版本提供可选 Supabase 云端备份和追加式业务事件同步；本地 SQLite 始终是事实来源，自动同步不开启匿名写入，也不使用 service-role key。未登录、未配置或断网时事件留在本地队列，合法会话与工作区可用后按幂等键上传。需要本地填写 publishable key，初始化步骤见 [`supabase/README.md`](supabase/README.md)。多设备业务记录下载、冲突合并和在线主库仍未启用。

详细实现与验收记录见 [docs/HANDOFF.md](docs/HANDOFF.md)；需求追溯见 [docs/REQUIREMENTS_TRACEABILITY.md](docs/REQUIREMENTS_TRACEABILITY.md)；P0 现场验收范围见 [tests/acceptance/P0.md](tests/acceptance/P0.md)。
