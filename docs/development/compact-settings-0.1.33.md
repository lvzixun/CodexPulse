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

## 发布验证

已发布 [v0.1.33 正式版](https://github.com/lvzixun/CodexPulse/releases/tag/v0.1.33)，并设为最新版本。源码提交及标签指向 `16a357800079b6f2dd2f50200a48a15fb920682f`。

- [双平台 CI](https://github.com/lvzixun/CodexPulse/actions/runs/39952977864) 全部成功：Windows 200 项、macOS 201 项 Rust 测试通过，两端各 7 项显式联网 / 实机探针按原设置忽略；Svelte、Node、Clippy、安装包构建及更新签名验证通过。
- Windows x64 应用和安装包版本检查通过；下载后签名、版本绑定及篡改拒绝再次通过，安装包 SHA-256 与 CI 输出一致。
- macOS Universal 下载包包含 arm64 / x86_64，两种架构的最低系统均为 12.0；版本 0.1.33、严格 ad-hoc 签名校验通过。只读挂载 DMG 确认安装入口存在，程序与自动更新归档内逐字节一致；更新签名、版本绑定及篡改拒绝通过。
- 7 个公开资产的名称、大小及 SHA-256 与本地全部一致。匿名读取最新正式版元数据确认 v0.1.33；匿名下载最新 `latest.json` 与本地逐字节相同，覆盖 Windows x64、macOS Intel / Apple Silicon 三个更新入口。
- 生产前端不包含截图用的虚构会话与项目数据；本次未替换本机已安装应用，也未进行 Windows / macOS 原生交互验收。
