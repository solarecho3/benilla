//! The driven body's head look. After the strafe twist has written SpineLow and Head, this pass
//! turns the head the rest of the way toward the current target, and nods a little, stopping at
//! the neck. The body keeps the facing the mover gave it. With no target, or with one inside the
//! body, the extra eases back to nothing and the strafe twist is the only procedural yaw.
//!
//! Unit space matches the twist: Y up, forward −Z, yaw `from_rotation_y`, so a heading of −π/2
//! faces +X (the character's right when the body yaw is 0). The extra is a remainder on top of
//! the twist's head share, and the sum is clamped, so a target already on the aim adds nothing
//! and a target behind holds the stop instead of spinning the neck.

use bevy::prelude::*;

use super::twist::{armed_shares, twist_stood_down, wrap_pi, BodyTwist};

/// How far the head bone may yaw from the chest, about 66°.
const NECK_YAW: f32 = 1.15;
/// How far it may nod, about 23°.
const NECK_PITCH: f32 = 0.40;
/// Inside this horizontal distance the target is the body itself.
const LOOK_DEAD: f32 = 0.4;
/// Rad/s. A stop-to-stop turn of the neck takes about a quarter second.
const YAW_RATE: f32 = 4.5;
const PITCH_RATE: f32 = 3.0;
/// A hitch must not spend the whole turn in one frame.
const DT_CAP: f32 = 0.05;

/// The head channel for [`apply_head_look`]. Its own base/last-out pair, separate from the twist's,
/// so the two writers do not stack.
#[derive(Component)]
pub(crate) struct HeadLook {
    bone: u16,
    /// The twist-output local this pass last composed on.
    base: Quat,
    /// What we last wrote. A bone still holding it was not re-keyed, so `base` stays.
    last_out: Quat,
    /// Smoothed yaw added on top of the strafe head share.
    yaw: f32,
    /// Smoothed nod.
    pitch: f32,
}

impl HeadLook {
    pub(crate) fn new(bone: u16) -> Self {
        Self {
            bone,
            base: Quat::IDENTITY,
            last_out: Quat::IDENTITY,
            yaw: 0.0,
            pitch: 0.0,
        }
    }
}

/// Extra head yaw so `head_share + extra` equals the neck-clamped yaw from the chest (`spine`)
/// toward `gap_to_target` (`wrap(target_yaw − body_yaw)`).
fn look_yaw_extra(spine: f32, head_share: f32, gap_to_target: f32) -> f32 {
    let desired = wrap_pi(gap_to_target - spine).clamp(-NECK_YAW, NECK_YAW);
    wrap_pi(desired - head_share)
}

/// Nod toward a target `dy` yards above the eyes, `horiz` yards away in the ground plane.
/// Non-positive or tiny `horiz` is the dead zone: look ahead.
fn look_pitch(dy: f32, horiz: f32) -> f32 {
    if !(horiz >= LOOK_DEAD) {
        return 0.0;
    }
    (dy / horiz).atan().clamp(-NECK_PITCH, NECK_PITCH)
}

/// `(yaw extra, pitch)` for one frame. `target` is `(feet, head height)`; `None` rests the neck.
fn look_goal(
    body_pos: Vec3,
    body_rot: Quat,
    body_height: f32,
    target: Option<(Vec3, f32)>,
    spine: f32,
    head_share: f32,
) -> (f32, f32) {
    let Some((target_pos, target_height)) = target else {
        return (0.0, 0.0);
    };
    let delta = (target_pos + Vec3::Y * target_height) - (body_pos + Vec3::Y * body_height);
    let horiz = Vec2::new(delta.x, delta.z).length();
    if horiz < LOOK_DEAD {
        return (0.0, 0.0);
    }
    // The same yaw the follow camera uses: `from_rotation_y(y) * -Z = (−sin y, 0, −cos y)`.
    let target_yaw = (-delta.x).atan2(-delta.z);
    let fwd = body_rot * Vec3::NEG_Z;
    let body_yaw = (-fwd.x).atan2(-fwd.z);
    let yaw = look_yaw_extra(spine, head_share, wrap_pi(target_yaw - body_yaw));
    (yaw, look_pitch(delta.y, horiz))
}

