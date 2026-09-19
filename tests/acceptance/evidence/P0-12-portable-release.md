# P0-12 Windows release 隔离现场记录
- 日期：2026-09-15
- 版本：V1.2.3 release
- 可执行文件 SHA-256：`D21B7019EBC6CD3567A02794AEB3C9DB5E2F3C884728838D30461A398F757722`
- 数据根目录：系统临时目录中的 `erp-field-20260915`（未使用工作区账套）
- 环境边界：本机 Windows；未模拟断网、干净系统、无 WebView2、磁盘耗尽、权限拒绝、杀毒软件占用或 U 盘拔出。

## 操作与结果

1. 将 release 可执行文件复制到隔离目录并启动。程序自动创建 `backup`、`config`、`data`、`export`、`files`、`logs`、`state` 七个目录，窗口正常进入工作台。
2. 对同一可执行文件再次启动。第二进程自动退出，原进程仍为唯一运行实例，窗口标题为“改性塑料颗粒销售 ERP”。
3. 首次进程强制结束前，`state/last_exit.json` 为 `status=dirty`。再次启动后界面显示“检测到异常退出”和“已完成启动完整性检查”，检查结果为“正常”，并在 `logs/erp-YYYYMMDD.log` 写入 `unclean_exit_recovered`。界面证据：`P0-12-portable-unclean-recovery.png`。
4. 关闭恢复提示后按 `Ctrl+Q`，在退出对话框选择“退出并备份”。进程数变为 0；`state/last_exit.json.status=clean`；生成 253,952 字节备份和 `backup/manifest.json`；备份实际 SHA-256 与清单一致；只读 SQLite `PRAGMA integrity_check` 返回 `ok`。退出对话框证据：`P0-12-portable-exit-dialog.png`。

## 可复核结果

```text
second launch: second process exited, original process count = 1
unclean recovery: previous status = dirty; recovery log event = unclean_exit_recovered
normal exit: process count = 0; last_exit.status = clean
backup: 253952 bytes; manifest checksum = actual checksum
sqlite: PRAGMA integrity_check = ok
```

辅助校验第一次把清单中的绝对路径再次拼接到根目录，产生了脚本路径语法错误；改用绝对路径判定后复核通过。该脚本错误未改变应用数据或退出结果。
