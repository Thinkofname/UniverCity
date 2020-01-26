use super::*;

mod room;
pub use self::room::*;
mod entity;
pub use self::entity::*;
mod student;
pub use self::student::*;

// No need to smooth movement on the server, just jump to the target
closure_system!(pub fn no_lerp_target_pos(
    em: EntityManager<'_>,
    log: Read<CLogger>,
    tiles: Read<level::LevelTiles>,
    rooms: Read<level::LevelRooms>,
    mut info: Write<pathfind::PathInfo>,
    mut speed: Write<MovementSpeed>,
    mut position: Write<Position>,
    mut target_pos: Write<TargetPosition>,
    mut target_rotation: Write<TargetRotation>,
    mut door: Write<Door>,
    mut target: Write<pathfind::Target>,
    adjust: Read<LagMovementAdjust>,
    mut catchup: Write<CatchupBuffer>
) {
    // Hack to make sure pathfinding runs first
    pathfind::travel_path(
        &em,
        &log, &tiles, &rooms,
        &mut info, &mut speed, &mut position,
        &mut target_pos, &mut target_rotation,
        &mut door, &mut target, &adjust
    );

    for (e, p) in em.group(&mut position) {
        let (remove_t, remove_c) = if let Some(t) = target_pos.get_component_mut(e) {
            if let Some(catchup) = catchup.remove_component(e) {
                t.ticks -= catchup.catchup_time;
            }

            if t.ticks > 0.0 {
                let am = 1.0f64.min(t.ticks) as f32;
                p.x += ((t.x - p.x) / (t.ticks as f32)) * am;
                p.y += ((t.y - p.y) / (t.ticks as f32)) * am;
                p.z += ((t.z - p.z) / (t.ticks as f32)) * am;
            } else {
                p.x = t.x;
                p.y = t.y;
                p.z = t.z;
            }

            t.ticks -= 1.0;
            if t.ticks <= 0.0 {
                catchup.add_component(e, CatchupBuffer {
                    catchup_time: -t.ticks,
                });
                (true, false)
            } else { (false, false) }
        } else if let Some(catchup) = catchup.get_component_mut(e) {
            catchup.catchup_time -= 1.0;
            (false, catchup.catchup_time <= 0.0)
        } else { (false, false) };
        if remove_t {
            target_pos.remove_component(e);
        }
        if remove_c {
            catchup.remove_component(e);
        }
    }
});

// No need to smooth rotation on the server, just jump to the target
closure_system!(pub fn no_lerp_target_rot(
    em: EntityManager<'_>,
    log: Read<CLogger>,
    mut rot: Write<Rotation>,
    mut tar: Write<TargetRotation>
) {
    let log = log.get_component(Container::WORLD).expect("Missing logger");
    for (e, r) in em.group_mask(&mut rot, |m| m.and(&tar)) {
        let remove = {
            let t = assume!(log.log, tar.get_component_mut(e));
            if t.ticks > 0.0 {
                let am = 1.0f64.min(t.ticks) as f32;
                r.rotation += ((t.rotation - r.rotation) / (t.ticks as f32)) * am;
            } else {
                r.rotation = t.rotation;
            }
            t.ticks -= 1.0;
            t.ticks <= 0.0
        };
        if remove {
            tar.remove_component(e);
        }
    }
});

closure_system!(pub fn open_door_server(em: EntityManager<'_>, mut door: Write<Door>) {
    use std::cmp::max;
    for (_e, d) in em.group(&mut door) {
        if d.open > 0 {
            d.open = max(d.open - 1, 0);
        }
        let open = d.open > 0;
        if open != d.was_open {
            d.was_open = open;
        }
        if open {
            d.open_time += 1;
        } else {
            d.open_time = 0;
        }
    }
});

closure_system!(pub fn lifetime_sys(em: EntityManager<'_>, mut lifetime: Write<Lifetime>) {
    for (e, lifetime) in em.group(&mut lifetime) {
        lifetime.time -= 1;
        if lifetime.time <= 0 {
            em.remove_entity(e);
        }
    }
});

closure_system!(pub fn velocity_sys(
    em: EntityManager<'_>,
    mut position: Write<Position>,
    mut velocity: Write<Velocity>
) {
    for (_e, (pos, vel)) in em.group((&mut position, &mut velocity)) {
        let initial_vel = vel.velocity;
        pos.x += initial_vel.0;
        pos.y += initial_vel.1;
        pos.z += initial_vel.2;

        vel.velocity.0 -= vel.friction.0;
        if vel.velocity.0.signum() != initial_vel.0.signum() {
            vel.velocity.0 = 0.0;
            vel.friction.0 = 0.0;
        }

        vel.velocity.1 -= vel.friction.1;
        if vel.velocity.1.signum() != initial_vel.1.signum() {
            vel.velocity.1 = 0.0;
            vel.friction.1 = 0.0;
        }

        vel.velocity.2 -= vel.friction.2;
        if vel.velocity.2.signum() != initial_vel.2.signum() {
            vel.velocity.2 = 0.0;
            vel.friction.2 = 0.0;
        }
    }
});

