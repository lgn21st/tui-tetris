//! Pixel rendering of the well through the Kitty graphics protocol.
//!
//! The playfield is pixel-native: no board cell is ever drawn as a character.
//! This module is the composition layer — it resolves the terminal's framing,
//! measures the cell grid, and hands the layer escapes to
//! `TerminalRenderer::write_raw` after the text pass.
//!
//! Graphics are a hard requirement for the playfield, so the interactive modes
//! refuse to start without them instead of silently showing an empty well.
//! Text (HUD, side panel, banners) stays text: the terminal's font renders it
//! better than any bitmap font we could ship.

use tetris_core::core::GameSnapshot;
use tetris_terminal::term::kitty::{
    GraphicsChoice, KittySupport, PreviewRenderer, WellGeometry, WellRenderer, detect,
};
use tetris_terminal::term::{GameView, TerminalRenderer, Viewport, detect_window_metrics};

/// Window size in pixels and in cells.
type WindowMetrics = ((u32, u32), (u32, u32));

/// Why the pixel playfield cannot be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphicsUnavailable(pub String);

impl std::fmt::Display for GraphicsUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The well's pixel backend.
pub struct Graphics {
    renderer: WellRenderer,
    previews: PreviewRenderer,
    board_mino: u32,
    out: Vec<u8>,
    cell_pixels: (u16, u16),
    window_metrics: WindowMetrics,
    last_geometry: WellGeometry,
}

impl Graphics {
    /// Resolve the backend once at startup.
    ///
    /// Fails when the terminal does not implement Kitty graphics, when tmux
    /// swallows passthrough, or when the cell grid cannot be measured — all of
    /// which would leave the well blank.
    pub fn from_env() -> Result<Self, GraphicsUnavailable> {
        let tmux = tmux_probe();
        let forced = std::env::var("TUI_TETRIS_GRAPHICS")
            .map(|value| value.trim().to_ascii_lowercase())
            .ok();
        match forced.as_deref() {
            Some("off") | Some("none") | Some("never") | Some("0") | Some("false") => {
                return Err(GraphicsUnavailable(
                    "TUI_TETRIS_GRAPHICS=off: the playfield is graphics-only".to_string(),
                ));
            }
            _ => {}
        }

        let choice = detect(&|key| outer_env_key(&tmux, key));
        let GraphicsChoice::Kitty(support) = choice else {
            return Err(GraphicsUnavailable(format!(
                "this terminal does not implement the Kitty graphics protocol (TERM={}, TERM_PROGRAM={})",
                std::env::var("TERM").unwrap_or_default(),
                std::env::var("TERM_PROGRAM").unwrap_or_default(),
            )));
        };

        let explicitly_forced = matches!(
            forced.as_deref(),
            Some("kitty" | "on" | "force" | "1" | "true")
        );
        if !explicitly_forced
            && let Some(probe) = tmux.as_ref()
            && !probe.passthrough_enabled()
        {
            return Err(GraphicsUnavailable(
                "tmux allow-passthrough is off (set -g allow-passthrough on)".to_string(),
            ));
        }

        let window_metrics = detect_window_metrics().ok_or_else(|| {
            GraphicsUnavailable(
                "the terminal did not report its pixel size (TIOCGWINSZ), so minos cannot be sized"
                    .to_string(),
            )
        })?;
        let cell_pixels =
            cell_pixels_from_env().unwrap_or_else(|| cell_pixels_from_metrics(window_metrics));
        let geometry = WellGeometry::new(2, 1, cell_pixels);
        let board_mino =
            u32::from(tetris_terminal::term::squarest_cell_size(cell_pixels.0, cell_pixels.1).0)
                * u32::from(cell_pixels.0);

        Ok(Self {
            renderer: WellRenderer::new(support, geometry),
            previews: PreviewRenderer::new(support, board_mino),
            board_mino,
            out: Vec::with_capacity(1024 * 1024),
            cell_pixels,
            window_metrics,
            last_geometry: geometry,
        })
    }

    pub fn support(&self) -> KittySupport {
        self.renderer.support()
    }

    /// Cell boxes the HUD must reserve for the pixel previews.
    pub fn preview_cells(&self) -> ((u16, u16), (u16, u16)) {
        (
            self.previews.hold_cell_box(self.cell_pixels),
            self.previews.next_cell_box(self.cell_pixels),
        )
    }

