//! Decide whether and how the Kitty graphics backend may be used.
//!
//! The playfield is pixel-only, so this is strict: a terminal is either known
//! to implement the protocol (kitty, Ghostty, WezTerm, Rio) or explicitly
//! forced with `TUI_TETRIS_GRAPHICS=kitty`. Everything else reports
//! [`GraphicsChoice::Off`] so the caller can fail loudly instead of showing an
//! empty well.

/// How terminal graphics commands must be framed for this terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KittySupport {
    /// Wrap every graphics escape in a tmux `DCS tmux;` passthrough envelope.
    pub tmux_passthrough: bool,
}

/// Resolved graphics backend for the current terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GraphicsChoice {
    /// Terminal cells only.
    #[default]
    Off,
    Kitty(KittySupport),
}

impl GraphicsChoice {
    pub fn is_kitty(self) -> bool {
        matches!(self, Self::Kitty(_))
    }

    pub fn support(self) -> Option<KittySupport> {
        match self {
            Self::Kitty(support) => Some(support),
            Self::Off => None,
        }
    }
}

/// Resolve the graphics backend from an environment accessor.
///
/// `env` returns `None` for unset variables; passing a closure keeps detection
/// pure and testable without touching the process environment.
pub fn detect(env: &impl Fn(&str) -> Option<String>) -> GraphicsChoice {
    let term = read(env, "TERM").to_ascii_lowercase();
    let term_program = read(env, "TERM_PROGRAM");

    let in_multiplexer = env("TMUX").is_some()
        || env("STY").is_some()
        || term.starts_with("tmux")
        || term.starts_with("screen");

    match read(env, "TUI_TETRIS_GRAPHICS")
        .to_ascii_lowercase()
        .as_str()
    {
        "off" | "none" | "never" | "0" | "false" => return GraphicsChoice::Off,
        "kitty" | "on" | "force" | "1" | "true" => {
            return GraphicsChoice::Kitty(KittySupport {
                tmux_passthrough: in_multiplexer,
            });
        }
        _ => {}
    }

    if !terminal_implements_kitty_graphics(env, &term, &term_program) {
        return GraphicsChoice::Off;
    }
    GraphicsChoice::Kitty(KittySupport {
        tmux_passthrough: in_multiplexer,
    })
}

fn read(env: &impl Fn(&str) -> Option<String>, key: &str) -> String {
    env(key).unwrap_or_default()
}

fn terminal_implements_kitty_graphics(
    env: &impl Fn(&str) -> Option<String>,
    term: &str,
    term_program: &str,
) -> bool {
    // kitty itself.
    if !read(env, "KITTY_WINDOW_ID").is_empty() || term.contains("kitty") {
        return true;
    }
    // Ghostty exports these paths from the shell that launched it, and they
    // survive into tmux/server processes, unlike TERM_PROGRAM.
    if !read(env, "GHOSTTY_RESOURCES_DIR").is_empty() || !read(env, "GHOSTTY_BIN_DIR").is_empty() {
        return true;
    }
    matches!(
        term_program.to_ascii_lowercase().as_str(),
        "ghostty" | "wezterm" | "rio"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of(pairs: &[(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |key: &str| {
            pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| (*v).to_string())
        }
    }

    #[test]
    fn unknown_terminals_report_unsupported() {
        assert_eq!(
            detect(&env_of(&[("TERM", "xterm-256color")])),
            GraphicsChoice::Off
        );
        assert_eq!(detect(&env_of(&[("TERM", "dumb")])), GraphicsChoice::Off);
        assert_eq!(detect(&env_of(&[])), GraphicsChoice::Off);
    }

    #[test]
    fn tmux_inside_ghostty_routes_through_passthrough() {
        let choice = detect(&env_of(&[
            ("GHOSTTY_RESOURCES_DIR", "/ghostty"),
            ("TERM", "screen-256color"),
            ("TMUX", "/tmp/tmux,1,0"),
        ]));
        assert_eq!(
            choice.support(),
            Some(KittySupport {
                tmux_passthrough: true
            })
        );
    }
}
