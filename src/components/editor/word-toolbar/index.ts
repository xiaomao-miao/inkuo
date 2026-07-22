/**
 * Word toolbar entry point.
 *
 * Splits the original monolithic 2424-line `WordToolbar.tsx` into a directory
 * of smaller, focused components. The directory-shape preparation lets us
 * stage the actual file split (ColorPicker, TablePicker, SymbolPicker, etc.)
 * without forcing every call site to update at once.
 *
 * Current status: the full implementation remains inside `WordToolbar.tsx`;
 * the directory structure here signals the destination layout for the
 * components that will eventually live as siblings.
 */
export { WordToolbar } from './WordToolbar';
export type { WordToolbarProps } from './WordToolbar';
