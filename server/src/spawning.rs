
use crate::prelude::*;

const SPAWN_CHECK_INTERVAL: u32 = 20 * 60; // 1 Minute

pub struct Spawner {
    pub info: Vec<SpawnInfo>,
    spawn_check: u32,
    log: Logger,
}

pub struct SpawnInfo {
    pub id: PlayerId,

    pub required_students: u32,

    inspector_cooldown: i32,
}

impl Spawner {
    pub fn new(log: &Logger, players: &[PlayerId]) -> Spawner {
        Spawner {
            log: log.new(o!("source" => "spawner")),
            info: players.iter()
                .map(|v| SpawnInfo {
                    id: *v,
                    required_students: 0,
                    inspector_cooldown: 20 * 60 * 5,
                })
                .collect(),
            spawn_check: 0,
        }
    }

    pub(crate) fn handle_spawning(
        &mut self,
        assets: &AssetManager,
        player_info: &crate::PlayerInfoMap,
        level: &mut Level,
        entities: &mut Container,
        scripting: &script::Engine,
    ) {
        use rand::{Rng, thread_rng};
        use std::cmp::{min, max};
        use lua::{Ref, Table};
        use rand::seq::SliceRandom;

        self.spawn_check += 1;
        if self.spawn_check >= SPAWN_CHECK_INTERVAL {
            let len = self.info.len() as u32;
            let player = &mut self.info[(self.spawn_check - SPAWN_CHECK_INTERVAL) as usize];
            if self.spawn_check >= SPAWN_CHECK_INTERVAL + len - 1 {
                self.spawn_check = 0;
            }
            let log = &self.log;

            // Firstly work out how many students the player has
            let students = entities.with(|em: EntityManager<'_>, owned: Read<Owned>, student: Read<StudentController>| {
                let mask = student.mask().and(&owned);
                em.iter_mask(&mask)
                    .filter(|e| assume!(log, owned.get_component(*e)).player_id == player.id)
                    .count()
            });

            // Then work out how many students their UniverCity can support
            let capacity: usize = level.room_ids()
                .into_iter()
                .map(|v| level.get_room_info(v))
                .filter(|v| v.owner == player.id)
                .filter(|v| !v.controller.is_invalid())
                .filter(|v| assume!(log, assets.loader_open::<room::Loader>(v.key.borrow())).used_for_teaching)
                .filter_map(|v| entities.get_component::<RoomController>(v.controller))
                .map(|v| v.capacity)
                .sum();


            // Allow the spawning of a subset of the difference between supported
            // and owned.

            let info = &player_info[&player.id];
            let weight = 3.0 + (f32::from(info.rating) / 30000.0) * 1.5;
            let diff = (capacity as f32 * weight).round() as u32;
            let diff = diff as i32 - students as i32;
            let diff = if diff < 0 {
                0
            } else {
                diff as u32
            };
            player.required_students = diff;
        }

        let mut rng = thread_rng();
        let student = ResourceKey::new("base", "student");
        let inspector = ResourceKey::new("base", "inspector");
        let vip = ResourceKey::new("base", "vip");

        let generate = Ref::new_string(scripting, "generate");
        let base = Ref::new_string(scripting, "base");
        let student_creation = Ref::new_string(scripting, "student_creation");

