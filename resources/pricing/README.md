# 内置价格版本

`openai-2026-10-06.json` 是 2026-10-06 核查的全球文本 API 参考价，使用整数 micro-USD / 百万 tokens。费用保存为 nano-USD，最终展示时四舍五入。

来源：[OpenAI Pricing](https://developers.openai.com/api/docs/pricing)、[Prompt caching](https://developers.openai.com/api/docs/guides/prompt-caching)，以及 [GPT-6.1 Sol](https://developers.openai.com/api/docs/models/gpt-6.1-sol)、[GPT-6 Astra](https://developers.openai.com/api/docs/models/gpt-6-astra)、[GPT-6 Luna](https://developers.openai.com/api/docs/models/gpt-6-luna) 的长上下文规则。价格包含普通输入、缓存读取、缓存写入及输出；缓存读取和写入都是输入的子集。长上下文以单次请求输入超过 272,000 tokens 判断，对整个请求适用。

包含 Standard、Fast、Batch、Flex 及 Astra 的 Ultrafast。已知 `default` 映射为 Standard，`priority` 映射为 Fast；没有档位、`auto`、未知模型、未知缓存写入或无法确定单次请求大小时不计价。区域处理、FedRAMP、工具调用和税费尚不纳入；这是 API 等价参考费用，不是 Codex 订阅账单。

这里的生效起点采用核查日期，表示本地开始采用此版本的时间；未声称这是官方历史调价日期。历史 facts 的价格与金额保留不变。总览、模型和会话金额另外采用按核查日期的 API 等价估算：独立参考价格投影按最多 1024 条的小批次补齐历史用量，断点与结果事务提交，价格内容变化时重新计算。计算未完成显示“计算中”，缺少服务层级、模型价格或完整计数仍标记未覆盖；历史价格分类继续保留记录时依据。
