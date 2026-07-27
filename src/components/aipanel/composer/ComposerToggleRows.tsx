// Row UI for each feature toggle inside the composer card.
//
// Data-driven: renders one row per entry in `TOGGLES`. The button
// shows the toggle's icon + label + a switch indicator. The
// `disabled` state is computed in `isToggleDisabled` (see
// `./toggles`) so the rule is reusable.
//
// Exported as a separate component (rather than inlined in
// `<ChatInput>`) because it has its own visual design (the
// `toggleRow` class cluster) and external callers may want to
// preview the "what's on" state without a full composer.

import React from 'react';

import type { FeatureToggleId, FeatureToggleMap } from '../../../types';
import { isToggleDisabled, toggleTooltip, TOGGLES } from './toggles';

import styles from '../AIPanelInput.module.css';

interface ComposerToggleRowsProps {
  sessionId: string | null;
  featureToggles: FeatureToggleMap | undefined;
  disabled?: boolean;
  onToggle: (id: FeatureToggleId, enable: boolean) => void;
}

/** Render the data-driven toolbar that lives inside the composer card.
 * Exported so callers (e.g. chat headers, snapshots) can preview the
 * "what's on" state without needing the full Composer. */
export const ComposerToggleRows: React.FC<ComposerToggleRowsProps> = ({
  sessionId,
  featureToggles,
  disabled,
  onToggle,
}) => {
  return (
    <>
      {TOGGLES.map((spec) => {
        const isDisabled = isToggleDisabled({ sessionId, disabled });
        const enabled = !!featureToggles?.[spec.id];
        return (
          <button
            key={spec.id}
            type="button"
            className={styles.toggleRow}
            data-enabled={enabled}
            data-disabled={isDisabled}
            aria-pressed={enabled}
            aria-disabled={isDisabled}
            disabled={isDisabled}
            title={toggleTooltip(spec, isDisabled)}
            onClick={() => {
              if (isDisabled) return;
              onToggle(spec.id, !enabled);
            }}
          >
            <span className={styles.toggleRowIcon}>{spec.icon}</span>
            <span className={styles.toggleRowLabel}>{spec.label}</span>
            <span className={styles.toggleRowSwitch} data-on={enabled} aria-hidden />
          </button>
        );
      })}
    </>
  );
};