use super::*;
use crate::ecs::EntityManager;
use crate::prelude::*;
use crate::render::animated_model;
use crate::server::entity::*;

closure_system!(
    pub fn tick_animations(
        em: EntityManager<'_>,
        log: Read<CLogger>,
        assets: Read<AssetManager>,
        mut info_t: Write<animated_model::InfoTick>,
        delta: Read<Delta>,
        model: Read<Model>,
        mut animated_model: Write<AnimatedModel>,
        invalid: Read<InvalidPlacement>,
    ) {
        use ::model as exmodel;
        use cgmath::prelude::*;
        use rayon::prelude::*;
        use std::collections::hash_map::Entry;

        let log = log.get_component(Container::WORLD).expect("Missing logger");

        let delta = assume!(log.log, delta.get_component(Container::WORLD)).0;
        let assets = assume!(log.log, assets.get_component(Container::WORLD));
        let info_t = assume!(log.log, info_t.get_component_mut(Container::WORLD));

        for (_e, (ainfo, model_ty)) in em.group((&mut animated_model, &model)) {
            let model = if let Some(model) = info_t.models.get_mut(&model_ty.name) {
                model
            } else {
                continue;
            };

            // Due to `AnimationSet` being behind an arc its pointer
            // will have a constant value for the lifetime of the set.
            // We can use this fact to have the pointer be a key into
            // a map for quick lookups.
            let ptr = {
                let val: &FNVMap<_, _> = &ainfo.animation_set;
                val as *const _ as usize
            };
            if let Entry::Vacant(entry) = model.loaded_sets.entry(ptr) {
                entry.insert(ainfo.animation_set.clone());
                for ani in ainfo.animation_set.values() {
                    for animation in &ani.animations {
                        if !info_t.animations.contains_key(animation) {
                            let mut file = assume!(
                                log.log,
                                assets.open_from_pack(
                                    animation.module_key(),
                                    &format!("models/{}.uani", animation.resource())
                                )
                            );
                            let ani = assume!(log.log, exmodel::Animation::read_from(&mut file));
                            info_t.animations.insert(animation.clone(), ani);
                        }
                        if !model.animations.contains_key(animation) {
                            let ani = &info_t.animations[animation];
                            let mut channels = Vec::with_capacity(model.bones.len());
                            for _ in 0..model.bones.len() {
                                channels.push(animated_model::AnimationDetails::empty());
                            }

                            let mut bones = FNVMap::default();
                            let root_node =
                                animated_model::AniNode::convert(&mut bones, &ani.root_node);

                            for (k, v) in &ani.channels {
                                let id = match model.bones.get(k) {
                                    Some(val) => *val,
                                    None => continue,
                                };
                                let c = &mut channels[id];
                                *c = animated_model::AnimationDetails::new(
                                    v.position.clone(),
                                    v.rotation.clone(),
                                    v.scale.clone(),
                                );
                            }

                            for (name, id) in &model.bones {
                                let (pos, rot) = if let Some(tid) = bones.get(name) {
                                    let tnode = &root_node.nodes[*tid];
                                    let node = &model.root_node.nodes[*id];

                                    let (_s, r, p) = animated_model::decompose(&node.transform);
                                    let (_ts, tr, tp) = animated_model::decompose(&tnode.transform);

                                    (p - tp, tr.normalize() * r.normalize().conjugate())
                                } else {
                                    (
                                        cgmath::Vector3::zero(),
                                        cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0),
                                    )
                                };
                                let c = &mut channels[*id];
                                for p in &mut c.position.buckets {
                                    *p += pos;
                                }
                                for r in &mut c.rotation.buckets {
                                    *r = *r * rot;
                                }
                            }

                            let nani = animated_model::Animation {
                                duration: ani.duration,
                                channels,
                            };
                            model.animations.insert(animation.clone(), nani);
                        }
                    }
                }
            }
        }

        em.par_group(&mut animated_model)
            .par_iter()
            .for_each(|(e, ainfo)| {
                if invalid.get_component(e).is_none() {
                    ainfo.time += delta * ainfo.speed;
                }

                let animations = &info_t.animations;
                while ainfo.animation_queue.len() != 1 {
                    let (dur, remove) = if let Some(animation) = ainfo
                        .animation_set
                        .get(assume!(log.log, ainfo.animation_queue.first()))
                    {
                        let total_duration = animation
                            .animations
                            .iter()
                            .filter_map(|v| animations.get(v))
                            .map(|v| v.duration)
                            .sum();
                        (total_duration, ainfo.time >= total_duration)
                    } else {
                        (0.0, false)
                    };
                    if remove {
                        ainfo.time -= dur;
                        ainfo.animation_queue.remove(0);
                    } else {
                        break;
                    }
                }

                if let Some(animation) = ainfo
                    .animation_set
                    .get(assume!(log.log, ainfo.animation_queue.first()))
                {
                    let total_duration = animation
                        .animations
                        .iter()
                        .filter_map(|v| animations.get(v))
                        .map(|v| v.duration)
                        .sum();

                    if animation.should_loop {
                        if total_duration <= 0.0 {
                            ainfo.time = 0.0;
                        } else {
                            ainfo.time %= total_duration;
                        }
                    } else if ainfo.time >= total_duration && !animation.should_loop {
                        if let Some(ani) =
                            animations.get(assume!(log.log, animation.animations.last()))
                        {
                            // Loop the last animation
                            ainfo.time = (total_duration - ani.duration)
                                + ((ainfo.time - (total_duration - ani.duration)) % ani.duration);
                        }
                    }
                }
            });
    }
);

