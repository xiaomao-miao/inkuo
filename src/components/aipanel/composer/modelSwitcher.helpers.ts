// Unified model switcher rendered next to the mode button.
//
// Lists every model the user can route through in a single dropdown,
// grouped into two `<optgroup>`s:
//   - 云端模型 — only shown when a cloud account is signed in
//   - 本地 API  — always shown
//
// Picking any option flips `cloud_mode_enabled` accordingly and writes
// the right id back into settings. The current selection is
// highlighted by matching the `<select>` value against the active
// id in the currently-active mode (cloud mode → cloud id, else →
// local config id).
//
// If neither group has any entries (e.g. no API configs and no
// cloud account) the switcher returns null — there's nothing
// meaningful the user could pick.

// Pure helpers — extracted so the React component stays declarative.

export type ModelKind = 'cloud' | 'local';

/**
 * Compose a `<select>` value for an entry. Cloud ids and local-config
 * ids are namespaced with a prefix so the two namespaces can't
 * collide (the store doesn't enforce id uniqueness across them).
 */
export function encodeSelectValue(kind: ModelKind, id: string): `cloud:${string}` | `local:${string}` {
  return kind === 'cloud' ? `cloud:${id}` : `local:${id}`;
}

/** Parse a `<select>` value back into a `{ kind, id }` pair, or null. */
export function decodeSelectValue(
  raw: string,
): { kind: ModelKind; id: string } | null {
  if (!raw) return null;
  const [kind, id] = raw.split(':', 2);
  if ((kind === 'cloud' || kind === 'local') && id) {
    return { kind, id };
  }
  return null;
}

interface ActiveSelection {
  cloudMode: boolean;
  activeCloudModelId: string | null;
  activeApiConfigId: string | null;
  fallbackLocalConfigId: string | null;
}

/**
 * Compute the highlighted `<select>` value (cloud-vs-local
 * encoded form) for the current mode's active id. Returns empty
 * string when nothing is selected AND no fallback exists.
 */
export function currentSelectValue(sel: ActiveSelection): string {
  const { cloudMode, activeCloudModelId, activeApiConfigId, fallbackLocalConfigId } = sel;
  if (cloudMode) {
    return activeCloudModelId ? encodeSelectValue('cloud', activeCloudModelId) : '';
  }
  if (activeApiConfigId) return encodeSelectValue('local', activeApiConfigId);
  if (fallbackLocalConfigId) return encodeSelectValue('local', fallbackLocalConfigId);
  return '';
}

/**
 * Label summarizing the active selection. Empty / null active ids
 * fall through to the matched model name, then to a "未选择"
 * placeholder so the user always sees something meaningful in the
 * tooltip.
 */
export function activeSelectionLabel(
  cloudMode: boolean,
  ctx: {
    activeCloudModelName: string | null;
    activeLocalConfigName: string | null;
    firstLocalConfigName: string | null;
  },
): string {
  if (cloudMode) {
    return `云端 · ${ctx.activeCloudModelName ?? '未选择'}`;
  }
  const name = ctx.activeLocalConfigName ?? ctx.firstLocalConfigName ?? '未选择';
  return `本地 · ${name}`;
}

interface GroupAvailability {
  hasCloudOptions: boolean;
  hasLocalOptions: boolean;
}

/** True when the switcher should hide entirely. */
export function shouldHideSwitcher(g: GroupAvailability): boolean {
  return !g.hasCloudOptions && !g.hasLocalOptions;
}