// Quiet inline status line rendered above the textarea when the
// composer is collapsed.
//
// Shows which toggles are on as plain text with bullet separators
// (no colored chips, no extra noise). Returns null (no DOM, no
// height) when no toggles are on, so the composer's collapsed
// footprint shrinks as much as possible.
//
// Data flows from `TOGGLES` (same registry `<ComposerToggleRows>`
// reads) so a new toggle automatically appears here too.

import React from 'react';

import type { FeatureToggleMap } from '../../../types';
import { TOGGLES } from './toggles';

import styles from '../AIPanelInput.module.css';

interface ActiveToggleStripProps {
  featureToggles: FeatureToggleMap | undefined;
}

export const ActiveToggleStrip: React.FC<ActiveToggleStripProps> = ({
  featureToggles,
}) => {
  const active = TOGGLES.filter((spec) => featureToggles?.[spec.id]);
  if (active.length === 0) return null;
  return (
    <div className={styles.composerHeader}>
      <span
        className={styles.activeBadges}
        aria-label={`${active.length} 个功能已启用`}
      >
        {active.map((spec, idx) => (
          <React.Fragment key={spec.id}>
            {idx > 0 && (
              <span className={styles.activeBadgeDot} aria-hidden>
                ·
              </span>
            )}
            <span className={styles.activeBadge}>
              {spec.icon}
              <span>{spec.label}</span>
            </span>
          </React.Fragment>
        ))}
      </span>
    </div>
  );
};