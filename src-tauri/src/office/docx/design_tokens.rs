//! Design system for Word document generation.
//!
//! This module encodes the visual language used across all inkuo-generated
//! Word documents. It is the Rust analogue of the Python project's
//! "设计规则" (design rules) — by centralising every colour / font size
//! / spacing value in one place, we get a single source of truth that
//! flows through every component builder, so a brand refresh touches one
//! file instead of fifty.
//!
//! ## Three-layer mapping
//!
//! The user-facing pipeline looks like:
//!
//! ```text
//! AI decision layer   (DocElement / ContentBlock JSON)
//!        ↓
//! Design tokens       (this module — pure data)
//!        ↓
//! Component renderer  (super::components — pure XML)
//!        ↓
//! OOXML emission      (writer.rs)
//! ```
//!
//! Each layer only depends on the layer above. The design tokens know
//! nothing about XML; the components know nothing about AI inputs.
//!
//! ## Hex conventions
//!
//! All colours are 6-char RGB strings with no leading `#` — this matches
//! the way Word expects them inside `<w:color w:val="..."/>` and
//! `<w:shd w:fill="..."/>`. Use [`palette`] to look up named tokens
//! rather than hand-typing hex strings in component code.

use serde::{Deserialize, Serialize};

/// Master design palette. Hex strings, no leading `#`.
///
/// Strings are owned (rather than `&'static str`) so the struct
/// can derive `Deserialize` for callers that want to read palettes
/// from configuration files. This costs a tiny allocation per
/// palette but the palette is shared across every emitted paragraph
/// so we still pay the price once, not per-paragraph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Palette {
    /// Deep green — primary brand colour. Used for top-level chapter
    /// titles, accent borders, and the table-header background.
    pub primary: String,
    /// Medium green — secondary headings (H2 / H3 in the chapter
    /// hierarchy). Lighter than `primary` so the heading levels form a
    /// visible "ladder".
    pub secondary: String,
    /// Warm gold-brown — used sparingly for emphasis (key terms,
    /// inline code callouts, "important" badges). Acts as the
    /// complement to `primary` and keeps the green palette from
    /// feeling monotone.
    pub accent: String,
    /// Light grey-green — zebra-stripe background for table body rows.
    /// Pale enough that black text reads cleanly on top.
    pub zebra: String,
    /// Pale green — background for informational callouts.
    /// Same hue family as `primary` so callouts feel related to
    /// chapter content rather than bolted-on.
    pub callout_info_bg: String,
    /// Pale amber — background for warning callouts.
    pub callout_warning_bg: String,
    /// Pale rose — background for important / danger callouts.
    pub callout_important_bg: String,
    /// Pale teal — background for tip callouts.
    pub callout_tip_bg: String,
    /// Off-white grey — code-block background. Slightly warmer than
    /// pure grey so blocks don't feel sterile against the page.
    pub code_bg: String,
    /// Near-black body text colour. We deliberately avoid pure black
    /// (`000000`) — `#2A2A2A` reads softer and is the convention in
    /// modern long-form editorial design.
    pub text: String,
    /// Subdued grey — captions, footnotes, table-of-contents entries.
    pub text_muted: String,
    /// Pure white. Used in the table header where text sits on the
    /// dark green `primary` background.
    pub text_on_primary: String,
}

/// Default inkuo palette. Inspired by long-form Chinese technical
/// reports (think "in-house design doc" rather than "marketing
/// brochure"). Single source of truth for every component builder.
///
/// Built at runtime (rather than as a `const`) so the colour strings
/// can be `String`s and the palette can derive `Deserialize`. The
/// function is `const fn`-compatible where possible, but the `const`
/// keyword is dropped because `String::from` is not stable in
/// `const` contexts.
pub fn default_palette() -> Palette {
    Palette {
        primary: String::from("213B32"),
        secondary: String::from("2E7D5B"),
        accent: String::from("B8893E"),
        zebra: String::from("EAF0EC"),
        callout_info_bg: String::from("E8F1ED"),
        callout_warning_bg: String::from("FBF1DC"),
        callout_important_bg: String::from("F8E2DD"),
        callout_tip_bg: String::from("E5F1EE"),
        code_bg: String::from("F4F1EC"),
        text: String::from("2A2A2A"),
        text_muted: String::from("6E6E6E"),
        text_on_primary: String::from("FFFFFF"),
    }
}

/// Backwards-compatible alias for callers that expect a `const`. We
/// make this a `static` that returns the same value as
/// [`default_palette`] the first time it's touched, so the
/// `DEFAULT_PALETTE` name still resolves at the call sites we wrote
/// in v1. New code should use [`default_palette`].
pub static DEFAULT_PALETTE: once_cell::sync::Lazy<Palette> =
    once_cell::sync::Lazy::new(default_palette);

