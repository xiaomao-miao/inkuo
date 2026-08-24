import { describe, expect, it } from 'vitest';
import { shouldSubmitComposerMessage } from './composerKeyboard';

describe('shouldSubmitComposerMessage', () => {
  it('submits a normal Enter', () => {
    expect(shouldSubmitComposerMessage({ key: 'Enter', shiftKey: false })).toBe(true);
  });

  it('keeps Shift+Enter as a newline', () => {
    expect(shouldSubmitComposerMessage({ key: 'Enter', shiftKey: true })).toBe(false);
  });

  it('does not submit while an IME composition is active', () => {
    expect(shouldSubmitComposerMessage({
      key: 'Enter', shiftKey: false, isComposing: true,
    })).toBe(false);
  });

  it('supports the Windows WebView IME keyCode fallback', () => {
    expect(shouldSubmitComposerMessage({
      key: 'Enter', shiftKey: false, keyCode: 229,
    })).toBe(false);
  });

  it('does not submit other keys', () => {
    expect(shouldSubmitComposerMessage({ key: 'Space', shiftKey: false })).toBe(false);
  });
});
