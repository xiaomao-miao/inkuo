export interface ComposerKeyboardEvent {
  key: string;
  shiftKey: boolean;
  isComposing?: boolean;
  keyCode?: number;
}

/** Prevent Enter used to confirm a Chinese/Japanese/Korean IME candidate
 * from accidentally sending a half-composed prompt. keyCode 229 covers
 * older Windows WebView IME implementations that omit isComposing. */
export function shouldSubmitComposerMessage(event: ComposerKeyboardEvent): boolean {
  return event.key === 'Enter'
    && !event.shiftKey
    && event.isComposing !== true
    && event.keyCode !== 229;
}
