# README 截图

截图来自实际 Svelte 界面的开发预览，使用 `apps/desktop/src/lib/readme-preview.ts` 中的虚构账户、会话和公告，不读取本机登录信息或对话日志。预览入口受 `import.meta.env.DEV && !native` 限制，示例数据不进入生产构建。

| 内容                       | 逻辑尺寸  | PNG 像素尺寸 |
| -------------------------- | --------- | ------------ |
| 总览、Sessions、Tibo | 380 × 800 | 1140 × 2400  |
| 模型（展开运行曲线） | 380 × 980 | 1140 × 2940 |
| Windows 小浮窗             | 184 × 36  | 552 × 108    |

中英文各一套，均为浏览器以 3 倍设备像素比直接渲染的无损 PNG，不是将旧截图放大。详情使用菜单栏布局；外层仅模拟原生窗口的圆角裁切，不模拟桌面背景或原生毛玻璃。原生平台字体与材质可能不同。

README 按逻辑宽度显示，并链接到原图；保留完整分辨率，不再将 JPEG 包装在 SVG 中。

## 重新生成

需要 Node.js、Playwright 和可用的 Chromium 浏览器。不要为截图构建 Rust。

先启动仅前端的开发服务器：

```sh
pnpm --filter @codexpulse/desktop dev --port 1430
```

另一个终端运行：

```sh
node scripts/capture-readme.cjs
# 仅更新模型截图
CODEXPULSE_CAPTURE_PAGE=models node scripts/capture-readme.cjs
```

默认连接 `http://127.0.0.1:1430/`，可通过 `CODEXPULSE_PREVIEW_URL` 修改本地地址。脚本拒绝非本地服务器，阻止页面请求外部服务；使用 Asia/Shanghai 时区、固定视口与 3 倍设备像素比，检查页面运行错误。截图日期和相对时间取生成时的当前时间。

Playwright 可以安装在独立的工具目录，用 `CODEXPULSE_PLAYWRIGHT_MODULE` 指向其模块路径，无需修改产品依赖。若使用系统 Edge，将 `CODEXPULSE_BROWSER_CHANNEL` 设为 `msedge`；未设置时使用 Playwright 的 Chromium。

修改示例数据后执行 `pnpm check` 与 `pnpm build:ui`，确认生产产物不包含示例数据；人工检查中英文截图的文字、状态与布局后再提交。

## 交互验证

`node scripts/verify-appearance.cjs` 覆盖中英文、Windows / macOS、深浅 / 跟随系统主题和五种强调色的 60 种组合，验证选中标签可区分及重置行同行显示。`verify-panel.cjs` 也覆盖浮窗整条拖动、标题栏留白、操作按钮排除、点击 / 拖动阈值及拖动后点击恢复。拖动使用 IPC 模拟，仍需 OS 手感实测。

同一开发服务器及 Playwright 环境下，可运行 `node scripts/verify-panel.cjs`。它使用 Tauri 官方 IPC 模拟，验证中英文更新提示、手动检查、下载动作、提示关闭及缓存复用，Tibo 自动已读的页面 / 可见性限制、新消息、并发及失败处理，以及浮窗菜单的三项顺序、动作与复用，不连接真实账户、不退出真实应用。该检查不代替 Windows 原生菜单绘制验收。
