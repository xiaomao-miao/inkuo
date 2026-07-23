// Word-toolbar popovers. These were previously inlined in WordToolbar.tsx;
// splitting them out keeps the toolbar's main file focused on layout and
// lets each panel evolve independently.
//
// All four panels wrap the shared `FormPopover` chrome from `../primitives`
// and are presentational — they fire callbacks; the parent `WordToolbar`
// (via `../handlers.ts`) routes the callbacks to the editor.

export { LinkPopover } from './LinkPopover';
export type { LinkPopoverProps } from './LinkPopover';

export { MathPopover } from './MathPopover';
export type { MathPopoverProps } from './MathPopover';

export { WatermarkPopover } from './WatermarkPopover';
export type {
  WatermarkPopoverProps,
  WatermarkConfig,
  CurrentWatermark,
} from './WatermarkPopover';

export { HeaderFooterPopover } from './HeaderFooterPopover';
export type { HeaderFooterPopoverProps, HeaderFooterConfig } from './HeaderFooterPopover';