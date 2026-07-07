/**
 * Streaming plan parser.
 *
 * Parses the accumulating model output into a structured PlanOutput.
 * The model is required to output:
 *   1. Free-form Markdown prose
 *   2. A single ```plan code block containing JSON
 *
 * This module handles partial delivery: the ```plan block may not have
 * arrived yet, or may be incomplete. We use the sentinel ```plan\n to
 * split the accumulated text into a "before" Markdown section and a
 * "candidate JSON" section.
 */

import type { PlanOutput } from '../../types';

const PLAN_FENCE = '```plan';

/** Result of parsing a partial plan text. */
export interface StreamingPlanParse {
  /** Markdown prose before the ```plan block. Always present (may be empty). */
  markdown: string;
  /**
   * The parsed plan object. `null` when:
   *   - The ```plan fence hasn't been seen yet
   *   - The block is open (no closing ``` found)
   *   - JSON parsing failed after the block closed
   */
  plan: PlanOutput | null;
  /**
   * Error message when the block closed but JSON was invalid.
   * `undefined` when no error or when block is still open.
   */
  parseError?: string;
  /**
   * `true` when the ```plan fence has been seen but the closing ```
   * has not yet arrived. Signals the UI to show "collecting...".
   */
  hasOpenBlock: boolean;
}

/**
 * Parse a (potentially partial) accumulated plan text.
 *
 * Strategy:
 *   - Find the LAST occurrence of ```plan (there should only ever be one)
 *   - If not found: return { markdown: text, plan: null, hasOpenBlock: false }
 *   - If found but no closing ```: return markdown + { plan: null, hasOpenBlock: true }
 *   - If found with closing ```: extract JSON, try JSON.parse
 *     - Success → { markdown, plan, hasOpenBlock: false }
 *     - Failure → { markdown, plan: null, parseError, hasOpenBlock: false }
 */
export function parseStreamingPlan(text: string): StreamingPlanParse {
  const fenceIdx = text.lastIndexOf(PLAN_FENCE);

  if (fenceIdx === -1) {
    return { markdown: text, plan: null, hasOpenBlock: false };
  }

  const markdown = text.slice(0, fenceIdx).trimEnd();

  // Everything after the fence opening marker (```plan + newline)
  const afterFence = text.slice(fenceIdx + PLAN_FENCE.length);
  const closeIdx = afterFence.indexOf('```');

  // No closing fence yet
  if (closeIdx === -1) {
    return { markdown, plan: null, hasOpenBlock: true };
  }

  // Extract the JSON content between the fences (strip leading newline from afterFence if present)
  const rawJson = afterFence.slice(0, closeIdx).trim();

  try {
    const parsed = JSON.parse(rawJson) as PlanOutput;
    return { markdown, plan: parsed, hasOpenBlock: false };
  } catch (err) {
    return {
      markdown,
      plan: null,
      parseError: err instanceof Error ? err.message : 'Invalid JSON',
      hasOpenBlock: false,
    };
  }
}