closure_system!(pub fn tick_emotes(
    em: EntityManager<'_>,
    mut emotes: Write<IconEmote>
) {
    for (_e, icons) in em.group(&mut emotes) {
        if icons.icons.is_empty() {
            continue;
        }
        icons.time += 1;
        if icons.time >= 60 {
            icons.icons.remove(0);
            icons.time = 0;
        }
    }
});

closure_system!(
/// Allows enities to follow other entities
pub fn follow_sys(
    em: EntityManager<'_>,
    log: Read<CLogger>,
    mut position: Write<Position>,
    follow: Read<Follow>
) {
    let log = log.get_component(Container::WORLD).expect("Missing logger");
    for (e, follow) in em.group_mask(&follow, |m| m.and(&position)) {
        let target = if let Some(t) = position.get_component(follow.target) {
            (t.x, t.y, t.z)
        } else {
            continue;
        };
        let pos = assume!(log.log, position.get_component_mut(e));
        pos.x = target.0 + follow.offset.0;
        pos.y = target.1 + follow.offset.1;
        pos.z = target.2 + follow.offset.2;
    }
});

closure_system!(
/// Allows enities to follow other entities
pub fn follow_rot(
    em: EntityManager<'_>,
    log: Read<CLogger>,
    mut rotation: Write<Rotation>,
    follow: Read<Follow>
) {
    let log = log.get_component(Container::WORLD).expect("Missing logger");
    for (e, follow) in em.group_mask(&follow, |m| m.and(&rotation)) {
        let target = if let Some(t) = rotation.get_component(follow.target) {
            t.rotation
        } else {
            continue;
        };
        let rot = assume!(log.log, rotation.get_component_mut(e));
        rot.rotation = target;
    }
});

closure_system!(pub(super) fn pay_staff(
    em: EntityManager<'_>,
    log: Read<CLogger>,
    day: Read<DayTick>,
    mut players: Write<crate::PlayerInfoMap>,
    mut paid: Write<Paid>,
    owned: Read<Owned>,
    mut emotes: Write<IconEmote>,
    frozen: Read<Frozen>
) {
    let log = log.get_component(Container::WORLD).expect("Missing logger");
    let day = assume!(log.log, day.get_component(Container::WORLD));
    let players = assume!(log.log, players.get_component_mut(Container::WORLD));

    for (e, (paid, owned)) in em.group_mask((&mut paid, &owned), |m| m.and_not(&frozen)) {
        if paid.last_payment.map_or(false, |v| day.day.wrapping_sub(v) < 3) {
            continue;
        }
        paid.last_payment = Some(day.day);

        let player = assume!(log.log, players.get_mut(&owned.player_id));
        paid.wanted_cost += paid.cost / 50; // 2% increase
        let money = paid.cost;
        player.change_money(-money);
        IconEmote::add(&mut emotes, e, Emote::Paid);
    }
});

closure_system!(pub(crate) fn tick_student_stats(
    em: EntityManager<'_>,
    mut vars: Write<StudentVars>,
    frozen: Read<Frozen>,
    owned: Read<Owned>,
    log: Read<CLogger>
) {
    let world = Container::WORLD;
    let log = log.get_component(world).expect("Missing logger");

    for (e, _owned) in em.group_mask(
        &owned,
        |m| m.and_not(&frozen).and(&vars)
    ) {
        let vars = assume!(log.log, vars.get_custom(e));
        let hunger = vars.get_stat(Stats::STUDENT_HUNGER);
        let happiness = vars.get_stat(Stats::STUDENT_HAPPINESS);
        vars.set_stat(Stats::STUDENT_HAPPINESS, happiness
            - (1.0 / (20.0 * 60.0 * 6.0))
                * (1.5 - hunger)
        );
        vars.set_stat(Stats::STUDENT_HUNGER, hunger
            - 1.0 / (20.0 * 60.0 * 4.0)
        );
    }
});


