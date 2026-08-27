import axios, { AxiosInstance, InternalAxiosRequestConfig } from 'axios';

const STORAGE_KEY = 'inkuo_admin_token';

export const tokenStore = {
  get: () => localStorage.getItem(STORAGE_KEY) ?? '',
  set: (token: string) => localStorage.setItem(STORAGE_KEY, token),
  clear: () => localStorage.removeItem(STORAGE_KEY),
};

// The cloud-server serializes JSON as snake_case (to match the desktop
// Rust client's wire types). The admin frontend is written in
// camelCase (TS convention), so we translate on the wire with two
// converters — request bodies go camel→snake, responses snake→camel.
const camelToSnake = (s: string) =>
  s.replace(/([A-Z])/g, (_m, c: string, i: number) => (i === 0 ? c.toLowerCase() : `_${c.toLowerCase()}`));

const snakeToCamel = (s: string) =>
  s.replace(/_([a-z0-9])/g, (_m, c: string) => c.toUpperCase());

const deepKeys = (input: unknown, transform: (s: string) => string): unknown => {
  if (Array.isArray(input)) return input.map((v) => deepKeys(v, transform));
  // Transform JSON records only. Blob, ArrayBuffer, URLSearchParams and other
  // browser objects would otherwise be replaced with an empty object by
  // Object.entries(), corrupting binary responses and non-JSON requests.
  if (
    input
    && typeof input === 'object'
    && (Object.getPrototypeOf(input) === Object.prototype || Object.getPrototypeOf(input) === null)
  ) {
    const out: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(input as Record<string, unknown>)) {
      out[transform(k)] = deepKeys(v, transform);
    }
    return out;
  }
  return input;
};

export const api: AxiosInstance = axios.create({
  baseURL: '',
  timeout: 30_000,
});

api.interceptors.request.use((config: InternalAxiosRequestConfig) => {
  const token = tokenStore.get();
  if (token) {
    config.headers.set('Authorization', `Bearer ${token}`);
  }
  if (config.data && typeof config.data === 'object' && !(config.data instanceof FormData)) {
    config.data = deepKeys(config.data, camelToSnake);
  }
  return config;
});

api.interceptors.response.use((response) => {
  if (response.data && typeof response.data === 'object') {
    response.data = deepKeys(response.data, snakeToCamel);
  }
  return response;
});

api.interceptors.response.use(
  (response) => response,
  (error) => {
    if (error.response?.status === 401) {
      tokenStore.clear();
      // Authentication has expired; use the concrete deployment path because
      // this redirect happens outside React Router.
      if (!window.location.pathname.endsWith('/login')) {
        window.location.href = '/admin/login';
      }
    }
    return Promise.reject(error);
  }
);

export function getApiErrorMessage(error: unknown, fallback: string): string {
  if (axios.isAxiosError<{ error?: unknown }>(error)) {
    const detail = error.response?.data?.error;
    if (typeof detail === 'string' && detail.trim()) return detail;
  }
  return fallback;
}

export function isRequestCancelled(error: unknown): boolean {
  return axios.isCancel(error);
}
