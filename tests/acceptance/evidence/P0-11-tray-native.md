# P0-11 / P0-14 原生托盘验收记录

- 验收日期：2026-09-19
- 版本：V1.2.7
- 数据集：`artifacts/portable/plastic-sales-erp-1.2.7-windows-x64-portable` 的隔离空账套
- 交付包启动前提：`verify-portable.ps1 -LaunchCheck` 通过，`state/ui_ready.json` 返回 `1.2.7`，WebView2 进程路径位于包内 `runtime/webview2`。

## 结果

1. 通过 Win32 `WM_CLOSE` 触发标题栏关闭后，窗口隐藏，应用进程仍保持运行；这符合默认“最小化到托盘”行为。
2. Windows 系统托盘溢出区发现 `SystemTray.NormalButton`（名称“改性塑料销售 ERP”）。对该图标执行左键调用后，主窗口恢复可见，进程保持单实例。
3. 修复托盘回调后，右键点击图标显示原生菜单，菜单项为“打开工作台”和“退出并备份”；调用“打开工作台”后主窗口恢复可见。
4. 修复前右键也会触发恢复窗口，导致菜单无法使用；本轮将回调限制为左键释放，Rust 回归用例 `tray_activation_only_restores_on_left_button_release` 通过。

## 尚未闭合

- “退出并备份”菜单项在本轮已确认可见，但尚未在当前隔离包中完成菜单项执行后的 `clean` 状态和备份核验；继续验收时应补做并记录 `state/last_exit.json`、备份 manifest checksum、SQLite `integrity_check` 和进程数。
- 托盘驻留期间的定时备份、真实并发任务、Office/杀毒软件占用仍按 P0-11/P0-14 保持现场待验。