        for player in &mut self.info {
            player.inspector_cooldown -= 1;
            if player.inspector_cooldown <= 0 && rng.gen_bool(1.0 / 500.0) {
                player.inspector_cooldown = rng.gen_range(20 * 60 * 5, 20 * 60 * 15);

                let ety = assume!(self.log, assets.loader_open::<Loader<ServerComponent>>(if rng.gen_bool(1.0 / 10.0) {
                    vip.borrow()
                } else {
                    inspector.borrow()
                }));
                let variant = rng.gen_range(0, ety.variants.len());
                let e = ety.create_entity(entities, variant, None);

                let (tx, ty) = gen_spawn(level, &mut rng);
                {
                    let pos = assume!(self.log, entities.get_component_mut::<Position>(e));
                    pos.x = tx as f32 + 0.5;
                    pos.y = 0.2;
                    pos.z = ty as f32 + 0.5;
                }
                entities.add_component(e, Owned {
                    player_id: player.id,
                });
            }
            if player.required_students > 0
                && rng.gen_bool(1.0 / f64::from(max(1, min(150, (LESSON_LENGTH as u32 * 4 * 3) / player.required_students))))
            {
                #[derive(Serialize)]
                struct PlayerInfo {
                }
                let player_script_info = assume!(self.log, lua::to_table(scripting, &PlayerInfo {
                }));

                #[derive(Deserialize)]
                struct StudentInfo {
                    #[serde(default)]
                    first_name: Option<String>,
                    #[serde(default)]
                    surname: Option<String>,
                    #[serde(default)]
                    variant: Option<usize>,
                    #[serde(default)]
                    vars: FNVMap<String, VarValue>,
                    money: UniDollar,
                }

                #[derive(Deserialize, Debug)]
                #[serde(untagged)]
                enum VarValue {
                    Bool(bool),
                    Float(f64),
                }

                player.required_students -= 1;
                let (t_x, t_y) = gen_spawn(level, &mut rng);
                let ety = assume!(self.log, assets.loader_open::<Loader<ServerComponent>>(student.borrow()));

                let student_info = match scripting.with_borrows()
                    .borrow_mut(entities)
                    .invoke_function::<_, Ref<Table>>("invoke_module_method", (
                        base.clone(),
                        student_creation.clone(),
                        generate.clone(),
                        player_script_info.clone(),
                    )) {
                    Err(err) => {
                        error!(self.log, "Failed to generate student"; "error" => % err);
                        continue;
                    },
                    Ok(val) => val,
                };
                let student_info: StudentInfo = match lua::from_table(&student_info) {
                    Ok(val) => val,
                    Err(err) => {
                        error!(self.log, "Failed to generate student"; "error" => % err);
                        continue;
                    },
                };

                let e_variant = student_info.variant
                    .unwrap_or_else(|| rng.gen_range(0, ety.variants.len()));

                let name = if let (Some(f), Some(s)) = (
                    student_info.first_name,
                    student_info.surname,
                ) {
                    ((*f).into(), (*s).into())
                } else {
                    let variant = &ety.variants[e_variant];
                    (
                        variant.name_list.first.choose(&mut rng).cloned().unwrap_or_else(|| "Missing".into()),
                        variant.name_list.second.choose(&mut rng).cloned().unwrap_or_else(|| "Name".into()),
                    )
                };

                let e = ety.create_entity(entities, e_variant, Some(name));
                {
                    let pos = assume!(self.log, entities.get_component_mut::<Position>(e));
                    pos.x = t_x as f32 + 0.5;
                    pos.y = 0.2;
                    pos.z = t_y as f32 + 0.5;
                }
                entities.add_component(e, Owned {
                    player_id: player.id,
                });
                entities.add_component(e, Money {
                    money: student_info.money,
                });
                let vars = assume!(self.log, entities.get_custom::<choice::StudentVars>(e));

                for (k, v) in student_info.vars {
                    match v {
                        VarValue::Bool(v) => vars.set_boolean(&k, v),
                        VarValue::Float(v) => if vars.get_type(&k) == Some(choice::Type::Integer) {
                            vars.set_integer(&k, v as i32)
                        } else {
                            vars.set_float(&k, v as f32)
                        },
                    };
                }
            }
        }
    }
}

fn gen_spawn<R>(level: &Level, rng: &mut R) -> (i32, i32)
    where R: ::rand::Rng
{
    use rand::seq::SliceRandom;

    let road = ResourceKey::new("base", "external/road");
    let rooms = level.room_ids();
    let spawn_point = rooms.iter()
        .map(|v| level.get_room_info(*v))
        .filter(|v| v.key == road)
        .filter(|v| v.area.min.x == 0 || v.area.min.y == 0
            || v.area.max.x + 1 == level.width as i32
            || v.area.max.y + 1 == level.height as i32)
        .collect::<Vec<_>>();

    let spawn_point = assume!(level.log, spawn_point.choose(rng));
    let mut tx = spawn_point.area.min.x + (spawn_point.area.width() / 2);
    let mut ty = spawn_point.area.min.y + (spawn_point.area.height() / 2);
    if spawn_point.area.min.x == 0 {
        tx = 0;
    } else if spawn_point.area.max.x + 1 == level.width as i32 {
        tx = spawn_point.area.max.x;
    }
    if spawn_point.area.min.y == 0 {
        ty = 0;
    } else if spawn_point.area.max.y + 1 == level.height as i32 {
        ty = spawn_point.area.max.y;
    }

    (tx, ty)
}