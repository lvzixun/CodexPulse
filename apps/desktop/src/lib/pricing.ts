import { formatUsd } from './format.ts';
import { translateFor, currentLanguage, type Language } from './i18n.ts';
import type { ReferenceEstimate } from './types';

const reasons: Record<string, string> = {
  unknown_model_id: '模型标识未关联',
  model_price_missing: '暂无官方参考价',
  token_details_missing: '缺少用量明细',
  invalid_token_counts: '用量明细异常',
  invalid_usage_record: '用量记录异常',
  pricing_tier_missing: '暂无适用档位价格',
};
export function referenceMoney(
  value: ReferenceEstimate,
  language: Language = currentLanguage(),
): string {
  const t = (key: string) => translateFor(language, key);
  if (value.error) return t('估算失败');
  if (value.pending) return t('计算中…');
  const max = value.max_cost_nanousd ?? value.cost_nanousd;
  if (value.unpriced_events && max === 0) return t(reasons[value.reasons?.[0] ?? ''] ?? '待计价');
  const minText = formatUsd(value.cost_nanousd);
  const maxText = formatUsd(max);
  return `${minText === maxText ? minText : `${minText}–${maxText}`}${value.unpriced_events ? '*' : ''}`;
}
export function referenceExplanation(
  value: ReferenceEstimate,
  language: Language = currentLanguage(),
): string {
  const t = (key: string) => translateFor(language, key);
  const notes = (value.reasons ?? []).map((code) => reasons[code]).filter(Boolean);
  const flags = value.estimate_flags ?? 0;
  for (const [bit, message] of [
    [1, '服务档位未知，按标准档估算'],
    [2, '单次请求大小未知，包含长短上下文价格区间'],
    [4, '缓存写入数量或时长未知，显示费用区间'],
    [8, '缓存命中数量未知，显示费用区间'],
    [16, '包含峰谷价格区间'],
    [32, '按模型官方价估算，非网关实际账单'],
  ] as const)
    if (flags & bit) notes.push(message);
  if (value.error) notes.unshift('估算暂时失败，稍后自动重试');
  return notes.map(t).join(' · ');
}
