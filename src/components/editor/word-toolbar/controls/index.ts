// Inline form controls used inside the Word toolbar's row 1 (font + colors +
// insert menu). These were inlined in WordToolbar.tsx and pulled out so that
// the toolbar's main file is focused on layout. Each component is small,
// self-contained, and depends only on the shared `primitives.tsx` chrome
// (`DropdownPortal`) and `constants.ts` for its static options.
//
// None of these controls talk to ProseMirror directly — they are presentational
// wrappers that fire callbacks; the parent `WordToolbar` is responsible for
// routing the callbacks to the editor (see `handlers.ts`).

export { FontSizeControl } from './FontSizeControl';
export type { FontSizeDropdownProps } from './FontSizeControl';

export { ColorPicker } from './ColorPicker';
export type { ColorPickerProps } from './ColorPicker';

export { PageColorPicker } from './PageColorPicker';
export type { PageColorPickerProps } from './PageColorPicker';

export { TablePicker } from './TablePicker';
export type { TablePickerProps } from './TablePicker';

export { SymbolPicker } from './SymbolPicker';
export type { SymbolPickerProps } from './SymbolPicker';