/// Move `current` toward `goal` by at most `rate * dt`, on the short arc.
fn approach(current: f32, goal: f32, rate: f32, dt: f32) -> f32 {
    let step = (rate * dt).max(0.0);
    let delta = wrap_pi(goal - current);
    if delta.abs() <= step {
        goal
    } else {
        current + delta.signum() * step
    }
}

/// `local' = local · yaw · pitch`, yaw about unit up and pitch about the headed right, each
/// expressed in the bone's frame through `g` (the bone's unit-space rotation before the extra).
/// Heading is the head's procedural yaw in the unit: spine share plus the clamped head total.
fn compose_look(g: Quat, local: Quat, yaw: f32, pitch: f32, heading: f32) -> Quat {
    if yaw == 0.0 && pitch == 0.0 {
        return local;
    }
    let yaw_q = Quat::from_axis_angle(g.inverse() * Vec3::Y, yaw);
    let g_yawed = g * yaw_q;
    // Right of this heading. At heading 0 the body faces −Z and right is +X, which is
    // `forward × up`.
    let right = Vec3::new(heading.cos(), 0.0, -heading.sin());
    let pitch_q = Quat::from_axis_angle(g_yawed.inverse() * right, pitch);
    (local * yaw_q * pitch_q).normalize()
}

/// The bone's rotation in the unit frame, copied from the twist's walk: ancestor bones, then the
/// frames from `joints_root` up to the unit, splicing a mount seat. The rig borrow ends before
/// the seat walk so a host rig can be read from the same query.
fn unit_frame(
    rigs: &Query<&mut benilla_world::rig_anim::RigPose>,
    unit: Entity,
    bone: usize,
    local: Quat,
    anchors: &Query<&benilla_world::rig_anim::RigAnchor>,
    parents: &Query<&ChildOf>,
    locals: &Query<&Transform>,
) -> Option<Quat> {
    let (mut g, mut e) = {
        let rig = rigs.get(unit).ok()?;
        let mut g = local;
        let mut b = rig.parents.get(bone).copied().unwrap_or(-1);
        while let Ok(p) = usize::try_from(b) {
            g = rig.locals.get(p)?.rotation * g;
            b = rig.parents.get(p).copied().unwrap_or(-1);
        }
        (g, rig.joints_root)
    };
    for _ in 0..32 {
        if e == unit {
            break;
        }
        if let Some((host_rig, hb)) = anchors
            .get(e)
            .ok()
            .and_then(|a| rigs.get(a.rig).ok().map(|r| (r, a.bone)))
        {
            let mut b = Ok(hb as usize);
            while let Ok(p) = b {
                let Some(t) = host_rig.locals.get(p) else {
                    break;
                };
                g = t.rotation * g;
                b = usize::try_from(host_rig.parents.get(p).copied().unwrap_or(-1));
            }
            e = host_rig.joints_root;
            continue;
        }
        if let Ok(t) = locals.get(e) {
            g = t.rotation * g;
        }
        let Ok(parent) = parents.get(e).map(|c| c.parent()) else {
            break;
        };
        e = parent;
    }
    Some(g)
}

fn eye_height(
    heights: &Query<(
        Option<&crate::player::CameraPivot>,
        Option<&crate::net::NetEntity>,
    )>,
    entity: Entity,
) -> f32 {
    let (pivot, net) = heights.get(entity).unwrap_or((None, None));
    let scale = net.map(|n| n.scale).filter(|s| *s > 0.0).unwrap_or(1.0);
    crate::player::head_height(pivot, scale)
}

