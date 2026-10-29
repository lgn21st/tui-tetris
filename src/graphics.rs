//! Pixel rendering of the well through the Kitty graphics protocol.
//!
//! Composition layer: resolve the terminal's framing, measure the cell grid, and
//! hand the layer escapes to `TerminalRenderer::write_raw` after the text pass.
//! The playfield is pixel-only, so the interactive modes refuse to start without
//! a working backend instead of showing an empty well. Text (HUD, labels,
//! banners) stays text: the terminal font renders it better than a bitmap font.

use tetris_core::core::GameSnapshot;
use tetris_terminal::term::kitty::{
    KittySupport, PreviewRenderer, WellGeometry, WellRenderer, detect,
};
use tetris_terminal::term::{
    GameView, TerminalRenderer, Viewport, detect_window_metrics, squarest_cell_size,
};

/// Why the pixel playfield cannot be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphicsUnavailable(pub String);

impl std::fmt::Display for GraphicsUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Terminal geometry, measured once and re-measured on resize.
#[derive(Debug, Clone, Copy)]
struct Display {
    cell_px: (u16, u16),
    window_px: (u32, u32),
    window_cells: (u32, u32),
}

impl Display {
    fn detect() -> Option<Self> {
        let (window_px, window_cells) = detect_window_metrics()?;
        let cell_px = cell_pixels_from_env().unwrap_or((
            (window_px.0 / window_cells.0).clamp(1, u32::from(u16::MAX)) as u16,
            (window_px.1 / window_cells.1).clamp(1, u32::from(u16::MAX)) as u16,
        ));
        Some(Self {
            cell_px,
            window_px,
            window_cells,
        })
    }

    /// Terminal cells per mino, and the device-pixel side that makes it square.
    fn board_mino(&self) -> u32 {
        let (cell_w, _) = squarest_cell_size(self.cell_px.0, self.cell_px.1);
        u32::from(cell_w) * u32::from(self.cell_px.0)
    }

    /// Cells the frame must reserve for the well.
    ///
    /// The frame hugs a square mino grid: 20 square minos of `side` pixels need
    /// `ceil(800 / cell_px_h)` rows, which is 19 rows for an 8x17 cell instead
    /// of 20 - and 19 is what keeps the image at a scale factor of exactly 1.
    fn well_cells(&self) -> (u16, u16) {
        let side = self.board_mino();
        let cell_w = u32::from(self.cell_px.0).max(1);
        let cell_h = u32::from(self.cell_px.1).max(1);
        (
            (10 * side).div_ceil(cell_w).clamp(4, 200) as u16,
            (20 * side).div_ceil(cell_h).clamp(4, 200) as u16,
        )
    }

    /// Device-pixel box of a cell box, from the window's own pixel size.
    fn layer_px(&self, cells: (u16, u16)) -> (u32, u32) {
        if self.window_cells.0 == 0 || self.window_cells.1 == 0 {
            return (
                u32::from(cells.0) * u32::from(self.cell_px.0),
                u32::from(cells.1) * u32::from(self.cell_px.1),
            );
        }
        (
            (u64::from(cells.0) * u64::from(self.window_px.0) / u64::from(self.window_cells.0))
                as u32,
            (u64::from(cells.1) * u64::from(self.window_px.1) / u64::from(self.window_cells.1))
                as u32,
        )
    }

    fn geometry(&self, view: &GameView) -> WellGeometry {
        let cells = view.well_cells();
        WellGeometry::new(cells, self.layer_px(cells))
    }
}

/// The well's pixel backend.
pub struct Graphics {
    display: Display,
    renderer: WellRenderer,
    previews: PreviewRenderer,
    out: Vec<u8>,
    last_geometry: WellGeometry,
}

