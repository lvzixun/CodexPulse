import type { SpeedPoint, SpeedSummary } from './types';
export function rateValue(
  value?: Pick<SpeedSummary, 'output_tokens' | 'elapsed_ms'> | null,
): number | null {
  if (
    !value ||
    !Number.isFinite(value.output_tokens) ||
    !Number.isFinite(value.elapsed_ms) ||
    value.output_tokens <= 0 ||
    value.elapsed_ms <= 0
  )
    return null;
  const rate = (value.output_tokens * 1000) / value.elapsed_ms;
  return Number.isFinite(rate) ? rate : null;
}
export function tierKey(tier?: string | null): string {
  switch (tier) {
    case 'fast':
    case 'priority':
      return 'Fast';
    case 'standard':
    case 'default':
      return '标准';
    case 'mixed':
      return '混合';
    case 'ultra_fast':
      return 'Ultra Fast';
    case 'flex':
      return 'Flex';
    case 'batch':
      return 'Batch';
    default:
      return '未知';
  }
}
export function speedGeometry(samples: SpeedPoint[]) {
  // Equal measurement times share one weighted plotted point, avoiding zero-width curves.
  const grouped = new Map<number, SpeedPoint>();
  for (const point of samples) {
    const at = Date.parse(point.measured_at);
    if (!Number.isFinite(at) || rateValue(point) === null) continue;
    const prior = grouped.get(at);
    grouped.set(
      at,
      prior
        ? {
            ...prior,
            output_tokens: prior.output_tokens + point.output_tokens,
            elapsed_ms: prior.elapsed_ms + point.elapsed_ms,
            samples: prior.samples + point.samples,
            service_tier: prior.service_tier === point.service_tier ? prior.service_tier : 'mixed',
          }
        : { ...point },
    );
  }
  const ordered = [...grouped].sort((a, b) => a[0] - b[0]);
  const first = ordered[0]?.[0] ?? 0,
    last = ordered.at(-1)?.[0] ?? first;
  const max = Math.max(10, ...ordered.map(([, p]) => rateValue(p)!));
  const step = 10 ** Math.floor(Math.log10(max));
  const ceiling = Math.ceil(max / step) * step;
  const points = ordered.map(([time, sample]) => ({
    time,
    sample,
    x: last === first ? 173 : 30 + ((time - first) / (last - first)) * 281,
    y: 96 - (rateValue(sample)! / ceiling) * 80,
  }));
  const f = (n: number) => n.toFixed(3);
  // Horizontal endpoint tangents keep every segment inside its two measured values.
  const path = points.length
    ? `M${f(points[0].x)},${f(points[0].y)}` +
      points
        .slice(1)
        .map((p, i) => {
          const a = points[i],
            dx = (p.x - a.x) / 3;
          return ` C${f(a.x + dx)},${f(a.y)} ${f(p.x - dx)},${f(p.y)} ${f(p.x)},${f(p.y)}`;
        })
        .join('')
    : '';
  return { points, path, first, last, ceiling };
}
export function speedTime(at: number, locale: string, timezone: string, withDay = false): string {
  return new Intl.DateTimeFormat(locale, {
    timeZone: timezone,
    ...(withDay ? { month: '2-digit', day: '2-digit' } : {}),
    hour: '2-digit',
    minute: '2-digit',
    hourCycle: 'h23',
  }).format(at);
}
