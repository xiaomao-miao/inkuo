/**
 * Cycles through an array of strings, typing and deleting each one in turn
 * to create a terminal-style "typewriter" effect.
 *
 * Usage:
 *   const tw = new Typewriter(el, ['让你专注想做什么', '帮你找资料', ...], { typeMs: 60, deleteMs: 30 });
 *   tw.start();
 *   // later: tw.stop();
 */
export interface TypewriterOptions {
  typeMs?: number;   // delay between typed characters
  deleteMs?: number;  // delay between deleted characters
  holdMs?: number;    // how long the word stays fully typed before deleting
  betweenMs?: number; // how long the slot is empty before the next word starts
}

export class Typewriter {
  private el: HTMLElement;
  private words: string[];
  private opts: Required<TypewriterOptions>;
  private idx = 0;
  private charIdx = 0;
  private mode: 'typing' | 'holding' | 'deleting' | 'waiting' = 'typing';
  private timer: number | null = null;
  private running = false;

  constructor(el: HTMLElement, words: string[], opts: TypewriterOptions = {}) {
    this.el = el;
    this.words = words;
    this.opts = {
      typeMs: 60, deleteMs: 30, holdMs: 1400, betweenMs: 400,
      ...opts,
    };
  }

  start(): void {
    if (this.running) return;
    this.running = true;
    this.tick();
  }

  stop(): void {
    this.running = false;
    if (this.timer != null) {
      window.clearTimeout(this.timer);
      this.timer = null;
    }
  }

  private tick = (): void => {
    if (!this.running) return;
    const word = this.words[this.idx];
    let delay = this.opts.typeMs;

    switch (this.mode) {
      case 'typing':
        if (this.charIdx < word.length) {
          this.charIdx++;
        } else {
          this.mode = 'holding';
          delay = this.opts.holdMs;
          break;
        }
        delay = this.opts.typeMs + (Math.random() * 40 - 20); // jitter
        break;

      case 'holding':
        this.mode = 'deleting';
        delay = this.opts.deleteMs;
        break;

      case 'deleting':
        if (this.charIdx > 0) {
          this.charIdx--;
        } else {
          this.mode = 'waiting';
          delay = this.opts.betweenMs;
          break;
        }
        delay = this.opts.deleteMs;
        break;

      case 'waiting':
        this.idx = (this.idx + 1) % this.words.length;
        this.mode = 'typing';
        delay = this.opts.typeMs;
        break;
    }

    this.el.textContent = word.slice(0, this.charIdx);
    this.timer = window.setTimeout(this.tick, Math.max(15, delay));
  };
}