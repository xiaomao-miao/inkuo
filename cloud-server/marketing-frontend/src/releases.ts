/**
 * Mirrors the JSON shape returned by the public /api/releases endpoint on
 * the inkuo cloud server. Field names are camelCase because the
 * marketing site calls /api/releases directly with fetch(), so these fields
 * intentionally use the server's actual snake_case wire format. The separate
 * admin SPA converts that payload to camelCase in its axios interceptor.
 */
export interface Release {
  id: string;
  version: string;
  channel: string;
  platform: string;
  architecture: string;
  file_name: string;
  file_size_bytes: number;
  sha256: string;
  download_url: string;
  release_notes: string | null;
  is_latest: boolean;
  created_at: string;
}

export function formatBytes(bytes: number): string {
  if (!bytes) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB'];
  let i = 0;
  let n = bytes;
  while (n >= 1024 && i < units.length - 1) { n /= 1024; i++; }
  return `${n.toFixed(n < 10 && i > 0 ? 2 : 1)} ${units[i]}`;
}

export async function fetchReleases(): Promise<Release[]> {
  const controller = new AbortController();
  const timer = window.setTimeout(() => controller.abort(), 10_000);
  try {
    const response = await fetch('/api/releases', {
      headers: { accept: 'application/json' },
      cache: 'no-store',
      signal: controller.signal,
    });
    if (!response.ok) throw new Error(`Failed to load releases: HTTP ${response.status}`);
    const payload: unknown = await response.json();
    if (!Array.isArray(payload) || !payload.every(isRelease)) {
      throw new Error('Release service returned an invalid payload');
    }
    return payload;
  } finally {
    window.clearTimeout(timer);
  }
}

function isRelease(value: unknown): value is Release {
  if (!value || typeof value !== 'object') return false;
  const item = value as Record<string, unknown>;
  return typeof item.id === 'string'
    && typeof item.version === 'string'
    && typeof item.channel === 'string'
    && typeof item.platform === 'string'
    && typeof item.architecture === 'string'
    && typeof item.file_name === 'string'
    && typeof item.file_size_bytes === 'number'
    && Number.isFinite(item.file_size_bytes)
    && item.file_size_bytes >= 0
    && typeof item.sha256 === 'string'
    && typeof item.download_url === 'string'
    && (item.release_notes === null || typeof item.release_notes === 'string')
    && typeof item.is_latest === 'boolean'
    && typeof item.created_at === 'string';
}
