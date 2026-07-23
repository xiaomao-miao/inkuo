import { describe, expect, it } from 'vitest';
import {
  CHAT_MODES,
  CHAT_MODE_HINT,
  CHAT_MODE_LABEL,
  DEFAULT_CHAT_MODE,
  nextChatMode,
} from './chatModes';

describe('constants/chatModes', () => {
  it('exposes a non-empty cycle order', () => {
    expect(CHAT_MODES.length).toBeGreaterThan(0);
    // Cycle order must be unique so `nextChatMode` is deterministic.
    expect(new Set(CHAT_MODES).size).toBe(CHAT_MODES.length);
  });

  it('default mode is the first cycle entry', () => {
    expect(DEFAULT_CHAT_MODE).toBe(CHAT_MODES[0]);
  });

  it('has a label and hint for every mode', () => {
    for (const mode of CHAT_MODES) {
      expect(CHAT_MODE_LABEL[mode]).toBeTruthy();
      expect(CHAT_MODE_HINT[mode]).toBeTruthy();
    }
  });

  describe('nextChatMode', () => {
    it('advances to the next mode in the cycle', () => {
      for (let i = 0; i < CHAT_MODES.length - 1; i += 1) {
        const current = CHAT_MODES[i];
        const expected = CHAT_MODES[i + 1];
        expect(nextChatMode(current)).toBe(expected);
      }
    });

    it('wraps from the last mode back to the first', () => {
      expect(nextChatMode(CHAT_MODES[CHAT_MODES.length - 1])).toBe(CHAT_MODES[0]);
    });

    it('returns the first mode for unknown input', () => {
      // Cast through `unknown` because the parameter type forbids non-members
      // at compile time, but the runtime guard must still hold.
      expect(nextChatMode('not-a-mode' as unknown as typeof CHAT_MODES[number]))
        .toBe(CHAT_MODES[0]);
    });

    it('returns the first mode after stepping through the whole cycle', () => {
      let current: typeof CHAT_MODES[number] = CHAT_MODES[0];
      for (let i = 0; i < CHAT_MODES.length; i += 1) {
        current = nextChatMode(current);
      }
      expect(current).toBe(CHAT_MODES[0]);
    });
  });
});
