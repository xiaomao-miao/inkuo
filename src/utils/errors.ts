export function extractErrorMessage(error: unknown, fallback = '发生未知错误'): string {
  if (error instanceof Error && error.message.trim()) {
    return error.message;
  }

  if (typeof error === 'string' && error.trim()) {
    return error;
  }

  if (error && typeof error === 'object' && 'message' in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === 'string' && message.trim()) {
      return message;
    }
  }

  return fallback;
}

export function reportError(scope: string, error: unknown): string {
  const message = extractErrorMessage(error);
  console.error(`[${scope}]`, error);
  return message;
}
