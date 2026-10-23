# 0.1.33 模型推理级别与 Fast 状态

macOS 菜单栏与 Windows 小浮窗在当前模型后显示推理级别和 Fast 状态。例如 `6-astra xhigh · Fast`；明确为常规服务时显示 `Fast off`，缺少信息时分别显示 `—` / `Fast ?`。空闲、活动记录过期或来源失联时隐藏会话设置。

设置来自所属会话的 `turn_context.effort` / `reasoning_effort` 和 `thread_settings_applied.thread_settings.reasoning_effort` / `service_tier`。复制进来的其他线程设置不会改变本会话显示；模型切换清除旧设置。Fast 开启接受 `priority` / `fast`，关闭接受 `default` / `standard` / `flex` / `batch`，其余保持未知，不从速度或历史采样推断。设置变化通过后台本地活动快速更新传递，无须等待速度样本或下一次完整统计。

旧检查点使用原有 service tier，并最多读取提交偏移前 256 KiB 的日志尾部恢复推理级别；忽略不完整记录、其他线程设置，不跨模型变化寻找旧级别。找不到时保留未知，之后随新的日志上下文更新。该升级每个文件只执行一次，不回退用量偏移、不重算 Token。Windows 浮窗改为 300 × 36 逻辑像素，以容纳新增字段；中英文文档示意图同步更新。

## 本地验证

- `cargo test --workspace --locked`：201 项通过，7 项显式联网 / 实机探针按原设置忽略。
- Clippy 全目标零警告，Rust 格式检查与前端 Prettier 检查通过。
- Svelte 零错误 / 警告，22 项 Node 测试及生产前端构建通过。
- 中英文 IPC 模拟回归验证 high / xhigh / max / ultra、Fast 开关与未知、无速度样本、空闲清除、300px 布局，以及原有浮窗拖动、点击、菜单和消息流程。
- Rust 回归覆盖线程归属、模型切换、显式空值、检查点序列化、旧检查点恢复、不完整追加、用量不重复，以及只改变级别 / Fast 时的快速状态更新。

Windows 原生浮窗和 macOS 菜单栏的实际交互验收不由 IPC 模拟替代。
