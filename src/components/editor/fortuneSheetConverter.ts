// Thin re-export of `./fortuneConverter` so the legacy import
// `from './fortuneSheetConverter'` continues to resolve. The original
// single-file implementation has been split into the modules under
// `fortuneConverter/`; new code should import from there directly.

export * from './fortuneConverter';