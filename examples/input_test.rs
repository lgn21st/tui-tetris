//! Simple input test - runs without TUI to diagnose input issues.

use std::io;
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode},
};

fn main() -> io::Result<()> {
    println!("Tetris Input Diagnostic Test");
    println!("==============================");
    println!("This will run for 10 seconds and log all key events.");
    println!("Press arrow keys, WASD, or 'q' to quit early.");
    println!();

    // Open log file
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/tetris_input_test.log")?;

    use std::io::Write;
    writeln!(log, "=== Input Test Started at {:?} ===", Instant::now())?;
    log.flush()?;

    // Enable raw mode
    enable_raw_mode()?;

    let start = Instant::now();
    let mut event_count = 0;
    let mut press_count = 0;
    let mut repeat_count = 0;
    let mut release_count = 0;

    loop {
        // Poll for events with 100ms timeout
        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
        {
            event_count += 1;
            let timestamp = start.elapsed().as_millis();

            match key.kind {
                KeyEventKind::Press => {
                    press_count += 1;
                    writeln!(log, "[{:>6}ms] PRESS   {:?}", timestamp, key.code)?;

                    if key.code == KeyCode::Char('q') {
                        writeln!(log, "Quit requested")?;
                        break;
                    }
                }
                KeyEventKind::Repeat => {
                    repeat_count += 1;
                    writeln!(log, "[{:>6}ms] REPEAT  {:?}", timestamp, key.code)?;
                }
                KeyEventKind::Release => {
                    release_count += 1;
                    writeln!(log, "[{:>6}ms] RELEASE {:?}", timestamp, key.code)?;
                }
            }
            log.flush()?;
        }

        // Stop after 10 seconds
        if start.elapsed() >= Duration::from_secs(10) {
            writeln!(log, "Test completed (10 second timeout)")?;
            break;
        }
    }

    // Restore terminal
    disable_raw_mode()?;

    // Write summary
    writeln!(log)?;
    writeln!(log, "=== Test Summary ===")?;
    writeln!(log, "Total events: {}", event_count)?;
    writeln!(log, "Press events: {}", press_count)?;
    writeln!(log, "Repeat events: {}", repeat_count)?;
    writeln!(log, "Release events: {}", release_count)?;
    writeln!(log, "Log file: /tmp/tetris_input_test.log")?;
    log.flush()?;

    // Print summary to console
    println!();
    println!("Test complete!");
    println!("Total events: {}", event_count);
    println!("Press events: {}", press_count);
    println!("Repeat events: {}", repeat_count);
    println!("Release events: {}", release_count);
    println!();
    println!("Log file: /tmp/tetris_input_test.log");
    println!();

    // Analyze and report findings
    if repeat_count > 0 {
        println!("⚠️  WARNING: Your terminal is generating REPEAT events!");
        println!("   This can cause input issues.");
        println!();
    }

    if press_count > 0 && release_count == 0 {
        println!("⚠️  WARNING: No RELEASE events detected!");
        println!("   Your terminal may not support key release events.");
        println!();
    }

    println!("Please check the log file for detailed event timing.");
    println!();

    Ok(())
}