impl Graphics {
    /// Resolve the backend once at startup.
    ///
    /// Fails when the terminal does not implement Kitty graphics, when tmux
    /// swallows passthrough, or when the cell grid cannot be measured - all of
    /// which would leave the well blank.
    pub fn from_env() -> Result<Self, GraphicsUnavailable> {
        let tmux = tmux_probe();
        let forced = std::env::var("TUI_TETRIS_GRAPHICS")
            .map(|value| value.trim().to_ascii_lowercase())
            .unwrap_or_default();
        if matches!(forced.as_str(), "off" | "none" | "never" | "0" | "false") {
            return Err(GraphicsUnavailable(
                "TUI_TETRIS_GRAPHICS=off: the playfield is graphics-only".to_string(),
            ));
        }

        let Some(support) = detect(&|key| outer_env_key(&tmux, key)) else {
            return Err(GraphicsUnavailable(format!(
                "this terminal does not implement the Kitty graphics protocol (TERM={}, TERM_PROGRAM={})",
                std::env::var("TERM").unwrap_or_default(),
                std::env::var("TERM_PROGRAM").unwrap_or_default(),
            )));
        };

        let forced = matches!(forced.as_str(), "kitty" | "on" | "force" | "1" | "true");
        if !forced
            && let Some(probe) = tmux.as_ref()
            && !probe.passthrough_enabled()
        {
            return Err(GraphicsUnavailable(
                "tmux allow-passthrough is off (set -g allow-passthrough on)".to_string(),
            ));
        }

        let display = Display::detect().ok_or_else(|| {
            GraphicsUnavailable(
                "the terminal did not report its pixel size (TIOCGWINSZ), so minos cannot be sized"
                    .to_string(),
            )
        })?;
        let geometry =
            WellGeometry::new(display.well_cells(), display.layer_px(display.well_cells()));
        Ok(Self {
            renderer: WellRenderer::new(support, geometry),
            previews: PreviewRenderer::new(support, display.board_mino()),
            display,
            out: Vec::with_capacity(1024 * 1024),
            last_geometry: geometry,
        })
    }

    pub fn support(&self) -> KittySupport {
        self.renderer.support()
    }

    /// Cell boxes the HUD must reserve for the pixel previews.
    pub fn preview_cells(&self) -> ((u16, u16), (u16, u16)) {
        (
            self.previews.hold_cell_box(self.display.cell_px),
            self.previews.next_cell_box(self.display.cell_px),
        )
    }

    /// Cells the frame must reserve for the well interior.
    pub fn well_cells(&self) -> (u16, u16) {
        self.display.well_cells()
    }

    /// Short one-line description for `tui-tetris diagnostic`.
    pub fn describe(&self) -> String {
        let framing = if self.support().tmux_passthrough {
            "tmux-passthrough"
        } else {
            "direct"
        };
        let (width, height) = self.last_geometry.pixels();
        format!(
            "kitty,{framing},cell={}x{}px,well={width}x{height}px,mino={}px",
            self.display.cell_px.0,
            self.display.cell_px.1,
            self.display.board_mino()
        )
    }

    /// The next text pass will be a full redraw, which clears images.
    pub fn invalidate(&mut self) {
        if let Some(display) = Display::detect() {
            // A new display scale also changes the preview sprite size.
            if display.board_mino() != self.display.board_mino() {
                self.previews = PreviewRenderer::new(self.support(), display.board_mino());
            }
            self.display = display;
        }
        self.renderer.invalidate();
        self.previews.invalidate();
    }

    /// Emit whichever layers changed. Returns the bytes to write after the text
    /// frame, or an empty slice.
    pub fn sync(&mut self, snap: &GameSnapshot, view: &GameView, viewport: Viewport) -> &[u8] {
        self.out.clear();
        let geometry = self.display.geometry(view);
        self.last_geometry = geometry;
        let origin = view.well_origin(viewport);

        let mut failed = self
            .renderer
            .sync(snap, geometry, origin, &mut self.out)
            .err();
        if failed.is_none()
            && let Some(layout) = view.panel_layout(snap, viewport)
        {
            failed = self
                .previews
                .sync(snap, layout.hold, layout.next, &mut self.out)
                .err();
        }
        if let Some(error) = failed {
            eprintln!("tui-tetris: layer update failed: {error}");
            self.out.clear();
        }
        &self.out
    }

    /// Remove every image from the terminal before leaving the screen.
    pub fn shutdown(&mut self, term: &mut TerminalRenderer) {
        self.out.clear();
        self.renderer.shutdown(&mut self.out);
        self.previews.shutdown(&mut self.out);
        if !self.out.is_empty()
            && let Err(error) = term.write_raw(&self.out)
        {
            let _ = error;
        }
    }
}

/// Result of asking tmux about the attached client.
pub struct TmuxProbe {
    client_termname: Option<String>,
    allow_passthrough: Option<String>,
}

