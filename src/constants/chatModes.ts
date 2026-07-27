/**
 * Chat mode constants — `agent` is the only mode.
 *
 * The constant is exported for backwards-compat with session
 * initializers that key off `DEFAULT_CHAT_MODE`; the other exports
 * (cycle order, labels, hints) used to drive a now-removed mode
 * switcher in the composer.
 */
import type { ChatMode } from '../types';

/** Mode the panel boots into for a brand-new session. */
export const DEFAULT_CHAT_MODE: ChatMode = 'agent';