    /// Terminal cells the frame must reserve for the well interior.
    ///
    /// The frame hugs a square mino grid: 20 square minos of `side` pixels need
    /// `ceil(20 * side / cell_px_h)` rows, which is 19 rows for an 8x17 cell
    /// instead of 20. Two rows of frame padding are added by the view.
    pub fn well_cells(&self) -> (u16, u16) {
        let (cell_px_w, cell_px_h) = (u32::from(self.cell_pixels.0), u32::from(self.cell_pixels.1));
        let (cell_w, _) =
            tetris_terminal::term::squarest_cell_size(self.cell_pixels.0, self.cell_pixels.1);
        let side = u32::from(cell_w) * cell_px_w;
        let rows = (20 * side).div_ceil(cell_px_h.max(1));
        let cols = (10 * side).div_ceil(cell_px_w.max(1));
        (cols.clamp(4, 200) as u16, rows.clamp(4, 200) as u16)
    }

    /// Short one-line description for `tui-tetris diagnostic`.
    pub fn describe(&self) -> String {
        let support = self.support();
        let framing = if support.tmux_passthrough {
            "tmux-passthrough"
        } else {
            "direct"
        };
        let (width, height) = self.last_geometry.pixels();
        format!(
            "kitty,{framing},cell={}x{}px,well={width}x{height}px,mino={}px",
            self.cell_pixels.0,
            self.cell_pixels.1,
            self.last_geometry.mino_side()
        )
    }

    /// The next text pass will be a full redraw, which clears images.
    pub fn invalidate(&mut self) {
        self.window_metrics = detect_window_metrics().unwrap_or(self.window_metrics);
        self.cell_pixels =
            cell_pixels_from_env().unwrap_or_else(|| cell_pixels_from_metrics(self.window_metrics));
        self.renderer.invalidate();
        self.previews.invalidate();
        let board_mino = u32::from(
            tetris_terminal::term::squarest_cell_size(self.cell_pixels.0, self.cell_pixels.1).0,
        ) * u32::from(self.cell_pixels.0);
        if board_mino != self.board_mino {
            // Display scale changed: rebuild the preview sprites at the new size.
            self.board_mino = board_mino;
            self.previews = PreviewRenderer::new(self.renderer.support(), board_mino);
        }
    }

    fn geometry(&self, view: &GameView) -> WellGeometry {
        let (cell_w, cell_h) = view.cell_size();
        let (window_px, window_cells) = self.window_metrics;
        let (cols, rows) = window_cells;
        let mut geometry = WellGeometry::new(cell_w, cell_h, self.cell_pixels);
        // Size the layer to the frame's cell box, using the window's pixel size
        // rather than `cells * integer cell size` so it is exact.
        let box_cells = view.well_cells();
        if cols > 0 && rows > 0 {
            let width = (u64::from(box_cells.0) * u64::from(window_px.0) / u64::from(cols)) as u32;
            let height = (u64::from(box_cells.1) * u64::from(window_px.1) / u64::from(rows)) as u32;
            geometry = geometry.with_layer_pixels(width, height);
        } else {
            geometry = geometry.with_layer_pixels(
                u32::from(box_cells.0) * u32::from(self.cell_pixels.0),
                u32::from(box_cells.1) * u32::from(self.cell_pixels.1),
            );
        }
        geometry.with_box_cells(box_cells)
    }

    /// Emit whichever well layers changed. Returns the bytes to write after the
    /// text frame, or an empty slice.
    pub fn sync(&mut self, snap: &GameSnapshot, view: &GameView, viewport: Viewport) -> &[u8] {
        self.out.clear();
        let geometry = self.geometry(view);
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

fn cell_pixels_from_metrics((window_px, window_cells): WindowMetrics) -> (u16, u16) {
    let (cols, rows) = window_cells;
    if cols == 0 || rows == 0 {
        return (8, 17);
    }
    let w = (window_px.0 / cols).clamp(1, u32::from(u16::MAX)) as u16;
    let h = (window_px.1 / rows).clamp(1, u32::from(u16::MAX)) as u16;
    (w, h)
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

    #[test]
    fn cell_pixels_are_derived_from_the_window_grid() {
        assert_eq!(cell_pixels_from_metrics(((1280, 816), (160, 48))), (8, 17));
        assert_eq!(cell_pixels_from_metrics(((0, 0), (0, 0))), (8, 17));
    }
}
