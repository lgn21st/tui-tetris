#[derive(Debug, Clone)]
pub struct RenderThrottle {
    min_static_interval_ms: u64,
    last_render_ms: u64,
    last_fingerprint: u64,
    has_rendered: bool,
}

impl RenderThrottle {
    pub fn new(min_static_interval_ms: u64) -> Self {
        Self {
            min_static_interval_ms,
            last_render_ms: 0,
            last_fingerprint: 0,
            has_rendered: false,
        }
    }

    /// Decide whether to render a new frame.
    ///
    /// - When `is_static=false`: always render (no throttling).
    /// - When `is_static=true`: render immediately on fingerprint change, otherwise at most
    ///   once per `min_static_interval_ms`.
    pub fn should_render(&mut self, now_ms: u64, fingerprint: u64, is_static: bool) -> bool {
        if !self.has_rendered {
            self.has_rendered = true;
            self.last_render_ms = now_ms;
            self.last_fingerprint = fingerprint;
            return true;
        }

        if !is_static {
            self.last_render_ms = now_ms;
            self.last_fingerprint = fingerprint;
            return true;
        }

        if fingerprint != self.last_fingerprint {
            self.last_render_ms = now_ms;
            self.last_fingerprint = fingerprint;
            return true;
        }

        if now_ms.saturating_sub(self.last_render_ms) >= self.min_static_interval_ms {
            self.last_render_ms = now_ms;
            return true;
        }

        false
    }
}

