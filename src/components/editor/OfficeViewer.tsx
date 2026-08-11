import { useState, useCallback, useEffect, useRef, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Workbook } from '@fortune-sheet/react';
import type { WorkbookInstance } from '@fortune-sheet/react';
import type { Sheet as FortuneSheetCoreSheet } from '@fortune-sheet/core';
import { Save, Table2 } from 'lucide-react';
import { useKeyboardSave } from './useKeyboardSave';
import { useSidebarStore, useEditorStore, useNotificationStore } from '../../store';
import {
  rustWorkbookToFortuneSheets,
} from './fortuneSheetConverter';
import { fortuneSheetsToSheetJSBuffer } from './fortuneSheetConverter';
import type { RustXlsxWorkbook } from './fortuneSheetConverter';
import { reportError } from '../../utils/errors';
import styles from './OfficeViewer.module.css';
import '@fortune-sheet/react/dist/index.css';

interface FortuneSheetCellLike {
  v?: unknown;
  m?: unknown;
  f?: unknown;
  ct?: unknown;
}

interface FortuneSheetRowLike {
  [col: number]: FortuneSheetCellLike | null;
}

interface FortuneSheetDataMatrix {
  [row: number]: FortuneSheetRowLike;
}

interface FortuneSheetForFingerprint {
  id?: string;
  name?: string;
  status?: number;
  order?: number;
  hide?: number;
  data?: FortuneSheetDataMatrix | null;
  celldata?: Array<{ r?: number; c?: number; v?: unknown }> | null;
}

// Build a fingerprint of user-visible content (cell values, formulas,
// statuses, names). Workbook's internal context rebuilds (e.g. when
// ensureSheetIndex patches sheet ids, or when immer produce reconstructs
// the sheet array) can produce new object references but leave this
// fingerprint unchanged — those rebuilds correspond to non-user-driven
// emissions. Real user edits change a cell's `v`, `f`, `m`, or the sheet's
// name/status, so they DO change this fingerprint.
function fingerprintSheets(sheets: FortuneSheetCoreSheet[]): string {
  const parts: string[] = [];
  for (const sheet of sheets as unknown as FortuneSheetForFingerprint[]) {
    parts.push(`#${sheet.id ?? ''}|${sheet.name ?? ''}|${sheet.status ?? ''}|${sheet.order ?? ''}|${sheet.hide ?? ''}`);
    const data = sheet.data;
    if (data && typeof data === 'object') {
      const rowKeys = Object.keys(data).map(Number).sort((a, b) => a - b);
      for (const r of rowKeys) {
        const row = data[r];
        if (!row) continue;
        const colKeys = Object.keys(row).map(Number).sort((a, b) => a - b);
        for (const c of colKeys) {
          const cell = row[c];
          if (cell == null) continue;
          const v = cell.v;
          const m = cell.m;
          const f = cell.f;
          if (v !== undefined || m !== undefined || f !== undefined) {
            parts.push(`${r},${c}:v=${JSON.stringify(v ?? null)};m=${JSON.stringify(m ?? null)};f=${JSON.stringify(f ?? null)};`);
          }
        }
      }
    } else if (Array.isArray(sheet.celldata)) {
      const sorted = [...sheet.celldata].sort((a, b) => (a.r ?? 0) - (b.r ?? 0) || (a.c ?? 0) - (b.c ?? 0));
      for (const cell of sorted) {
        parts.push(`${cell.r ?? ''},${cell.c ?? ''}:${JSON.stringify(cell.v ?? null)};`);
      }
    }
    parts.push('|');
  }
  return parts.join('\n');
}

interface OfficeToolbarProps {
  fileName: string;
  isDirty: boolean;
  onSave: () => void;
  canSave: boolean;
  formatIcon?: React.ReactNode;
  editLabel?: string;
}

const OfficeToolbar: React.FC<OfficeToolbarProps> = ({
  fileName,
  isDirty,
  onSave,
  canSave,
  formatIcon,
  editLabel = '可编辑',
}) => {
  return (
    <div className={styles.editorToolbar}>
      <div className={styles.toolbarLeft}>
        {formatIcon}
        <span className={styles.fileName}>
          {fileName}
          {isDirty && <span className={styles.dirtyDot}>·</span>}
        </span>
      </div>
      <div className={styles.toolbarRight}>
        <span className={`${styles.editMode} ${isDirty ? styles.dirtyBadge : ''}`}>
          {editLabel}
        </span>
        <button
          className={`${styles.saveButton} ${isDirty ? styles.dirty : ''}`}
          onClick={onSave}
          disabled={!canSave}
          title="保存 (Ctrl+S)"
        >
          <Save size={14} />
          <span>保存</span>
        </button>
      </div>
    </div>
  );
};