closure_system!(pub(crate) fn tick_professor_stats(
    em: EntityManager<'_>,
    mut vars: Write<ProfessorVars>,
    frozen: Read<Frozen>,
    owned: Read<Owned>,
    mut player_info: Write<FNVMap<PlayerId, PlayerInfo>>,
    log: Read<CLogger>,
    paid: Read<Paid>
) {
    use crate::player::IssueState;
    use std::collections::hash_map::Entry;
    let world = Container::WORLD;
    let log = log.get_component(world).expect("Missing logger");

    let player_info = assume!(log.log, player_info.get_component_mut(world));

    for (e, owned) in em.group_mask(
        &owned,
        |m| m.and_not(&frozen).and(&vars)
    ) {
        let vars = assume!(log.log, vars.get_custom(e));

        let happiness = vars.get_stat(Stats::PROFESSOR_HAPPINESS);
        let fatigue = vars.get_stat(Stats::PROFESSOR_FATIGUE);
        let job_satisfaction = vars.get_stat(Stats::PROFESSOR_JOB_SATISFACTION);

        vars.set_stat(Stats::PROFESSOR_HAPPINESS, happiness
            - (1.0 / (20.0 * 60.0 * 10.0))
                // When sleepy happiness depletes faster
                * (1.5 - fatigue)
        );
        vars.set_stat(Stats::PROFESSOR_FATIGUE, fatigue
            - 1.0 / (20.0 * 60.0 * 12.0)
        );

        let factor = if let Some(paid) = paid.get_component(e) {
            let factor = (paid.wanted_cost - paid.cost).0 as f32 / (paid.cost.0 as f32);
            1.0 + factor * 5.0
        } else {
            1.0
        };

        let happiness = vars.get_stat(Stats::PROFESSOR_HAPPINESS);
        vars.set_stat(Stats::PROFESSOR_JOB_SATISFACTION, job_satisfaction
            - (1.0 / (20.0 * 60.0 * 60.0))
                // When unhappy, drain faster
                * (1.5 - happiness)
                // When the payment is too low, drain faster
                * factor
        );
        let job_satisfaction = vars.get_stat(Stats::PROFESSOR_JOB_SATISFACTION);

        let player = assume!(log.log, player_info.get_mut(&owned.player_id));

        if job_satisfaction < 0.2 {
            // Ask for raise
            if let Entry::Vacant(entry) = player.staff_issues.entry(e) {
                entry.insert(IssueState::WantsPay);
                // Prevent instantly quiting
                vars.set_stat(Stats::PROFESSOR_JOB_SATISFACTION, 0.2);
            }
        }
        if job_satisfaction < 0.05 {
            // Quit
            player.staff_issues.insert(e, IssueState::Quit);
        }
    }
});

closure_system!(pub(crate) fn rest_staff(
    em: EntityManager<'_>,
    rooms: Read<LevelRooms>,
    auto_rest: Read<AutoRest>,
    frozen: Read<Frozen>,
    pos: Read<Position>,
    owned: Read<Owned>,
    room_owned: Read<RoomOwned>,
    log: Read<CLogger>,
    mut rc: Write<RoomController>,
    mut goto_room: Write<GotoRoom>
) {
    let world = Container::WORLD;
    let log = log.get_component(world).expect("Missing logger");

    let rooms = assume!(log.log, rooms.get_component(Container::WORLD));

    let staff_room = assets::ResourceKey::new("base", "staff_room");

    for (e, (owned, pos)) in em.group_mask(
        (&owned, &pos),
        |m| m
            .and(&auto_rest)
            .and_not(&room_owned)
            .and_not(&frozen)
            .and_not(&goto_room)
    ) {
        let owner = owned.player_id;
        let nearest_sr = rooms.room_ids()
            .map(|v| rooms.get_room_info(v))
            .filter(|v| v.state.is_done())
            .filter(|v| v.key == staff_room)
            .filter(|v| v.owner == owner)
            .filter(|v| {
                let rc = assume!(log.log, rc.get_component(v.controller));
                rc.visitors.len() + rc.waiting_list.len() + rc.potential_list.len() < rc.capacity
            })
            .min_by(|a, b| {
                let adx = ((a.area.min.x + a.area.max.x) / 2) - pos.x as i32;
                let adz = ((a.area.min.y + a.area.max.y) / 2) - pos.z as i32;
                let a_dist = adx * adx + adz * adz;
                let bdx = ((b.area.min.x + b.area.max.x) / 2) - pos.x as i32;
                let bdz = ((b.area.min.y + b.area.max.y) / 2) - pos.z as i32;
                let b_dist = bdx * bdx + bdz * bdz;
                a_dist.cmp(&b_dist)
            });
        if let Some(room) = nearest_sr {
            goto_room.add_component(e, GotoRoom::new(&log.log, e, rooms, &mut rc, room.id));
        }
    }
});

closure_system!(pub(crate) fn require_room(
    em: EntityManager<'_>,
    log: Read<CLogger>,
    rooms: Read<LevelRooms>,
    room_owned: Read<RoomOwned>,
    requires: Read<RequiresRoom>
) {
    let log = log.get_component(Container::WORLD).expect("Missing logger");
    let rooms = assume!(log.log, rooms.get_component(Container::WORLD));

    for (e, ro) in em.group_mask(&room_owned, |m| m.and(&requires)) {
        if !rooms.try_room_info(ro.room_id)
            .map_or(false, |v| v.state.is_done())
        {
            em.remove_entity(e);
        }
    }
});
