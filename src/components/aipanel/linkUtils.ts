export function safelyDecodeHref(href: string | undefined): string | undefined {
  if (!href) return href;
  try {
    return decodeURIComponent(href);
  } catch {
    return href;
  }
}

export function isExternalHttpLink(href: string | undefined): boolean {
  return /^https?:\/\//i.test(href ?? '');
}

export function isLikelyWorkspacePath(href: string | undefined): boolean {
  if (!href || href.startsWith('#')) return false;
  if (/^[a-z][a-z0-9+.-]*:/i.test(href) && !/^[A-Za-z]:[\\/]/.test(href)) {
    return false;
  }

  return href.startsWith('/')
    || href.startsWith('~')
    || href.startsWith('./')
    || href.startsWith('../')
    || /^[A-Za-z]:[\\/]/.test(href)
    || href.includes('/')
    || /\.[A-Za-z0-9]{1,12}(?:[?#].*)?$/.test(href);
}

export function resolveWorkspaceHref(href: string, workspacePath: string | undefined): string {
  const isAbsolute = href.startsWith('/')
    || href.startsWith('~')
    || href.startsWith('\\\\')
    || /^[A-Za-z]:[\\/]/.test(href);
  if (isAbsolute || !workspacePath) return href;

  const separator = workspacePath.includes('\\') ? '\\' : '/';
  return `${workspacePath.replace(/[\\/]+$/, '')}${separator}${href.replace(/^\.[\\/]/, '')}`;
}