closure_system!(
    pub fn animate_movement_speed(
        em: EntityManager<'_>,
        mut model: Write<AnimatedModel>,
        speed: Read<MovementSpeed>,
        ams: Read<AnimationMovementSpeed>,
    ) {
        for (_e, (model, speed, ams)) in em.group((&mut model, &speed, &ams)) {
            model.speed = f64::from(speed.speed / speed.base_speed) * ams.modifier;
        }
    }
);

closure_system!(
    pub fn animate_walking(
        em: EntityManager<'_>,
        living: Read<Living>,
        mut model: Write<AnimatedModel>,
        mut speed: Write<MovementSpeed>,
        path: Read<pathfind::PathInfo>,
        frozen: Read<Frozen>,
    ) {
        for (e, (model, speed)) in em.group_mask((&mut model, &mut speed), |m| m.and(&living)) {
            if path.get_component(e).map_or(false, |v| v.is_moving()) {
                model.set_animation("walk");
            } else if model.current_animation().map_or(true, |v| v == "walk")
                || frozen.get_component(e).is_some()
            {
                model.set_animation("idle");
                speed.speed = speed.base_speed;
            }
        }
    }
);

closure_system!(
    pub fn lerp_target_pos(
        em: EntityManager<'_>,
        log: Read<CLogger>,
        delta: Read<Delta>,
        tiles: Read<LevelTiles>,
        rooms: Read<LevelRooms>,
        mut info: Write<pathfind::PathInfo>,
        mut speed: Write<MovementSpeed>,
        mut position: Write<Position>,
        mut target_pos: Write<TargetPosition>,
        mut target_rotation: Write<TargetRotation>,
        mut door: Write<Door>,
        mut target: Write<pathfind::Target>,
        adjust: Read<LagMovementAdjust>,
        mut catchup: Write<CatchupBuffer>,
    ) {
        // Hack to make sure pathfinding runs first
        pathfind::travel_path(
            &em,
            &log,
            &tiles,
            &rooms,
            &mut info,
            &mut speed,
            &mut position,
            &mut target_pos,
            &mut target_rotation,
            &mut door,
            &mut target,
            &adjust,
        );
        let log = log.get_component(Container::WORLD).expect("Missing logger");

        let delta = assume!(log.log, delta.get_component(Container::WORLD)).0 / 3.0;
        for (e, p) in em.group(&mut position) {
            let (remove_t, remove_c) = if let Some(t) = target_pos.get_component_mut(e) {
                if let Some(catchup) = catchup.remove_component(e) {
                    t.ticks -= catchup.catchup_time;
                }

                if t.ticks > 0.0 {
                    let am = t.ticks.min(delta);
                    p.x += ((t.x - p.x) / (t.ticks as f32)) * (am as f32);
                    p.y += ((t.y - p.y) / (t.ticks as f32)) * (am as f32);
                    p.z += ((t.z - p.z) / (t.ticks as f32)) * (am as f32);
                } else {
                    p.x = t.x;
                    p.y = t.y;
                    p.z = t.z;
                }
                t.ticks -= delta;
                if t.ticks <= 0.0 {
                    catchup.add_component(
                        e,
                        CatchupBuffer {
                            catchup_time: -t.ticks,
                        },
                    );
                    (true, false)
                } else {
                    (false, false)
                }
            } else if let Some(catchup) = catchup.get_component_mut(e) {
                catchup.catchup_time -= delta;
                (false, catchup.catchup_time <= 0.0)
            } else {
                (false, false)
            };
            if remove_t {
                target_pos.remove_component(e);
            }
            if remove_c {
                catchup.remove_component(e);
            }
        }
    }
);

closure_system!(
    pub fn lerp_target_rot(
        em: EntityManager<'_>,
        log: Read<CLogger>,
        delta: Read<Delta>,
        mut rot: Write<Rotation>,
        mut tar: Write<TargetRotation>,
    ) {
        let log = log.get_component(Container::WORLD).expect("Missing logger");

        let delta = assume!(log.log, delta.get_component(Container::WORLD)).0 / 3.0;
        for (e, r) in em.group_mask(&mut rot, |m| m.and(&tar)) {
            let remove = {
                let t = assume!(log.log, tar.get_component_mut(e));
                if t.ticks > 0.0 {
                    let am = t.ticks.min(delta) as f32;
                    r.rotation += ((t.rotation - r.rotation) / (t.ticks as f32)) * am;
                } else {
                    r.rotation = t.rotation;
                }
                t.ticks -= delta;
                t.ticks <= 0.0
            };
            if remove {
                tar.remove_component(e);
            }
        }
    }
);

