export const MAX_SINGLE_CREDIT_POINTS = 1_000_000_000_000;
export const MAX_ACCOUNT_BALANCE_POINTS = 5_000_000_000_000;
export const MAX_CODE_USES = 1_000_000;
export const MIN_CODE_LENGTH = 4;
export const MAX_CODE_LENGTH = 64;
export const MAX_ADJUSTMENT_REASON_LENGTH = 500;
export const CODE_PATTERN = /^[A-Za-z0-9_-]+$/;
export const trimCode = (value?: string) => value?.trim();
