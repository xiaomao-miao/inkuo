export function validateNewPassword(password: string): Promise<void> {
  if (password.length < 12) return Promise.reject(new Error('密码至少需要 12 个字符'));
  if (new TextEncoder().encode(password).length > 72) {
    return Promise.reject(new Error('密码不能超过 72 个 UTF-8 字节'));
  }
  return Promise.resolve();
}
