const DEVELOPMENT_CLOUD_URL = 'http://localhost:8080';
const PRODUCTION_CLOUD_URL = 'https://cloud.inkuo.com';

/** Resolve and normalize the cloud endpoint selected at build time. */
export function resolveCloudBaseUrl(
  configuredUrl: string | undefined,
  isDevelopment: boolean,
): string {
  const fallback = isDevelopment ? DEVELOPMENT_CLOUD_URL : PRODUCTION_CLOUD_URL;
  const candidate = configuredUrl?.trim() || fallback;

  try {
    const parsed = new URL(candidate);
    if (!['http:', 'https:'].includes(parsed.protocol) || parsed.username || parsed.password) {
      throw new Error('unsupported cloud URL');
    }
    return parsed.toString().replace(/\/$/, '');
  } catch {
    console.error('[cloud-config] VITE_INKUO_CLOUD_BASE_URL is invalid; using the safe default');
    return fallback;
  }
}

const INKUO_CLOUD_BASE_URL = resolveCloudBaseUrl(
  import.meta.env.VITE_INKUO_CLOUD_BASE_URL,
  import.meta.env.DEV,
);

export function getCloudBaseUrl(): string {
  return INKUO_CLOUD_BASE_URL;
}