/// Font sizing convention. All sizes are in half-points (Word's
/// internal unit; 24 = 12 pt). We use a single struct so every
/// component reaches into the same hierarchy rather than redefining
/// sizes inline — a designer can re-tune `body_pt` and every
/// dependent field recalibrates at once.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FontScale {
    /// Cover title — the first thing readers see.
    pub cover_title_pt: u32,
    /// H1 / chapter heading. Sized so it doesn't compete with the
    /// cover but still announces a new section.
    pub h1_pt: u32,
    /// H2 / subsection heading.
    pub h2_pt: u32,
    /// H3 / minor heading.
    pub h3_pt: u32,
    /// Body text — the workhorse size.
    pub body_pt: u32,
    /// Body emphasised (callout titles, inline labels).
    pub body_strong_pt: u32,
    /// Caption / footnote size.
    pub caption_pt: u32,
    /// Header / footer text.
    pub header_footer_pt: u32,
    /// Table body text. Slightly smaller than body so multi-line
    /// tables don't dominate the page.
    pub table_body_pt: u32,
    /// Table header text. Matches `table_body_pt` because we
    /// rely on weight + colour to differentiate headers, not size.
    pub table_header_pt: u32,
}

impl Default for FontScale {
    fn default() -> Self {
        // Default ladder matches the conventions described in the
        // design-doc table the user shared (10 pt body, 34 pt cover,
        // etc.). Half-points here, so 20 = 10 pt.
        Self {
            cover_title_pt: 68,        // 34 pt
            h1_pt: 40,                 // 20 pt
            h2_pt: 28,                 // 14 pt
            h3_pt: 23,                 // 11.5 pt
            body_pt: 20,               // 10 pt
            body_strong_pt: 22,        // 11 pt
            caption_pt: 18,            // 9 pt
            header_footer_pt: 16,      // 8 pt
            table_body_pt: 17,         // 8.5 pt
            table_header_pt: 17,       // 8.5 pt
        }
    }
}

/// Spatial conventions. Most values are in twentieths of a point
/// (twips — Word's spacing unit) and are inlined into `<w:spacing>`
/// and `<w:tcMar>` blocks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Spacing {
    /// Body line spacing as a multiplier multiplied by 240 (Word's
    /// `<w:spacing w:line="…" w:lineRule="auto"/>` units). 1.22×
    /// reads as the "long-form report" cadence.
    pub body_line: u32,
    /// Heading line spacing. Same multiplier convention.
    pub heading_line: u32,
    /// Space-before for H1 paragraphs (twips). Big enough to mark a
    /// new chapter without leaving a quarter-page hole.
    pub h1_before: u32,
    pub h2_before: u32,
    pub h3_before: u32,
    /// Space-after headings — smaller than space-before so headings
    /// "stick" to the body that follows them.
    pub h1_after: u32,
    pub h2_after: u32,
    pub h3_after: u32,
    /// Space-after body paragraphs.
    pub body_after: u32,
    /// Cell inner padding in twips. Symmetric top/bottom and left/right
    /// — applied via `<w:tcMar>`.
    pub cell_pad_v: u32,
    pub cell_pad_h: u32,
    /// Callout inner padding (twips).
    pub callout_pad: u32,
    /// Code-block inner padding (twips).
    pub code_pad: u32,
}

impl Default for Spacing {
    fn default() -> Self {
        Self {
            body_line: 293,    // ≈ 1.22 × 240
            heading_line: 300, // 1.25
            h1_before: 480,
            h2_before: 360,
            h3_before: 240,
            h1_after: 120,
            h2_after: 80,
            h3_after: 60,
            body_after: 120,
            cell_pad_v: 80,
            cell_pad_h: 108,
            callout_pad: 200,
            code_pad: 240,
        }
    }
}

/// Aggregate design tokens. One struct to pass into every component
/// renderer. Components never read `DEFAULT_PALETTE` directly — they
/// take a [`DesignTokens`] so unit tests can swap in a different
/// palette and verify a brand rebuild.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesignTokens {
    pub palette: Palette,
    pub fonts: FontScale,
    pub spacing: Spacing,
}

impl Default for DesignTokens {
    fn default() -> Self {
        Self {
            palette: default_palette(),
            fonts: FontScale::default(),
            spacing: Spacing::default(),
        }
    }
}

impl DesignTokens {
    /// Resolve a half-point `font_size` for a named semantic slot
    /// (e.g. `"h1"`, `"body"`, `"table_body"`). Returns `None` for
    /// unknown slots so callers can fall back to the style default.
    pub fn font_size_hp(&self, slot: &str) -> Option<u32> {
        match slot {
            "cover_title" => Some(self.fonts.cover_title_pt * 2),
            "h1" => Some(self.fonts.h1_pt),
            "h2" => Some(self.fonts.h2_pt),
            "h3" => Some(self.fonts.h3_pt),
            "body" => Some(self.fonts.body_pt),
            "body_strong" => Some(self.fonts.body_strong_pt),
            "caption" => Some(self.fonts.caption_pt),
            "header_footer" => Some(self.fonts.header_footer_pt),
            "table_body" => Some(self.fonts.table_body_pt),
            "table_header" => Some(self.fonts.table_header_pt),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_palette_matches_design_doc() {
        // These are the values the user shared in the design doc.
        // Locking them in a test catches accidental drift.
        assert_eq!(DEFAULT_PALETTE.primary, "213B32");
        assert_eq!(DEFAULT_PALETTE.secondary, "2E7D5B");
        assert_eq!(DEFAULT_PALETTE.accent, "B8893E");
        assert_eq!(DEFAULT_PALETTE.text, "2A2A2A");
    }

    #[test]
    fn font_size_lookup_is_consistent() {
        let t = DesignTokens::default();
        assert_eq!(t.font_size_hp("h1"), Some(40));
        assert_eq!(t.font_size_hp("body"), Some(20));
        assert!(t.font_size_hp("not_a_real_slot").is_none());
    }
}
