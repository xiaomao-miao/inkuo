/**
 * Mirrors the JSON shape returned by the public /api/releases endpoint on
 * the inkuo cloud server. Field names are camelCase because the
 * server-side minimal API is configured with snake_case outgoing + the
 * admin SPA's axios interceptor does the conversion — but the marketing
 * site calls /api/releases directly with fetch(), so the server's actual
 * wire format (snake_case) is what we see.
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
  const r = await fetch('/api/releases', { headers: { accept: 'application/json' } });
  if (!r.ok) throw new Error(`Failed to load releases: HTTP ${r.status}`);
  return r.json();
}