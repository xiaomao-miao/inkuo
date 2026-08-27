import { describe, expect, it } from 'vitest';

import { validateNewPassword } from './passwordPolicy';

describe('validateNewPassword', () => {
  it('enforces the minimum length', () => {
    expect(validateNewPassword('short')).toBe('密码至少需要 12 个字符');
  });

  it('enforces BCrypt\'s UTF-8 byte limit', () => {
    expect(validateNewPassword('密'.repeat(25))).toBe('密码不能超过 72 个 UTF-8 字节');
  });

  it('accepts a valid passphrase', () => {
    expect(validateNewPassword('a-secure-passphrase')).toBeNull();
  });
});
