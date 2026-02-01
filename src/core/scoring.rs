//! Scoring module - Classic and Modern Tetris scoring rules
//!
//! Classic rules: 40/100/300/1200 * (level + 1) for 1/2/3/4 lines
//! Modern rules: adds T-spins, B2B, and combo bonuses

use crate::types::{TSpinKind, COMBO_BASE, LINE_SCORES};

/// Score calculation result
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScoreResult {
    pub line_clear_score: u32,
    pub tspin_score: u32,
    pub combo_score: u32,
    pub back_to_back_bonus: u32,
    pub total: u32,
    pub is_back_to_back: bool,
    pub combo_count: u32,
}

/// Calculate line clear score (Classic rules)
/// lines: number of lines cleared (1-4)
/// level: current level (0-based)
pub fn calculate_line_score(lines: usize, level: u32) -> u32 {
    if lines == 0 || lines > 4 {
        return 0;
    }
    let base_score = LINE_SCORES[lines];
    base_score * (level + 1)
}

/// Calculate T-spin score (Modern rules)
pub fn calculate_tspin_score(tspin: TSpinKind, lines: usize, level: u32) -> u32 {
    match (tspin, lines) {
        (TSpinKind::Full, 0) => 400 * (level + 1), // T-spin no lines
        (TSpinKind::Full, 1) => 800 * (level + 1), // T-spin single
        (TSpinKind::Full, 2) => 1200 * (level + 1), // T-spin double
        (TSpinKind::Full, 3) => 1600 * (level + 1), // T-spin triple
        (TSpinKind::Mini, 0) => 100 * (level + 1), // Mini T-spin no lines
        (TSpinKind::Mini, 1) => 200 * (level + 1), // Mini T-spin single
        (TSpinKind::Mini, 2) => 400 * (level + 1), // Mini T-spin double
        _ => 0,
    }
}

/// Calculate combo score
/// combo: consecutive line clears (starts at 0, first combo is 1)
pub fn calculate_combo_score(combo: u32, level: u32) -> u32 {
    if combo == 0 {
        return 0;
    }
    // Combo adds 50 * combo_count * (level + 1)
    COMBO_BASE * combo * (level + 1)
}

/// Check if this clear qualifies for back-to-back
/// B2B applies to: T-spin full with any lines, or Tetris (4 lines)
pub fn qualifies_for_b2b(tspin: TSpinKind, lines: usize) -> bool {
    matches!(
        (tspin, lines),
        (TSpinKind::Full, 1..=4) | // T-spin full with lines
        (TSpinKind::None, 4) // Tetris
    )
}

/// Calculate back-to-back bonus
/// Returns bonus points (3/2 multiplier applied to total score)
pub fn calculate_b2b_bonus(base_score: u32) -> u32 {
    // B2B gives 3/2 = 1.5x bonus
    // So bonus = base_score * 0.5 = base_score / 2
    base_score / 2
}

/// Calculate complete score for a line clear
/// This is the main scoring function that combines all rules
pub fn calculate_score(
    lines: usize,
    level: u32,
    tspin: TSpinKind,
    combo: u32,
    previous_b2b: bool,
) -> ScoreResult {
    // Calculate base scores
    let line_clear_score = calculate_line_score(lines, level);
    let tspin_score = calculate_tspin_score(tspin, lines, level);
    let base_score = line_clear_score + tspin_score;

    // Check if this qualifies for B2B
    let qualifies_b2b = qualifies_for_b2b(tspin, lines);
    let is_back_to_back = qualifies_b2b && previous_b2b;

    // Calculate B2B bonus
    let back_to_back_bonus = if is_back_to_back {
        calculate_b2b_bonus(base_score)
    } else {
        0
    };

    // Calculate combo score
    let combo_score = calculate_combo_score(combo, level);

    // Total score
    let total = base_score + back_to_back_bonus + combo_score;

    ScoreResult {
        line_clear_score,
        tspin_score,
        combo_score,
        back_to_back_bonus,
        total,
        is_back_to_back,
        combo_count: combo,
    }
}

/// Calculate drop score
/// soft_drop: +1 per cell
/// hard_drop: +2 per cell
pub fn calculate_drop_score(cells: u32, is_hard_drop: bool) -> u32 {
    if is_hard_drop {
        cells * 2
    } else {
        cells
    }
}

