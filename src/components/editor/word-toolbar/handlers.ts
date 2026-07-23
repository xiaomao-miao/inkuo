// Re-export of `./handlers/index`.
//
// The original 781-line `handlers.ts` has been split into focused
// sub-hooks under `./handlers/` (one per domain). The composer
// `useWordToolbarHandlers` lives in `./handlers/index.ts`.
//
// New code can import individual sub-hooks (e.g.
// `useLinkHandlers`) when it only needs a slice.

export * from './handlers/index';
