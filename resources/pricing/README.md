# 内置价格版本

`openai-2026-10-06.json` 是 2026-10-06 核查的全球文本 API 参考价，使用整数 micro-USD / 百万 tokens。费用保存为 nano-USD，最终展示时四舍五入。

来源：[OpenAI Pricing](https://developers.openai.com/api/docs/pricing)、[Prompt caching](https://developers.openai.com/api/docs/guides/prompt-caching)，以及 [GPT-6.1 Sol](https://developers.openai.com/api/docs/models/gpt-6.1-sol)、[GPT-6 Astra](https://developers.openai.com/api/docs/models/gpt-6-astra)、[GPT-6 Luna](https://developers.openai.com/api/docs/models/gpt-6-luna) 的长上下文规则。价格包含普通输入、缓存读取、缓存写入及输出；缓存读取和写入都是输入的子集。长上下文以单次请求输入超过 272,000 tokens 判断，对整个请求适用。

包含 Standard、Fast、Batch、Flex 及 Astra 的 Ultrafast。已知 `default` 映射为 Standard，`priority` 映射为 Fast；没有档位、`auto`、未知模型、未知缓存写入或无法确定单次请求大小时不计价。区域处理、FedRAMP、工具调用和税费尚不纳入；这是 API 等价参考费用，不是 Codex 订阅账单。

这里的生效起点采用核查日期，表示本地开始采用此版本的时间；未声称这是官方历史调价日期。较早事件暂不自动套用当前价。历史参考价的显式重算与界面依据说明仍待实现。已记录的费用不会随采集回放自动替换。
