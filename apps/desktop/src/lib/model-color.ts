// Derive color from the model name so refreshed rankings and pagination do not recolor it.
export function modelColor(model: string): string {
  let hash = 2166136261;
  for (const character of model.toLowerCase())
    hash = Math.imul(hash ^ character.charCodeAt(0), 16777619);
  // Mix upper bits too: similarly named versions should not share their suffix's color.
  hash ^= hash >>> 16;
  hash = Math.imul(hash, 0x85ebca6b);
  hash ^= hash >>> 13;
  return `var(--cp-model-${(hash >>> 0) % 8})`;
}
