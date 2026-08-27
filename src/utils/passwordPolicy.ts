export function validateNewPassword(password: string): string | null {
  if (password.length < 12) return '密码至少需要 12 个字符';
  if (new TextEncoder().encode(password).length > 72) {
    return '密码不能超过 72 个 UTF-8 字节';
  }
  return null;
}