// ─── Excel Editor (FortuneSheet) ─────────────────────────────────────────────────

interface ExcelEditorProps {
  filePath: string;
  fileName: string;
  isActive: boolean;
}

export const ExcelEditor: React.FC<ExcelEditorProps> = ({
  filePath,
  fileName,
  isActive,
}) => {
  const workbookRef = useRef<WorkbookInstance | null>(null);

  // ── State that actually needs to trigger re-renders ──────────────────────
  // fortuneSheets is the single source of truth for the Workbook data prop.
  // We intentionally update it ONLY for initial load and external file changes,
  // NOT for every edit (onChange only sets loadedSheetsRef for dirty tracking).
  const [fortuneSheets, setFortuneSheets] = useState<FortuneSheetCoreSheet[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [isDirty, setIsDirty] = useState(false);

  // ── Store actions (stable selectors, no re-render on other state) ─────────
  const setOpenTabDirty = useSidebarStore((s) => s.setOpenTabDirty);
  const setFortuneSheetsToStore = useEditorStore((s) => s.setFortuneSheets);
  const pushNotification = useNotificationStore((s) => s.pushNotification);

  // ── External file version watcher ─────────────────────────────────────────
  const officeBufferVersion = useEditorStore(
    (s) => s.documentContents[filePath]?.office.bufferVersion ?? 0,
  );

  // ── Stable callback refs — updated via ref, not state ────────────────────
  // These avoid creating new function references on every render,
  // which would cause FortuneSheet's useEffect([onChange]) to re-fire.
  const recalcAllSheetsRef = useRef<() => void>(() => {});
  const loadedSheetsRef = useRef<FortuneSheetCoreSheet[]>([]);
  const formulaInitDoneRef = useRef(false);
  // Snapshot of the most recently set sheets data. Workbook echoes this back
  // through onChange whenever its internal context is rebuilt from the data
  // prop (file open, settings change). When the echo equals the snapshot, no
  // user edit has occurred — don't flip isDirty.
  const lastLoadedSheetsRef = useRef<FortuneSheetCoreSheet[] | null>(null);
  // Cached fingerprint of lastLoadedSheetsRef, used as a cheap structural
  // comparison key to distinguish Workbook's internal context rebuilds from
  // genuine user edits.
  const lastLoadedFingerprintRef = useRef<string | null>(null);
  // Tracks the most recent onOp we observed from Workbook. Workbook's onOp
  // fires only for real operations (user edits, undo/redo, paste, etc.) —
  // it does NOT fire for internal context rebuilds (which is why it doesn't
  // fire for file open). We only flip isDirty when onOp fires with an op
  // that touches a user-visible field.
  const userEditSeenRef = useRef(false);

  // ── Hooks — empty. Formula recalculation is handled by FortuneSheet internally.
  const hooks = useMemo((): import('@fortune-sheet/core').Hooks => ({}), []);

  // onChange handler: stable ref function, called by Workbook on every change.
  // Updates loadedSheetsRef for save; does NOT call setFortuneSheets to avoid
  // triggering a Workbook data prop change on every keystroke (which re-initializes
  // the entire sheet and causes lag).
  const handleFortuneChange = useCallback(
    (changedSheets: FortuneSheetCoreSheet[]) => {
      // loadedSheetsRef is kept for backward compatibility, but save now reads
      // directly from workbookRef.getAllSheets() which is always current.
      loadedSheetsRef.current = changedSheets;

      // Formula calculation: FortuneSheet does NOT compute formula results on
      // initial data load — `setFormulaCellInfoMap` only builds the dependency
      // graph. Trigger `calculateFormula` for every sheet the first time the
      // Workbook reports its data is ready (workbookRef + a populated sheet).
      // This MUST run regardless of `userEditSeenRef`, because that flag only
      // fires after the user's first real op — by then the user has already
      // observed the uncalculated cells. So the calc block is intentionally
      // outside the dirty-gate early return below.
      if (!formulaInitDoneRef.current && workbookRef.current) {
        formulaInitDoneRef.current = true;
        const wb = workbookRef.current;
        for (const sheet of wb.getAllSheets()) {
          wb.calculateFormula(sheet.id);
        }
      }

      // Dirty rule: only mark dirty when onOp has fired since the last load
      // or save. Workbook's onChange echoes its internal context on every
      // rebuild (file open, settings change, selection, etc.) and does not
      // by itself indicate user input. onOp, in contrast, fires only for
      // genuine operations (cell edits, format changes, sheet operations)
      // and never for the post-load context rebuild — so we gate dirty on
      // that signal.
      if (!userEditSeenRef.current) {
        // Workbook-internal rebuild echo — refresh the snapshot but don't
        // mark dirty.
        lastLoadedSheetsRef.current = changedSheets;
        if (lastLoadedFingerprintRef.current !== null) {
          lastLoadedFingerprintRef.current = fingerprintSheets(changedSheets);
        }
        return;
      }
      // User-driven change confirmed by onOp. Update the snapshot so future
      // Workbook rebuilds don't re-flip the dirty state.
      lastLoadedSheetsRef.current = changedSheets;
      lastLoadedFingerprintRef.current = fingerprintSheets(changedSheets);
      setIsDirty(true);
    },
    [],
  );

  // ── onOp handler: tracks genuine user-driven operations. Workbook's
  //    onOp fires for cell edits, format changes, sheet ops, undo/redo, and
  //    paste actions — never for the post-load context rebuild. We use it as
  //    the gating signal that allows handleFortuneChange to flip isDirty.
  const handleFortuneOp = useCallback(() => {
    userEditSeenRef.current = true;
  }, []);

  // ── Recalculate all sheets for save ───────────────────────────────────────────
  recalcAllSheetsRef.current = () => {
    const wb = workbookRef.current;
    if (!wb) return;
    for (const sheet of wb.getAllSheets()) {
      wb.calculateFormula(sheet.id);
    }
  };

  // Token counter to abort stale loads. See the matching comment in
  // WordEditor for the full rationale — the same TOCTOU bug exists here:
  // when the user asks AI to modify the spreadsheet back-to-back, two
  // `loadFromDisk` calls race against each other. The slower one reads
  // an *earlier* version and can land last, overwriting the newer
  // sheets data and leaving the editor showing stale content. Bumping
  // this counter on every reload invalidates every in-flight load so
  // only the latest one is allowed to commit.
  const loadTokenRef = useRef(0);
  // Set after the first load completes. Guards the version-watcher
  // effect against firing during the initial paint (where the parent
  // already passed us the right data).
  const hasInitializedRef = useRef(false);

  // Load bytes from disk and push them through every surface that
  // displays the workbook:
  //   - `fortuneSheets` state → the `<Workbook data=...>` reactively reloads;
  //   - `setFortuneSheetsToStore(...)` → mirrors sheets into the editor
  //     store so a later tab switch / file re-open sees the same content;
  // Returns true if the load committed (i.e. wasn't aborted by a newer
  // load); the caller uses the return value to know whether to clear
  // the loading flag.
  const readAndApplySheets = useCallback(
    async (token: number): Promise<boolean> => {
      try {
        const rustWorkbook = await invoke<RustXlsxWorkbook>('read_xlsx_structured', { path: filePath });
        if (loadTokenRef.current !== token) return false;
        const sheets = rustWorkbookToFortuneSheets(rustWorkbook);
        loadedSheetsRef.current = sheets;
        setFortuneSheets(sheets);
        setFortuneSheetsToStore(filePath, { sheets });
        // Reset the user-edit gate so the post-load Workbook onChange
        // echo doesn't flip isDirty. See handleFortuneChange for the
        // full rationale.
        lastLoadedSheetsRef.current = sheets;
        lastLoadedFingerprintRef.current = fingerprintSheets(sheets);
        userEditSeenRef.current = false;
        setIsDirty(false);
        setOpenTabDirty(filePath, false);
        return loadTokenRef.current === token;
      } catch (err) {
        if (loadTokenRef.current !== token) return false;
        const message = reportError('office-excel-reload', err);
        setError(message);
        pushNotification({
          kind: 'error',
          title: '刷新 Excel 文档失败',
          message,
        });
        return false;
      }
    },
    [filePath, setFortuneSheetsToStore, setOpenTabDirty, pushNotification]
  );

  // Bump the load token and re-read the workbook. Used by every code
  // path that wants the editor to repaint with fresh bytes — including
  // the initial disk load and the AI-driven version watcher.
  const loadFromDisk = useCallback(async () => {
    const token = ++loadTokenRef.current;
    setLoading(true);
    setError(null);
    try {
      await readAndApplySheets(token);
    } finally {
      if (loadTokenRef.current === token) {
        setLoading(false);
      }
    }
  }, [readAndApplySheets]);

  // Persist the load function (kept for any legacy callers that grab
  // the ref instead of going through the version-watcher).
  const loadFromDiskRef = useRef(loadFromDisk);
  loadFromDiskRef.current = loadFromDisk;

  // Initial load: only run once per mounted editor instance. Re-runs
  // when `filePath` changes (tab switching), which is the only case we
  // actually want to reload from disk.
  useEffect(() => {
    hasInitializedRef.current = false;
    loadTokenRef.current++; // invalidate any in-flight load from the previous path
    void loadFromDisk().then(() => {
      hasInitializedRef.current = true;
    });
  }, [filePath, loadFromDisk]);

  // Re-read when external file version changes. Driven by the AI
  // pipeline (`invalidateOfficeBuffer` from streamEventHandlers.ts).
  //
  // The previous version mixed `hasLoadedRef = false` into this effect,
  // trying to piggy-back on the initial-load effect — but that effect
  // depends on `[filePath]`, which doesn't change here, so the
  // `hasLoadedRef = false` write was a no-op and the version bump
  // only worked via the explicit `loadFromDiskRef.current()` call below.
  // We now read the bytes through the same token-gated path the Word
  // editor uses, so concurrent AI writes can't race against each other
  // and leave the editor showing stale content.
  useEffect(() => {
    // Skip the first paint — the initial-load effect above already
    // handled that. After that, every increment of `officeBufferVersion`
    // (driven by `invalidateOfficeBuffer` from the AI pipeline) is a
    // signal to re-read the file.
    if (officeBufferVersion === 0) return;
    if (!hasInitializedRef.current) return;
    void loadFromDisk();
  }, [officeBufferVersion, loadFromDisk]);

  // Sync dirty state to sidebar tab
  useEffect(() => {
    if (isActive && isDirty) {
      setOpenTabDirty(filePath, true);
    }
  }, [isActive, isDirty, filePath, setOpenTabDirty]);

  // ── Save ────────────────────────────────────────────────────────────────
  const handleSave = useCallback(async () => {
    const wb = workbookRef.current;
    if (!wb) {
      pushNotification({ kind: 'error', title: '保存失败', message: '表格引擎未就绪' });
      return;
    }
    const sheets = wb.getAllSheets();
    if (!sheets?.length) {
      pushNotification({ kind: 'error', title: '保存失败', message: '没有可用的工作表数据' });
      return;
    }
    try {
      recalcAllSheetsRef.current();
      const latestSheets = wb.getAllSheets();
      const buffer = await fortuneSheetsToSheetJSBuffer(latestSheets, wb.dataToCelldata.bind(wb));
      const bufferArray = Array.from(buffer);
      await invoke('write_office_file', { path: filePath, data: bufferArray });

      loadedSheetsRef.current = latestSheets;
      setFortuneSheetsToStore(filePath, { sheets: latestSheets });
      // Reset the dirty filter snapshot and user-edit gate to the just-saved
      // state. Future echoes from the Workbook are recognized as non-user-
      // driven until onOp fires again.
      lastLoadedSheetsRef.current = latestSheets;
      lastLoadedFingerprintRef.current = fingerprintSheets(latestSheets);
      userEditSeenRef.current = false;
      setIsDirty(false);
      setOpenTabDirty(filePath, false);
    } catch (err) {
      const message = reportError('office-excel-save', err);
      pushNotification({ kind: 'error', title: '保存 Excel 文档失败', message });
    }
  }, [filePath, setFortuneSheetsToStore, setOpenTabDirty, pushNotification]);

  useKeyboardSave({ onSave: handleSave, enabled: isDirty && isActive });

  // ── Render ───────────────────────────────────────────────────────────────
  return (
    <div className={styles.officeEditor}>
      <OfficeToolbar
        fileName={fileName}
        isDirty={isDirty}
        onSave={handleSave}
        canSave={isDirty && !loading && !error}
        formatIcon={<Table2 size={16} />}
        editLabel={loading ? '加载中...' : error ? '加载失败' : '可编辑'}
      />
      <div className={styles.excelContainer}>
        {(loading || error) ? (
          <div className={styles.editorOverlay} role="status" aria-live="polite">
            {loading ? (
              <>
                <div className={styles.loadingSpinner} />
                <span>正在加载 Excel 文档...</span>
              </>
            ) : (
              <span className={styles.editorErrorMessage}>加载失败: {error}</span>
            )}
          </div>
        ) : (
          <Workbook
            data={fortuneSheets}
            onChange={handleFortuneChange}
            onOp={handleFortuneOp}
            ref={workbookRef}
            showToolbar={true}
            showFormulaBar={true}
            showSheetTabs={true}
            allowEdit={true}
            forceCalculation={true}
            hooks={hooks}
          />
        )}
      </div>
    </div>
  );
};
