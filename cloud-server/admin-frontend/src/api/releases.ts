import { api } from './client';

export interface Release {
  id: string;
  version: string;
  channel: string;
  platform: string;
  architecture: string;
  fileName: string;
  fileSizeBytes: number;
  sha256: string;
  downloadUrl: string;
  storagePath?: string;
  releaseNotes: string | null;
  isLatest: boolean;
  enabled: boolean;
  createdAt: string;
  createdByAdminId: string | null;
}

export interface UploadReleaseInput {
  version: string;
  channel: string;
  platform: string;
  architecture: string;
  releaseNotes?: string;
  isLatest: boolean;
  enabled: boolean;
  file: File;
}

export const releasesApi = {
  list: () => api.get<Release[]>('/api/releases/admin/all').then(r => r.data),
  upload: (
    data: UploadReleaseInput,
    options?: { signal?: AbortSignal; onProgress?: (percent: number) => void },
  ) => {
    const fd = new FormData();
    fd.append('file', data.file);
    fd.append('version', data.version);
    fd.append('channel', data.channel);
    fd.append('platform', data.platform);
    fd.append('architecture', data.architecture);
    if (data.releaseNotes) fd.append('releaseNotes', data.releaseNotes);
    fd.append('isLatest', String(data.isLatest));
    fd.append('enabled', String(data.enabled));
    return api.post<{ id: string; version: string; fileName: string; fileSizeBytes: number; sha256: string; downloadUrl: string; createdAt: string }>(
      '/api/releases/upload', fd, {
        signal: options?.signal,
        // Installer uploads can legitimately take much longer than the JSON
        // client's 30-second default timeout.
        timeout: 0,
        onUploadProgress: (event) => {
          if (event.total && event.total > 0) {
            options?.onProgress?.(Math.min(100, Math.round((event.loaded / event.total) * 100)));
          }
        },
      },
    ).then(r => r.data);
  },
  setEnabled: (id: string, enabled: boolean) =>
    api.patch<{ id: string; enabled: boolean }>(`/api/releases/${id}/enabled`, { enabled }).then(r => r.data),
  setLatest: (id: string, isLatest: boolean) =>
    api.patch<{ id: string; isLatest: boolean }>(`/api/releases/${id}/latest`, { isLatest }).then(r => r.data),
  remove: (id: string) =>
    api.delete<{ message: string; id: string }>(`/api/releases/${id}`).then(r => r.data),
};

export function formatBytes(bytes: number): string {
  if (!bytes) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB'];
  let i = 0;
  let n = bytes;
  while (n >= 1024 && i < units.length - 1) { n /= 1024; i++; }
  return `${n.toFixed(n < 10 && i > 0 ? 2 : 1)} ${units[i]}`;
}
