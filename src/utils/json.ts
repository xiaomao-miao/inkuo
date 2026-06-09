import { parse as parsePartialJson } from 'jsonchunk';

export function parsePartialJsonObject(raw: string): Record<string, unknown> {
  if (!raw.trim()) {
    return {};
  }

  const partial = parsePartialJson(raw);
  if (partial && typeof partial === 'object' && !Array.isArray(partial)) {
    return partial as Record<string, unknown>;
  }

  return {};
}
