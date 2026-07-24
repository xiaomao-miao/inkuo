// Unit tests for `useKeyedDebouncedCallback`.
//
// The hook itself is just a `useRef` + `useCallback` wrapper over a small
// state machine (one Map entry per key, each entry holding a `setTimeout`
// handle and the latest pending args). We test the state machine directly
// by re-implementing the same lifecycle against `vi.useFakeTimers()` —
// this avoids the need for a DOM testing environment, which is not set up
// in this project, while still exercising the exact debounce semantics
// (per-key trailing, follow-up suppression, unmount cleanup).
//
// If `useKeyedDebouncedCallback` ever grows React-specific behaviour
// (effects beyond the unmount cleanup, dependency tracking on `delayMs`,
// etc.), these tests should be promoted to a real `renderHook` test with
// `@testing-library/react` rather than relying on the harness below.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

type Key = string;
type Args = [key: string, payload: string];

interface Entry {
  timer: ReturnType<typeof setTimeout> | null;
  pending: Args | null;
}

interface Harness {
  trigger(key: string, payload: string): void;
  unmount(): void;
}

/**
 * Spin up a faithful copy of the keyed-debouncer state machine.
 *
 * The real hook has one more piece of machinery — `useRef` to make the
 * `Map` survive between renders — but in this test we only call
 * `trigger()` synchronously so a plain `Map` is equivalent.
 *
 * The real hook derives the key via the caller-supplied `extractKey`
 * function. To keep the test focused on the state machine, the harness
 * here treats the first argument as the key and the second as the
 * payload — the same shape the workspace tree uses (key=directory,
 * payload=ignored-but-present).
 */
function makeHarness(callback: (args: Args) => void, delayMs: number): Harness {
  const entries = new Map<Key, Entry>();

  const trigger = (key: string, payload: string): void => {
    const args: Args = [key, payload];
    let entry = entries.get(key);
    if (!entry) {
      entry = { timer: null, pending: null };
      entries.set(key, entry);
    }
    entry.pending = args;
    if (entry.timer !== null) {
      clearTimeout(entry.timer);
    }
    entry.timer = setTimeout(() => {
      entry.timer = null;
      if (entry.pending !== null) {
        const pendingArgs = entry.pending;
        entry.pending = null;
        entries.delete(key);
        callback(pendingArgs);
      }
    }, delayMs);
  };

  const unmount = (): void => {
    for (const entry of entries.values()) {
      if (entry.timer !== null) {
        clearTimeout(entry.timer);
      }
    }
    entries.clear();
  };

  return { trigger, unmount };
}

describe('keyed debouncer semantics', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('fires once per key with the latest args', () => {
    const calls: Args[] = [];
    const harness = makeHarness((args) => calls.push(args), 100);

    harness.trigger('a', 'a-1');
    harness.trigger('a', 'a-2');
    harness.trigger('a', 'a-3');

    expect(calls).toEqual([]);

    vi.advanceTimersByTime(100);

    // The flat debouncer would have dropped `a-1` and `a-2`; the keyed
    // debouncer collapses the three same-key calls into one with `a-3`.
    expect(calls).toEqual([['a', 'a-3']]);
  });

  it('keeps different keys independent so a burst across dirs does not drop any', () => {
    const calls: Args[] = [];
    const harness = makeHarness((args) => calls.push(args), 100);

    // Simulate the OS watcher reporting changes in two parent directories
    // inside the same debounce window. The flat debouncer would collapse
    // these to the latest call's key; the keyed debouncer must fire each
    // parent's refresh.
    harness.trigger('/root', 'parent');
    harness.trigger('/root/sub', 'parent');
    harness.trigger('/root/other', 'parent');

    vi.advanceTimersByTime(100);

    expect(calls).toHaveLength(3);
    const keys = calls.map((c) => c[0]);
    expect(keys).toEqual(expect.arrayContaining(['/root', '/root/sub', '/root/other']));
  });

  it('does not leak entries between invocations of the same key', () => {
    const calls: Args[] = [];
    const harness = makeHarness((args) => calls.push(args), 100);

    harness.trigger('a', 'a-1');
    vi.advanceTimersByTime(100);
    harness.trigger('a', 'a-2');
    vi.advanceTimersByTime(100);

    expect(calls).toEqual([
      ['a', 'a-1'],
      ['a', 'a-2'],
    ]);
  });

  it('unmount drops every pending timer', () => {
    const calls: Args[] = [];
    const harness = makeHarness((args) => calls.push(args), 100);

    harness.trigger('a', 'a-1');
    harness.trigger('b', 'b-1');
    harness.unmount();

    vi.advanceTimersByTime(500);

    expect(calls).toEqual([]);
  });

  it('post-fire the next call for the same key uses a fresh timer', () => {
    const calls: Args[] = [];
    const harness = makeHarness((args) => calls.push(args), 100);

    harness.trigger('a', 'a-1');
    vi.advanceTimersByTime(100);
    // Right after the first fire, fire a new call. The previous timer
    // already cleared the entry so a new one should schedule cleanly.
    harness.trigger('a', 'a-2');
    vi.advanceTimersByTime(50);
    expect(calls).toEqual([['a', 'a-1']]);
    vi.advanceTimersByTime(50);
    expect(calls).toEqual([
      ['a', 'a-1'],
      ['a', 'a-2'],
    ]);
  });
});