closure_system!(
    pub fn animate_door(
        em: EntityManager<'_>,
        log: Read<CLogger>,
        position: Read<Position>,
        mut door: Write<Door>,
        mut model: Write<AnimatedModel>,
        mut audio: Write<AudioController>,
    ) {
        use std::cmp::max;
        let log = log.get_component(Container::WORLD).expect("Missing logger");
        let audio = assume!(log.log, audio.get_component_mut(Container::WORLD));
        for (_e, (animated_model, d, pos)) in em.group((&mut model, &mut door, &position)) {
            if d.open > 0 {
                d.open = max(d.open - 1, 0);
            }
            let open = d.open > 0;
            if open != d.was_open {
                d.was_open = open;
                if open {
                    animated_model.queue_animation("opening");
                    animated_model.queue_animation("open");
                    // TODO: Allow changing
                    audio.play_sound_at(
                        ResourceKey::new("base", "door_open"),
                        (pos.x as f32, pos.z as f32),
                    );
                } else {
                    animated_model.queue_animation("closing");
                    animated_model.queue_animation("closed");
                    // TODO: Allow changing
                    audio.play_sound_at(
                        ResourceKey::new("base", "door_close"),
                        (pos.x as f32, pos.z as f32),
                    );
                }
            }
            if open {
                d.open_time += 1;
            } else {
                d.open_time = 0;
            }
        }
    }
);

closure_system!(
    pub fn fade_lifetime(
        em: EntityManager<'_>,
        lifetime: Read<Lifetime>,
        fade: Read<FadeOverLife>,
        mut color: Write<Color>,
    ) {
        for (_e, (lifetime, color)) in em.group_mask((&lifetime, &mut color), |m| m.and(&fade)) {
            color.color.3 = ((lifetime.time as f32 / lifetime.max_life as f32) * 255.0) as u8;
        }
    }
);

closure_system!(
    pub fn tick_emotes(
        em: EntityManager<'_>,
        log: Read<CLogger>,
        mut emotes: Write<IconEmote>,
        mut icon: Write<Icon>,
        mut pos: Write<Position>,
        mut lifetime: Write<Lifetime>,
        mut fade: Write<FadeOverLife>,
        mut color: Write<Color>,
        mut follow: Write<Follow>,
    ) {
        let mask = emotes.mask().and(&pos);

        let log = log.get_component(Container::WORLD).expect("Missing logger");

        for e in em.iter_mask(&mask).collect::<Vec<_>>() {
            let emotes = assume!(log.log, emotes.get_component_mut(e));
            if let Some(cur) = emotes.icons.first().cloned().map(|v| v.1) {
                if emotes.current == Some(cur) {
                    continue;
                }
                emotes.current = Some(cur);
                let loc = {
                    let pos = assume!(log.log, pos.get_component(e));
                    (pos.x, pos.y, pos.z)
                };

                let i = em.new_entity();
                icon.add_component(
                    i,
                    Icon {
                        texture: match cur {
                            Emote::Confused => assets::ResourceKey::new("base", "icons/confuse"),
                            Emote::Paid => assets::ResourceKey::new("base", "icons/money"),
                        },
                        size: (0.3, 0.3),
                    },
                );
                pos.add_component(
                    i,
                    Position {
                        x: loc.0,
                        y: loc.1,
                        z: loc.2,
                    },
                );
                lifetime.add_component(i, Lifetime::new(60));
                fade.add_component(i, FadeOverLife);
                color.add_component(
                    i,
                    Color {
                        color: (255, 255, 255, 255),
                    },
                );
                follow.add_component(
                    i,
                    Follow {
                        target: e,
                        offset: (0.0, 1.2, 0.0),
                    },
                );
            } else {
                emotes.current = None;
            }
        }
    }
);

closure_system!(
    pub fn remove_attachments(em: EntityManager<'_>, attachment: Read<AttachedTo>) {
        for (e, attachment) in em.group(&attachment) {
            if !em.is_valid(attachment.target) {
                em.remove_entity(e);
            }
        }
    }
);

closure_system!(
    pub fn remove_attachments_room(
        em: EntityManager<'_>,
        attachment: Read<AttachedTo>,
        room_owned: Read<RoomOwned>,
        required: Read<RequiresRoom>,
    ) {
        for (e, attachment) in em.group_mask(&attachment, |m| m.and(&required).and(&room_owned)) {
            if room_owned.get_component(e).map(|v| v.room_id)
                != room_owned
                    .get_component(attachment.target)
                    .map(|v| v.room_id)
            {
                em.remove_entity(e);
            }
        }
    }
);
