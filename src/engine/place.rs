use crate::core::GameState;
use crate::types::{GameAction, Rotation, BOARD_WIDTH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceError {
    HoldUnavailable,
    RotationBlocked,
    XOutOfBounds,
    XBlocked,
    NotPlayable,
    NoActive,
}

impl PlaceError {
    pub fn code(self) -> &'static str {
        match self {
            PlaceError::HoldUnavailable => "hold_unavailable",
            PlaceError::RotationBlocked
            | PlaceError::XOutOfBounds
            | PlaceError::XBlocked
            | PlaceError::NotPlayable
            | PlaceError::NoActive => "invalid_place",
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            PlaceError::HoldUnavailable => "hold requested when unavailable",
            PlaceError::RotationBlocked => "could not rotate to target rotation",
            PlaceError::XOutOfBounds => "target x would place piece out of bounds",
            PlaceError::XBlocked => "could not move to target x due to collision",
            PlaceError::NotPlayable => "game is not playable",
            PlaceError::NoActive => "no active piece",
        }
    }
}

pub fn apply_place(
    state: &mut GameState,
    target_x: i8,
    target_rot: Rotation,
    use_hold: bool,
) -> Result<(), PlaceError> {
    if state.paused() || state.game_over() {
        return Err(PlaceError::NotPlayable);
    }

    // Hold first if requested.
    if use_hold {
        if !state.apply_action(GameAction::Hold) {
            return Err(PlaceError::HoldUnavailable);
        }
    }

    let Some(active0) = state.active() else {
        return Err(PlaceError::NoActive);
    };

    // Try CW/CCW plans including 180; keep shorter first.
    let rot_to_i = |r: Rotation| match r {
        Rotation::North => 0i8,
        Rotation::East => 1i8,
        Rotation::South => 2i8,
        Rotation::West => 3i8,
    };

    let cur = rot_to_i(active0.rotation);
    let tgt = rot_to_i(target_rot);
    let cw = (tgt - cur).rem_euclid(4) as u8;
    let ccw = (cur - tgt).rem_euclid(4) as u8;

    // Always consider both directions; for 180 both will be 2.
    let mut plans: [(&'static str, bool, u8); 2] = [("cw", true, cw), ("ccw", false, ccw)];
    if plans[1].2 < plans[0].2 {
        plans.swap(0, 1);
    }

    let snapshot = state.clone();
    let mut rotated = false;
    for (_, is_cw, steps) in plans {
        *state = snapshot.clone();
        let mut ok = true;
        for _ in 0..steps {
            if !state.try_rotate(is_cw) {
                ok = false;
                break;
            }
        }
        if ok {
            rotated = true;
            break;
        }
    }
    if !rotated {
        return Err(PlaceError::RotationBlocked);
    }

    let Some(active) = state.active() else {
        return Err(PlaceError::NoActive);
    };

    if active.rotation != target_rot {
        return Err(PlaceError::RotationBlocked);
    }

    // Validate x bounds based on current shape.
    let shape = active.shape();
    let mut min_dx: i8 = i8::MAX;
    let mut max_dx: i8 = i8::MIN;
    for (dx, _) in shape {
        min_dx = min_dx.min(dx);
        max_dx = max_dx.max(dx);
    }
    if target_x + min_dx < 0 || target_x + max_dx >= BOARD_WIDTH as i8 {
        return Err(PlaceError::XOutOfBounds);
    }

    let dx = target_x - active.x;
    if dx > 0 {
        for _ in 0..dx {
            if !state.try_move(1, 0) {
                return Err(PlaceError::XBlocked);
            }
        }
    } else if dx < 0 {
        for _ in 0..(-dx) {
            if !state.try_move(-1, 0) {
                return Err(PlaceError::XBlocked);
            }
        }
    }

    if !state.apply_action(GameAction::HardDrop) {
        return Err(if state.active().is_none() {
            PlaceError::NoActive
        } else {
            PlaceError::NotPlayable
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PieceKind;

    #[test]
    fn place_rejected_when_paused() {
        let mut gs = GameState::new(1);
        gs.start();
        assert!(gs.apply_action(GameAction::Pause));

        let a = gs.active().expect("expected active piece");
        let err = apply_place(&mut gs, a.x, a.rotation, false).unwrap_err();
        assert!(matches!(err, PlaceError::NotPlayable));
    }

    #[test]
    fn place_rejected_when_x_out_of_bounds() {
        let mut gs = GameState::new(1);
        gs.start();

        let a = gs.active().expect("expected active piece");
        let err = apply_place(&mut gs, -50, a.rotation, false).unwrap_err();
        assert!(matches!(err, PlaceError::XOutOfBounds));
    }

    #[test]
    fn place_rejected_when_x_blocked_by_collision() {
        let mut gs = GameState::new(1);
        gs.start();

        let a = gs.active().expect("expected active piece");
        let shape = a.shape();

        // Prefer moving left; fall back to right if left would be out of bounds.
        let mut min_dx = i8::MAX;
        let mut max_dx = i8::MIN;
        for (dx, _) in shape {
            min_dx = min_dx.min(dx);
            max_dx = max_dx.max(dx);
        }

        let (delta_x, target_x) = if a.x + min_dx - 1 >= 0 {
            (-1i8, a.x - 1)
        } else if a.x + max_dx + 1 < BOARD_WIDTH as i8 {
            (1i8, a.x + 1)
        } else {
            panic!("cannot construct a simple blocked-move test for this spawn");
        };

        // Place blocking cells exactly where the piece would occupy after shifting.
        for (dx, dy) in shape {
            let bx = a.x + dx + delta_x;
            let by = a.y + dy;
            let _ = gs.board_mut().set(bx, by, Some(PieceKind::I));
        }

        let err = apply_place(&mut gs, target_x, a.rotation, false).unwrap_err();
        assert!(matches!(err, PlaceError::XBlocked));
    }
}
