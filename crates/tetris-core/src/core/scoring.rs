//! Scoring module - Guideline DS/Friends scoring
//!
//! Compatibility note:
//! Portable scoring is defined by `protocol/rules/SPEC.md`.
//! In particular:
//! - T-Spin scoring uses the T-Spin tables (it does not add line-clear points).
//! - B2B applies a 3/2 multiplier to the base clear points (before combo bonus).
//! - Combo bonus is `combo_base * combo_index * guideline_level`.
//! - Mini T-Spin with lines is difficult (B2B-capable).

use crate::types::{
    B2B_DENOMINATOR, B2B_NUMERATOR, COMBO_BASE, DROP_INTERVAL_FLOOR_MS, DROP_INTERVALS,
    LINE_SCORES, TSpinKind,
};

/// Score calculation result
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct ScoreResult {
    /// Base points for the clear (includes B2B multiplier, excludes combo bonus).
    pub line_clear_score: u32,
    /// Combo bonus added on top of `line_clear_score`.
    pub combo_bonus: u32,
    pub total: u32,
    pub qualifies_for_b2b: bool,
    /// Whether a B2B multiplier was applied to this clear.
    pub b2b_applied: bool,
}

/// Calculate line clear score (Classic rules)
/// lines: number of lines cleared (1-4)
/// level: current level (0-based)
pub(crate) fn calculate_line_score(lines: usize, level: u32) -> u32 {
    if lines == 0 || lines > 4 {
        return 0;
    }
    let base_score = LINE_SCORES[lines];
    base_score.saturating_mul(level.saturating_add(1))
}

/// Calculate T-spin score (Modern rules)
pub(crate) fn calculate_tspin_score(tspin: TSpinKind, lines: usize, level: u32) -> u32 {
    let level_multiplier = level.saturating_add(1);
    match (tspin, lines) {
        (TSpinKind::Full, 0) => 400u32.saturating_mul(level_multiplier),
        (TSpinKind::Full, 1) => 800u32.saturating_mul(level_multiplier),
        (TSpinKind::Full, 2) => 1200u32.saturating_mul(level_multiplier),
        (TSpinKind::Full, 3) => 1600u32.saturating_mul(level_multiplier),
        (TSpinKind::Mini, 0) => 100u32.saturating_mul(level_multiplier),
        (TSpinKind::Mini, 1) => 200u32.saturating_mul(level_multiplier),
        (TSpinKind::Mini, 2) => 400u32.saturating_mul(level_multiplier),
        _ => 0,
    }
}

/// Calculate combo bonus (Guideline DS/Friends).
///
/// `combo_index` semantics:
/// - `-1`: no combo chain
/// - `0`: first clear in chain (no bonus)
/// - `1+`: bonus applies as `combo_base * combo_index * guideline_level`
pub(crate) fn calculate_combo_bonus(combo_index: i32, level: u32) -> u32 {
    if combo_index <= 0 {
        return 0;
    }
    COMBO_BASE
        .saturating_mul(combo_index as u32)
        .saturating_mul(level.saturating_add(1))
}

/// Check if this clear qualifies for back-to-back.
/// B2B applies to: T-spin full or mini with lines, or Tetris (4 lines).
pub(crate) fn qualifies_for_b2b(tspin: TSpinKind, lines: usize) -> bool {
    matches!(
        (tspin, lines),
        (TSpinKind::Full, 1..=4) | (TSpinKind::Mini, 1..=4) | (TSpinKind::None, 4)
    )
}

pub(crate) fn apply_b2b_multiplier(points: u32) -> u32 {
    points
        .saturating_mul(B2B_NUMERATOR)
        .saturating_div(B2B_DENOMINATOR)
}

/// Calculate complete score for a line clear (modern ruleset behavior).
///
/// Notes:
/// - T-Spin uses its table score instead of the classic line-clear score.
/// - B2B applies a multiplier to the base clear points.
/// - Combo bonus is added after the base clear points.
pub(crate) fn calculate_score(
    lines: usize,
    level: u32,
    tspin: TSpinKind,
    combo_index: i32,
    previous_b2b: bool,
) -> ScoreResult {
    let qualifies_b2b = qualifies_for_b2b(tspin, lines);

    let base_points = match tspin {
        TSpinKind::Full | TSpinKind::Mini => calculate_tspin_score(tspin, lines, level),
        TSpinKind::None => calculate_line_score(lines, level),
    };

    let b2b_applied = qualifies_b2b && previous_b2b;
    let line_clear_score = if b2b_applied {
        apply_b2b_multiplier(base_points)
    } else {
        base_points
    };

    let combo_bonus = calculate_combo_bonus(combo_index, level);
    let total = line_clear_score.saturating_add(combo_bonus);

    ScoreResult {
        line_clear_score,
        combo_bonus,
        total,
        qualifies_for_b2b: qualifies_b2b,
        b2b_applied,
    }
}

