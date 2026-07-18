use std::time::Duration;

use tui_tetris::engine::fixed_step::FixedStepClock;

#[test]
fn fixed_step_clock_retains_fractional_elapsed_time() {
    let mut clock = FixedStepClock::new(Duration::from_millis(16), 8);

    assert_eq!(clock.advance(Duration::from_millis(15)), 0);
    assert_eq!(clock.advance(Duration::from_millis(1)), 1);
    assert_eq!(clock.advance(Duration::from_millis(8)), 0);
    assert_eq!(clock.advance(Duration::from_millis(8)), 1);
}

#[test]
fn fixed_step_clock_caps_bursts_without_discarding_backlog() {
    let mut clock = FixedStepClock::new(Duration::from_millis(16), 8);

    assert_eq!(clock.advance(Duration::from_millis(16 * 20)), 8);
    assert_eq!(clock.advance(Duration::ZERO), 8);
    assert_eq!(clock.advance(Duration::ZERO), 4);
    assert_eq!(clock.backlog(), Duration::ZERO);
}

#[test]
fn fixed_step_clock_reports_time_until_next_step() {
    let mut clock = FixedStepClock::new(Duration::from_millis(16), 8);
    assert_eq!(clock.until_next_step(), Duration::from_millis(16));
    assert_eq!(clock.advance(Duration::from_millis(6)), 0);
    assert_eq!(clock.until_next_step(), Duration::from_millis(10));
}