impl TmuxProbe {
    fn passthrough_enabled(&self) -> bool {
        matches!(self.allow_passthrough.as_deref(), Some("on") | Some("all"))
    }
}

fn tmux_query(args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("tmux")
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

/// Query tmux for the pane's passthrough setting and the outer terminal name.
///
/// `None` outside tmux. `TUI_TETRIS_TMUX_PASSTHROUGH` and
/// `TUI_TETRIS_TMUX_CLIENT` override the queries, which helps when `tmux` is not
/// on `PATH`, the option is set per pane, or a harness needs to emulate a pane.
pub fn tmux_probe() -> Option<TmuxProbe> {
    std::env::var_os("TMUX")?;
    let override_of = |key: &str, args: &[&str]| {
        std::env::var(key)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| tmux_query(args).unwrap_or_default())
    };
    Some(TmuxProbe {
        client_termname: Some(override_of(
            "TUI_TETRIS_TMUX_CLIENT",
            &["display-message", "-p", "#{client_termname}"],
        ))
        .filter(|value| !value.is_empty()),
        allow_passthrough: Some(override_of(
            "TUI_TETRIS_TMUX_PASSTHROUGH",
            &["show", "-gv", "allow-passthrough"],
        ))
        .filter(|value| !value.is_empty()),
    })
}

/// Resolve one environment key, preferring the outer terminal's identity when
/// running inside tmux (tmux overwrites `TERM`/`TERM_PROGRAM` for the pane).
fn outer_env_key(tmux: &Option<TmuxProbe>, key: &str) -> Option<String> {
    if let Some(probe) = tmux {
        let client = probe.client_termname.as_deref();
        match key {
            "TERM" => {
                return client
                    .map(str::to_string)
                    .or_else(|| std::env::var(key).ok());
            }
            "TERM_PROGRAM" => {
                if let Some(program) = client.and_then(outer_program) {
                    return Some(program.to_string());
                }
            }
            _ => {}
        }
    }
    std::env::var(key).ok()
}

fn outer_program(client_termname: &str) -> Option<&'static str> {
    let name = client_termname.to_ascii_lowercase();
    if name.contains("ghostty") {
        Some("ghostty")
    } else if name.contains("wezterm") {
        Some("WezTerm")
    } else if name.contains("rio") {
        Some("rio")
    } else {
        None
    }
}

fn cell_pixels_from_env() -> Option<(u16, u16)> {
    let read = |key: &str| {
        std::env::var(key)
            .ok()
            .and_then(|value| value.trim().parse::<u16>().ok())
            .filter(|value| *value > 0)
    };
    match (read("TUI_TETRIS_CELL_PX_W"), read("TUI_TETRIS_CELL_PX_H")) {
        (Some(w), Some(h)) => Some((w, h)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tmux_client_names_map_to_their_terminal() {
        assert_eq!(outer_program("xterm-ghostty"), Some("ghostty"));
        assert_eq!(outer_program("xterm-kitty"), None);
        assert_eq!(outer_program("screen-256color"), None);
    }

    fn display(cell_px: (u16, u16), window_px: (u32, u32), window_cells: (u32, u32)) -> Display {
        Display {
            cell_px,
            window_px,
            window_cells,
        }
    }

    #[test]
    fn the_frame_hugs_square_minos_at_any_scale() {
        // 1x: 8x17 cell -> 16px minos, 19 rows.
        let one_x = display((8, 17), (640, 408), (80, 24));
        assert_eq!(one_x.board_mino(), 16);
        assert_eq!(one_x.well_cells(), (20, 19));
        assert_eq!(one_x.layer_px((20, 19)), (160, 323));

        // 2x: the same grid in device pixels, doubled.
        let two_x = display((16, 34), (1280, 816), (80, 24));
        assert_eq!(two_x.board_mino(), 32);
        assert_eq!(two_x.well_cells(), (20, 19));
        assert_eq!(two_x.layer_px((20, 19)), (320, 646));
    }

    #[test]
    fn missing_window_metrics_fall_back_to_integer_cells() {
        let fallback = display((8, 17), (0, 0), (0, 0));
        assert_eq!(fallback.layer_px((20, 19)), (160, 323));
    }
}