/// Calculate drop score
/// soft_drop: +1 per cell
/// hard_drop: +2 per cell
pub(crate) fn calculate_drop_score(cells: u32, is_hard_drop: bool) -> u32 {
    if is_hard_drop {
        cells.saturating_mul(2)
    } else {
        cells
    }
}

/// Level management
/// Level increases every 10 lines cleared
pub(crate) fn calculate_level(total_lines: u32) -> u32 {
    total_lines / 10
}

/// Get drop interval for a level (in milliseconds)
/// Returns interval based on level, clamped at minimum
pub(crate) fn get_drop_interval_ms(level: u32) -> u32 {
    DROP_INTERVALS
        .get(level as usize)
        .copied()
        .unwrap_or(DROP_INTERVAL_FLOOR_MS)
}

/// Calculate soft drop interval
/// Base interval divided by soft drop multiplier
#[cfg(test)]
pub(crate) fn get_soft_drop_interval_ms(base_interval: u32, multiplier: u32) -> u32 {
    let interval = base_interval / multiplier;
    interval.max(1) // Minimum 1ms to avoid division issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TSpinKind;

    #[test]
    fn test_classic_line_scores() {
        // Level 0 (guideline level 1)
        assert_eq!(calculate_line_score(1, 0), 100);
        assert_eq!(calculate_line_score(2, 0), 300);
        assert_eq!(calculate_line_score(3, 0), 500);
        assert_eq!(calculate_line_score(4, 0), 800);

        // Level 5 (guideline level 6)
        assert_eq!(calculate_line_score(1, 5), 100 * 6);
        assert_eq!(calculate_line_score(4, 5), 800 * 6);
    }

    #[test]
    fn test_tspin_scores() {
        // Level 0
        assert_eq!(calculate_tspin_score(TSpinKind::Full, 0, 0), 400);
        assert_eq!(calculate_tspin_score(TSpinKind::Full, 1, 0), 800);
        assert_eq!(calculate_tspin_score(TSpinKind::Full, 2, 0), 1200);
        assert_eq!(calculate_tspin_score(TSpinKind::Full, 3, 0), 1600);

        assert_eq!(calculate_tspin_score(TSpinKind::Mini, 0, 0), 100);
        assert_eq!(calculate_tspin_score(TSpinKind::Mini, 1, 0), 200);
        assert_eq!(calculate_tspin_score(TSpinKind::Mini, 2, 0), 400);

        // Level 2
        assert_eq!(calculate_tspin_score(TSpinKind::Full, 1, 2), 800 * 3);
        assert_eq!(calculate_tspin_score(TSpinKind::Mini, 2, 2), 400 * 3);
    }

    #[test]
    fn test_combo_bonus() {
        assert_eq!(calculate_combo_bonus(-1, 0), 0);
        assert_eq!(calculate_combo_bonus(0, 0), 0);
        assert_eq!(calculate_combo_bonus(1, 0), 50);
        assert_eq!(calculate_combo_bonus(3, 0), 150);
        assert_eq!(calculate_combo_bonus(3, 5), 50 * 3 * 6);
    }

    #[test]
    fn test_b2b_qualification() {
        // T-spin full with lines qualifies
        assert!(qualifies_for_b2b(TSpinKind::Full, 1));
        assert!(qualifies_for_b2b(TSpinKind::Full, 4));

        // Tetris qualifies
        assert!(qualifies_for_b2b(TSpinKind::None, 4));

        // T-spin mini with lines qualifies
        assert!(qualifies_for_b2b(TSpinKind::Mini, 1));
        assert!(qualifies_for_b2b(TSpinKind::Mini, 2));

        // Zero-line T-spins do not qualify (they also do not break a live chain)
        assert!(!qualifies_for_b2b(TSpinKind::Full, 0));
        assert!(!qualifies_for_b2b(TSpinKind::Mini, 0));

        // Regular clears do not qualify
        assert!(!qualifies_for_b2b(TSpinKind::None, 1));
        assert!(!qualifies_for_b2b(TSpinKind::None, 3));
    }

    #[test]
    fn test_b2b_multiplier() {
        assert_eq!(apply_b2b_multiplier(0), 0);
        assert_eq!(apply_b2b_multiplier(1200), 1800);
    }

    #[test]
    fn test_full_score_calculation() {
        // T-spin full single uses table (no classic add)
        let result = calculate_score(1, 0, TSpinKind::Full, 0, false);
        assert_eq!(result.line_clear_score, 800);
        assert_eq!(result.combo_bonus, 0);
        assert_eq!(result.total, 800);

        // Second consecutive clear has combo bonus (combo_index = 1).
        let result = calculate_score(1, 0, TSpinKind::None, 1, false);
        assert_eq!(result.line_clear_score, 100);
        assert_eq!(result.combo_bonus, 50);
        assert_eq!(result.total, 150);

        // Back-to-back Tetris multiplies the base clear points only.
        let result = calculate_score(4, 0, TSpinKind::None, 1, true);
        assert_eq!(result.line_clear_score, 1200);
        assert_eq!(result.combo_bonus, 50);
        assert_eq!(result.total, 1250);
        assert!(result.qualifies_for_b2b);
        assert!(result.b2b_applied);
    }

    #[test]
    fn test_mini_tspin_with_lines_gets_b2b_multiplier() {
        let result = calculate_score(1, 0, TSpinKind::Mini, 0, true);
        assert_eq!(result.line_clear_score, 300);
        assert_eq!(result.combo_bonus, 0);
        assert_eq!(result.total, 300);
        assert!(result.qualifies_for_b2b);
        assert!(result.b2b_applied);

        let result = calculate_score(2, 3, TSpinKind::Mini, 0, true);
        assert_eq!(result.line_clear_score, 400 * 4 * 3 / 2);
        assert_eq!(result.combo_bonus, 0);
        assert_eq!(result.total, 400 * 4 * 3 / 2);
        assert!(result.qualifies_for_b2b);
        assert!(result.b2b_applied);

        let result = calculate_score(1, 0, TSpinKind::Mini, 2, true);
        assert_eq!(result.line_clear_score, 300);
        assert_eq!(result.combo_bonus, 100);
        assert_eq!(result.total, 400);
        assert!(result.qualifies_for_b2b);
        assert!(result.b2b_applied);
    }

    #[test]
    fn test_non_qualifying_clear_breaks_b2b_multiplier_application() {
        // Even if the previous clear qualified (previous_b2b=true), a normal single clear
        // must not receive the B2B multiplier.
        let result = calculate_score(1, 0, TSpinKind::None, 0, true);
        assert_eq!(result.line_clear_score, 100);
        assert_eq!(result.combo_bonus, 0);
        assert_eq!(result.total, 100);
        assert!(!result.qualifies_for_b2b);
        assert!(!result.b2b_applied);
    }

    #[test]
    fn test_full_tspin_single_gets_b2b_multiplier_when_chain_active() {
        let result = calculate_score(1, 0, TSpinKind::Full, 0, true);
        assert_eq!(result.line_clear_score, 1200); // 800 * 3/2
        assert_eq!(result.combo_bonus, 0);
        assert_eq!(result.total, 1200);
        assert!(result.qualifies_for_b2b);
        assert!(result.b2b_applied);
    }

    #[test]
    fn test_full_tspin_no_lines_does_not_qualify_for_b2b() {
        let result = calculate_score(0, 0, TSpinKind::Full, 0, true);
        assert_eq!(result.line_clear_score, 400);
        assert_eq!(result.combo_bonus, 0);
        assert_eq!(result.total, 400);
        assert!(!result.qualifies_for_b2b);
        assert!(!result.b2b_applied);
    }

    #[test]
    fn test_combo_bonus_scales_with_guideline_level() {
        let result = calculate_score(2, 5, TSpinKind::None, 3, false);
        assert_eq!(result.line_clear_score, 300 * 6);
        assert_eq!(result.combo_bonus, 50 * 3 * 6);
        assert_eq!(result.total, 300 * 6 + 50 * 3 * 6);
    }

    #[test]
    fn test_drop_scores() {
        assert_eq!(calculate_drop_score(10, false), 10); // Soft drop 10 cells
        assert_eq!(calculate_drop_score(10, true), 20); // Hard drop 10 cells
    }

    #[test]
    fn score_helpers_saturate_instead_of_overflowing() {
        assert_eq!(calculate_line_score(4, u32::MAX), u32::MAX);
        assert_eq!(
            calculate_tspin_score(TSpinKind::Full, 3, u32::MAX),
            u32::MAX
        );
        assert_eq!(calculate_drop_score(u32::MAX, true), u32::MAX);
        assert_eq!(calculate_combo_bonus(i32::MAX, 0), u32::MAX);
        assert_eq!(calculate_combo_bonus(i32::MAX, u32::MAX), u32::MAX);
    }

    #[test]
    fn test_level_calculation() {
        assert_eq!(calculate_level(0), 0);
        assert_eq!(calculate_level(9), 0);
        assert_eq!(calculate_level(10), 1);
        assert_eq!(calculate_level(29), 2);
        assert_eq!(calculate_level(100), 10);
    }

    #[test]
    fn test_drop_intervals() {
        assert_eq!(get_drop_interval_ms(0), 1000);
        assert_eq!(get_drop_interval_ms(8), 160);
        assert_eq!(get_drop_interval_ms(9), 120);
        assert_eq!(get_drop_interval_ms(20), 120); // Floor at 120
        assert_eq!(get_drop_interval_ms(8), DROP_INTERVALS[8]);
        assert_eq!(get_drop_interval_ms(9), DROP_INTERVAL_FLOOR_MS);
    }

    #[test]
    fn test_soft_drop_interval() {
        assert_eq!(get_soft_drop_interval_ms(1000, 10), 100);
        assert_eq!(get_soft_drop_interval_ms(100, 10), 10);
        assert_eq!(get_soft_drop_interval_ms(5, 10), 1); // Minimum 1ms
    }
}