/// Compose the look onto the driven body's head, after [`super::twist::apply_body_twist`].
fn apply_head_look(
    time: Res<Time>,
    selection: Option<Res<crate::target::Selection>>,
    mut bodies: Query<
        (
            Entity,
            &mut BodyTwist,
            &mut HeadLook,
            Option<&crate::net::ObjectStore>,
        ),
        (
            With<crate::net::Embodied>,
            Without<crate::player::camera::FlyCam>,
            Without<benilla_world::rig_anim::AnimParked>,
        ),
    >,
    heights: Query<(
        Option<&crate::player::CameraPivot>,
        Option<&crate::net::NetEntity>,
    )>,
    mut rigs: Query<&mut benilla_world::rig_anim::RigPose>,
    anchors: Query<&benilla_world::rig_anim::RigAnchor>,
    parents: Query<&ChildOf>,
    locals: Query<&Transform>,
) {
    // The twist's bisect stands this pass down too, so the head stays on the clip.
    if twist_stood_down() {
        return;
    }
    let dt = time.delta_secs().min(DT_CAP);
    let target_entity = selection.as_ref().and_then(|s| s.target);
    for (unit, mut twist, mut look, store) in &mut bodies {
        let mounted = store.is_some_and(|s| s.0.unit_mount_display_id() != 0);
        let (spine, head_share) = armed_shares(twist.yaw_gap, mounted);
        let (body_pos, body_rot) = {
            let Ok(tf) = locals.get(unit) else {
                continue;
            };
            (tf.translation, tf.rotation)
        };
        let target = target_entity.and_then(|entity| {
            let translation = locals.get(entity).ok()?.translation;
            Some((translation, eye_height(&heights, entity)))
        });
        let (goal_yaw, goal_pitch) = look_goal(
            body_pos,
            body_rot,
            eye_height(&heights, unit),
            target,
            spine,
            head_share,
        );
        look.yaw = approach(look.yaw, goal_yaw, YAW_RATE, dt);
        look.pitch = approach(look.pitch, goal_pitch, PITCH_RATE, dt);
        // The written total stays inside the neck even when a strafe share jumps while the
        // extra is still easing.
        let total = (head_share + look.yaw).clamp(-NECK_YAW, NECK_YAW);
        let yaw_extra = total - head_share;
        let pitch = look.pitch.clamp(-NECK_PITCH, NECK_PITCH);
        let heading = spine + total;
        let bone = look.bone as usize;
        let (cur, base) = {
            let Ok(rig) = rigs.get(unit) else {
                continue;
            };
            let Some(local) = rig.locals.get(bone) else {
                continue;
            };
            let cur = local.rotation;
            let base = if cur == look.last_out {
                look.base
            } else {
                cur
            };
            (cur, base)
        };
        let out = if yaw_extra == 0.0 && pitch == 0.0 {
            base
        } else {
            let Some(g) = unit_frame(&rigs, unit, bone, base, &anchors, &parents, &locals) else {
                continue;
            };
            compose_look(g, base, yaw_extra, pitch, heading)
        };
        look.base = base;
        look.last_out = out;
        let holds_out = if out != cur {
            if let Ok(mut rig) = rigs.get_mut(unit) {
                if let Some(local) = rig.locals.get_mut(bone) {
                    local.rotation = out;
                    rig.pose_dirty = true;
                    true
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            true
        };
        if holds_out {
            twist.note_head_written(out);
        }
    }
}

/// Register [`apply_head_look`] after the strafe twist, in the same pose window.
pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        PostUpdate,
        apply_head_look
            .after(super::twist::apply_body_twist)
            .in_set(benilla_world::rig_anim::PosePost),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};

    fn right_of(heading: f32) -> Vec3 {
        // Independent of the sin/cos in `compose_look`: forward × up.
        (Quat::from_rotation_y(heading) * Vec3::NEG_Z).cross(Vec3::Y)
    }

    #[test]
    fn aim_aligned_strafe_adds_nothing() {
        // 90° unmounted strafe already puts the head on the aim. Looking there adds no second turn.
        let (spine, head) = armed_shares(-FRAC_PI_2, false);
        let extra = look_yaw_extra(spine, head, -FRAC_PI_2);
        assert!(
            extra.abs() < 1e-5,
            "target on the aim must not stack, extra {extra}"
        );
        assert!((spine + head + extra + FRAC_PI_2).abs() < 1e-5);
    }

    #[test]
    fn standing_forty_degrees_is_forty() {
        let deg = 40.0_f32.to_radians();
        let extra = look_yaw_extra(0.0, 0.0, deg);
        assert!((extra - deg).abs() < 1e-5, "{extra}");
        let extra = look_yaw_extra(0.0, 0.0, -deg);
        assert!((extra + deg).abs() < 1e-5, "{extra}");
    }

    #[test]
    fn a_target_behind_holds_the_neck_stop() {
        assert!((look_yaw_extra(0.0, 0.0, PI) - NECK_YAW).abs() < 1e-5);
        // Just off the rear seam on the other side. −π wraps to +π, so use a hair under it.
        assert!((look_yaw_extra(0.0, 0.0, -3.0) + NECK_YAW).abs() < 1e-5);
    }

    #[test]
    fn strafe_share_plus_look_stays_inside_the_neck() {
        // A gap of π caps the twist at 45° + 45°. The look may finish at the neck stop, not at
        // 45° plus another full neck.
        let (spine, head) = armed_shares(PI, false);
        let extra = look_yaw_extra(spine, head, PI);
        let total = head + extra;
        assert!(
            total.abs() <= NECK_YAW + 1e-5,
            "head total {total} passed the neck"
        );
        assert!((total - NECK_YAW).abs() < 1e-4);
        assert!(
            extra.abs() < FRAC_PI_4,
            "the extra must be the remainder, not a second neck turn ({extra})"
        );
    }

    #[test]
    fn mounted_aim_reaches_the_neck_and_not_past_it() {
        // Mounted, the spine stays put and the head share is 45° of a 90° gap, short of the aim.
        // The look closes up to the neck and stops.
        let (spine, head) = armed_shares(-FRAC_PI_2, true);
        assert_eq!(spine, 0.0);
        let extra = look_yaw_extra(spine, head, -FRAC_PI_2);
        let total = head + extra;
        assert!((total + NECK_YAW).abs() < 1e-4, "{total}");
        assert!(extra.abs() < NECK_YAW);
    }

    #[test]
    fn no_target_and_a_target_inside_the_body_rest() {
        let (yaw, pitch) = look_goal(Vec3::ZERO, Quat::IDENTITY, 1.5, None, 0.0, 0.0);
        assert_eq!((yaw, pitch), (0.0, 0.0));
        let (yaw, pitch) = look_goal(
            Vec3::ZERO,
            Quat::IDENTITY,
            1.5,
            Some((Vec3::new(0.1, 0.0, 0.1), 1.5)),
            0.0,
            0.0,
        );
        assert_eq!((yaw, pitch), (0.0, 0.0));
    }

    #[test]
    fn a_target_straight_ahead_adds_nothing_and_one_on_the_right_is_negative() {
        let (yaw, pitch) = look_goal(
            Vec3::ZERO,
            Quat::IDENTITY,
            1.5,
            Some((Vec3::new(0.0, 0.0, -8.0), 1.5)),
            0.0,
            0.0,
        );
        assert!(yaw.abs() < 1e-5 && pitch.abs() < 1e-5, "{yaw} {pitch}");

        // Identity faces −Z. +X is the character's right: forward × up.
        let fwd = Vec3::NEG_Z;
        let to_right = Vec3::X;
        assert!(fwd.cross(Vec3::Y).dot(to_right) > 0.9);
        let (yaw, pitch) = look_goal(
            Vec3::ZERO,
            Quat::IDENTITY,
            1.5,
            Some((Vec3::new(8.0, 0.0, 0.0), 1.5)),
            0.0,
            0.0,
        );
        assert!((yaw + NECK_YAW).abs() < 1e-4, "a target on the right yaws negative, {yaw}");
        assert!(pitch.abs() < 1e-5);
    }

    #[test]
    fn a_turned_body_looks_along_its_own_forward() {
        // Yaw −π/2 faces +X. A target further along +X is straight ahead.
        let body_rot = Quat::from_rotation_y(-FRAC_PI_2);
        let fwd = body_rot * Vec3::NEG_Z;
        assert!(fwd.x > 0.9 && fwd.z.abs() < 1e-4, "{fwd}");
        let (yaw, _) = look_goal(
            Vec3::ZERO,
            body_rot,
            1.5,
            Some((Vec3::new(10.0, 0.0, 0.0), 1.5)),
            0.0,
            0.0,
        );
        assert!(yaw.abs() < 1e-4, "{yaw}");

        // +Z is that body's right (forward × up). The neck stops there.
        let right = fwd.cross(Vec3::Y);
        assert!(right.z > 0.9, "{right}");
        let (yaw, _) = look_goal(
            Vec3::ZERO,
            body_rot,
            1.5,
            Some((Vec3::new(0.0, 0.0, 10.0), 1.5)),
            0.0,
            0.0,
        );
        assert!(yaw < 0.0, "right of the body is a negative yaw, {yaw}");
        assert!((yaw + NECK_YAW).abs() < 1e-4, "{yaw}");
    }

    #[test]
    fn pitch_looks_up_toward_a_high_target_and_stops() {
        let (_, pitch) = look_goal(
            Vec3::ZERO,
            Quat::IDENTITY,
            1.5,
            Some((Vec3::new(0.0, 5.0, -5.0), 1.5)),
            0.0,
            0.0,
        );
        // Eyes at 1.5, target head at 6.5, five yards up and five ahead: a 45° ask, held at the stop.
        assert!((pitch - NECK_PITCH).abs() < 1e-4, "{pitch}");
        let (_, pitch) = look_goal(
            Vec3::ZERO,
            Quat::IDENTITY,
            1.5,
            Some((Vec3::new(0.0, 0.0, -10.0), 0.5)),
            0.0,
            0.0,
        );
        assert!(pitch < 0.0, "a lower head nods down, {pitch}");
        assert!(look_pitch(0.2, 10.0) > 0.0);
        assert_eq!(look_pitch(4.0, 0.2), 0.0);
    }

    #[test]
    fn the_heading_right_matches_forward_cross_up() {
        for heading in [-1.2_f32, -0.4, 0.0, 0.7, 2.0] {
            let formula = Vec3::new(heading.cos(), 0.0, -heading.sin());
            assert!(
                formula.distance(right_of(heading)) < 1e-5,
                "{heading}: {formula} vs {}",
                right_of(heading)
            );
        }
    }

    #[test]
    fn yaw_extra_turns_about_unit_up_when_the_bone_is_rolled() {
        let g = Quat::from_rotation_x(0.7);
        let out = compose_look(g, g, -0.5, 0.0, -0.5);
        let turned = (out * g.inverse()) * Vec3::NEG_Z;
        let expect = Quat::from_rotation_y(-0.5) * Vec3::NEG_Z;
        assert!(
            turned.distance(expect) < 1e-4,
            "rolled bone yawed off unit up: {turned} vs {expect}"
        );
    }

    #[test]
    fn positive_pitch_lifts_the_face_including_when_already_turned() {
        let up = compose_look(Quat::IDENTITY, Quat::IDENTITY, 0.0, 0.35, 0.0);
        let fwd = up * Vec3::NEG_Z;
        assert!(fwd.y > 0.2 && fwd.z < -0.5, "a nod lifts −Z, {fwd}");

        // Already facing +X (heading −π/2). A positive nod lifts that face, it does not spin it.
        let heading = -FRAC_PI_2;
        let g = Quat::from_rotation_y(heading);
        let out = compose_look(g, g, 0.0, 0.35, heading);
        let fwd = out * Vec3::NEG_Z;
        assert!(fwd.y > 0.2 && fwd.x > 0.5, "turned nod: {fwd}");
    }

    #[test]
    fn a_resting_look_returns_the_bone_unchanged() {
        let local = Quat::from_rotation_y(0.2);
        assert_eq!(compose_look(local, local, 0.0, 0.0, 0.4), local);
    }

    #[test]
    fn the_neck_eases_and_a_cleared_target_eases_back() {
        let step = approach(0.0, NECK_YAW, YAW_RATE, 0.05);
        assert!((step - 0.225).abs() < 1e-5, "one frame must not snap, {step}");
        assert!((approach(0.1, 0.0, YAW_RATE, 0.05)).abs() < 1e-5);
        let back = approach(0.8, 0.0, YAW_RATE, 0.05);
        assert!(back < 0.8 && back > 0.5, "clearing the target eases home, {back}");
        // Across the rear seam the short arc passes through center, which is the only way
        // that stays inside the neck.
        let across = approach(1.0, -1.0, YAW_RATE, 0.05);
        assert!(across < 1.0 && across > 0.7, "{across}");
    }
}