/// Level management
/// Level increases every 10 lines cleared
pub fn calculate_level(total_lines: u32) -> u32 {
    total_lines / 10
}

/// Get drop interval for a level (in milliseconds)
/// Returns interval based on level, clamped at minimum
pub fn get_drop_interval_ms(level: u32) -> u32 {
    let intervals: [u32; 9] = [1000, 800, 650, 500, 400, 320, 250, 200, 160];

    if (level as usize) < intervals.len() {
        intervals[level as usize]
    } else {
        // After level 9, use 120ms floor
        120
    }
}

/// Calculate soft drop interval
/// Base interval divided by soft drop multiplier
pub fn get_soft_drop_interval_ms(base_interval: u32, multiplier: u32) -> u32 {
    let interval = base_interval / multiplier;
    interval.max(1) // Minimum 1ms to avoid division issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TSpinKind;

    #[test]
    fn test_classic_line_scores() {
        // Level 0
        assert_eq!(calculate_line_score(1, 0), 40);
        assert_eq!(calculate_line_score(2, 0), 100);
        assert_eq!(calculate_line_score(3, 0), 300);
        assert_eq!(calculate_line_score(4, 0), 1200);

        // Level 5
        assert_eq!(calculate_line_score(1, 5), 40 * 6);
        assert_eq!(calculate_line_score(4, 5), 1200 * 6);
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

        // Level 2
        assert_eq!(calculate_tspin_score(TSpinKind::Full, 1, 2), 800 * 3);
    }

    #[test]
    fn test_combo_scores() {
        // No combo
        assert_eq!(calculate_combo_score(0, 0), 0);

        // Combo 1 (first consecutive clear)
        assert_eq!(calculate_combo_score(1, 0), 50);

        // Combo 3
        assert_eq!(calculate_combo_score(3, 0), 150);

        // Combo with level
        assert_eq!(calculate_combo_score(2, 5), 100 * 6);
    }

    #[test]
    fn test_b2b_qualification() {
        // T-spin full with lines qualifies
        assert!(qualifies_for_b2b(TSpinKind::Full, 1));
        assert!(qualifies_for_b2b(TSpinKind::Full, 4));

        // Tetris qualifies
        assert!(qualifies_for_b2b(TSpinKind::None, 4));

        // T-spin mini does not qualify
        assert!(!qualifies_for_b2b(TSpinKind::Mini, 1));

        // Regular clears do not qualify
        assert!(!qualifies_for_b2b(TSpinKind::None, 1));
        assert!(!qualifies_for_b2b(TSpinKind::None, 3));
    }

    #[test]
    fn test_b2b_bonus() {
        let base = 1000;
        let bonus = calculate_b2b_bonus(base);
        assert_eq!(bonus, 500); // Half of base score
    }

    #[test]
    fn test_full_score_calculation() {
        // Regular Tetris, no combo, no B2B
        let result = calculate_score(4, 0, TSpinKind::None, 0, false);
        assert_eq!(result.line_clear_score, 1200);
        assert_eq!(result.total, 1200);
        assert!(!result.is_back_to_back);

        // T-spin double with combo
        // Line clear: 100, T-spin: 1200, Combo 2: 100 = 1400
        let result = calculate_score(2, 0, TSpinKind::Full, 2, false);
        assert_eq!(result.line_clear_score, 100);
        assert_eq!(result.tspin_score, 1200);
        assert_eq!(result.combo_score, 100);
        assert_eq!(result.total, 1400);

        // Back-to-back Tetris
        let result = calculate_score(4, 0, TSpinKind::None, 0, true);
        assert_eq!(result.line_clear_score, 1200);
        assert_eq!(result.back_to_back_bonus, 600);
        assert_eq!(result.total, 1800);
        assert!(result.is_back_to_back);
    }

    #[test]
    fn test_drop_scores() {
        assert_eq!(calculate_drop_score(10, false), 10); // Soft drop 10 cells
        assert_eq!(calculate_drop_score(10, true), 20); // Hard drop 10 cells
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
    }

    #[test]
    fn test_soft_drop_interval() {
        assert_eq!(get_soft_drop_interval_ms(1000, 10), 100);
        assert_eq!(get_soft_drop_interval_ms(100, 10), 10);
        assert_eq!(get_soft_drop_interval_ms(5, 10), 1); // Minimum 1ms
    }
}
