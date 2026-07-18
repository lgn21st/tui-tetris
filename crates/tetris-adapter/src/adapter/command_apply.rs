use crate::adapter::protocol::ErrorCode;
use crate::engine::place::PlaceError;

pub fn map_place_error_code(err: PlaceError) -> ErrorCode {
    match err {
        PlaceError::HoldUnavailable => ErrorCode::HoldUnavailable,
        PlaceError::RotationBlocked
        | PlaceError::XOutOfBounds
        | PlaceError::XBlocked
        | PlaceError::NotPlayable
        | PlaceError::NoActive => ErrorCode::InvalidPlace,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_place_error_code_maps_hold_unavailable() {
        assert_eq!(
            map_place_error_code(PlaceError::HoldUnavailable),
            ErrorCode::HoldUnavailable
        );
        assert_eq!(
            map_place_error_code(PlaceError::RotationBlocked),
            ErrorCode::InvalidPlace
        );
    }
}
