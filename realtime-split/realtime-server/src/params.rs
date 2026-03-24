use crate::magicnums::*;
use crate::types::*;

pub(crate) fn param_level_bounds(param: ParamKind) -> (u8, u8) {
    match param {
        ParamKind::Gravity | ParamKind::Speed => (PARAM_MIN_LEVEL, PARAM_MAX_LEVEL_THREE_STAGE),
        ParamKind::Friction => (PARAM_MIN_LEVEL, PARAM_MAX_LEVEL_FRICTION),
    }
}

pub(crate) fn apply_param_change(
    params: &PlayerParams,
    next_param_change_at_unix: u64,
    param: ParamKind,
    direction: ParamDirection,
    now_unix: u64,
) -> std::result::Result<ParamMutation, ParamUpdateError> {
    if now_unix < next_param_change_at_unix {
        return Err(ParamUpdateError::CooldownActive {
            next_allowed_at_unix: next_param_change_at_unix,
        });
    }

    let mut next = params.clone();
    let slot = match param {
        ParamKind::Gravity => &mut next.gravity,
        ParamKind::Friction => &mut next.friction,
        ParamKind::Speed => &mut next.speed,
    };

    let candidate = match direction {
        ParamDirection::Increase => slot.saturating_add(1),
        ParamDirection::Decrease => slot.saturating_sub(1),
    };

    let (min_level, max_level) = param_level_bounds(param);
    if !(min_level..=max_level).contains(&candidate) {
        return Err(ParamUpdateError::OutOfRange);
    }

    *slot = candidate;

    Ok(ParamMutation {
        params: next,
        next_param_change_at_unix: now_unix + PARAM_CHANGE_COOLDOWN_SECS,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn param_change_updates_one_step_only() {
        let params = PlayerParams::default();
        let updated = apply_param_change(
            &params,
            0,
            ParamKind::Gravity,
            ParamDirection::Increase,
            100,
        )
        .expect("should update");
        assert_eq!(updated.params.gravity, 3);
        assert_eq!(updated.params.friction, 2);
        assert_eq!(updated.params.speed, 2);
        assert_eq!(updated.next_param_change_at_unix, 130);
    }

    #[test]
    fn friction_can_only_toggle_between_off_and_on() {
        let params = PlayerParams::default();
        let updated = apply_param_change(
            &params,
            0,
            ParamKind::Friction,
            ParamDirection::Decrease,
            100,
        )
        .expect("should update");
        assert_eq!(updated.params.friction, 1);

        let err = apply_param_change(
            &params,
            0,
            ParamKind::Friction,
            ParamDirection::Increase,
            100,
        )
        .expect_err("must reject");
        assert_eq!(err, ParamUpdateError::OutOfRange);
    }

    #[test]
    fn param_change_respects_range_limit() {
        let params = PlayerParams {
            gravity: 3,
            friction: 2,
            speed: 2,
        };
        let err = apply_param_change(
            &params,
            0,
            ParamKind::Gravity,
            ParamDirection::Increase,
            100,
        )
        .expect_err("must reject");
        assert_eq!(err, ParamUpdateError::OutOfRange);
    }

    #[test]
    fn param_change_respects_cooldown() {
        let params = PlayerParams::default();
        let err = apply_param_change(
            &params,
            120,
            ParamKind::Speed,
            ParamDirection::Decrease,
            100,
        )
        .expect_err("must reject");
        assert_eq!(
            err,
            ParamUpdateError::CooldownActive {
                next_allowed_at_unix: 120
            }
        );
    }
}
