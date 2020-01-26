//! Save format structures and handling

use crate::prelude::*;

pub mod filesystem;
mod conv;

use std::io::{SeekFrom, Write as IoWrite, Read as IoRead, Seek, BufReader};
use serde_cbor;
use serde_transcode;
use serde::Deserialize;
use lua::{self, Ref, Table, Unknown};
use crate::script;
#[cfg(feature = "steam")]
use steamworks;
use byteorder::{WriteBytesExt, ReadBytesExt, LittleEndian};
use crate::mission;
use crate::script_room;

use crate::packet::HistoryEntry;
use crate::player::PlayerConfig;
use crate::room::RoomState;
use self::filesystem::*;

/// The version number currently used by this version of the
/// game.
pub const SAVE_VERSION: u32 = 6;

/// The size of the save icon
pub const SAVE_ICON_SIZE: (u32, u32) = (800, 600);

/// Marks the type of save file. Used to
/// filter saves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveType {
    /// A single player free play map.
    ///
    /// No mission will be active
    FreePlay,
    /// A single player map with a mission active.
    ///
    /// A mission script will be controlling this
    /// map.
    Mission,
    /// A multiplayer free play map.
    ///
    /// Cannot be played in single player. No mission
    /// will be active.
    ServerFreePlay,
}

impl SaveType {
    fn as_u32(self) -> u32 {
        match self {
            SaveType::FreePlay => 0,
            SaveType::Mission => 1,
            SaveType::ServerFreePlay => 2,
        }
    }
    fn from_u32(v: u32) -> Option<Self> {
        Some(match v {
            0 => SaveType::FreePlay,
            1 => SaveType::Mission,
            2 => SaveType::ServerFreePlay,
            _ => return None,
        })
    }
}

/// Deletes the named save file
pub fn delete_save<F: FileSystem>(fs: &F, name: &str) -> UResult<()> {
    let path = format!("{}.usav", name);
    fs.delete(&path)?;
    Ok(())
}

/// Returns whether the save file can be loaded.
///
/// Returns an error if the file couldn't be loaded
/// for any reason. Returns true if the file can be
/// loaded with the current filter and false if it
/// cannot.
pub fn can_load<F: FileSystem>(
    fs: &F,
    name: &str,
    ty: SaveType,
) -> UResult<bool> {
    let path = format!("{}.usav", name);
    if !fs.exists(&path) {
        return Err(ErrorKind::NoSuchSave.into());
    }
    let mut f = BufReader::new(fs.read(&path)?);

    let version = f.read_u32::<LittleEndian>()?;
    match version {
        0 | 1 | 2 | 3 => bail!("Early alpha save version"),
        4 => bail!("Early Access 0.2.0 save version"),
        5 => bail!("Early Access 0.3.0 save version"),
        // Versions with converters
        // Current version
        SAVE_VERSION => {},
        _ => bail!("Unknown save version"),
    };
    let sty = SaveType::from_u32(f.read_u32::<LittleEndian>()?);

    Ok(sty == Some(ty))
}

/// Returns the icon for the save file if it has one
pub fn get_icon<F: FileSystem>(
    fs: &F,
    name: &str,
) -> Option<Vec<u8>>
{
    let path = format!("{}.usav", name);
    if !fs.exists(&path) {
        return None;
    }
    let mut f = BufReader::new(fs.read(&path).ok()?);
    let version = f.read_u32::<LittleEndian>().ok()?;
    if version < 4 {
        return None;
    }
    let _sty = f.read_u32::<LittleEndian>().ok()?;
    let len = f.read_i32::<LittleEndian>().ok()?;
    if len == -1 {
        None
    } else {
        let mut buf = vec![0; len as usize];
        f.read_exact(&mut buf).ok()?;
        Some(buf)
    }
}

/// Captures a screenshot of the current game to use
/// as an icon.
pub trait IconCapture {
    /// Captures a screenshot of the game and serializes it
    /// as a png image.
    fn capture(&self) -> Option<Vec<u8>>;
}

/// Saves the game's state with the given name
/// in the default location
pub(crate) fn save_game<F: FileSystem>(
    fs: &F,
    name: &str,
    ty: SaveType,
    players: &mut crate::PlayerInfoMap,
    level: &mut Level, entities: &mut Container,
    engine: &script::Engine,
    choices: &choice::Choices,
    running_choices: &script_room::RunningChoices,
    mission: Option<&mut mission::MissionController>,
    day_tick: &DayTick, icon: Option<&dyn IconCapture>,
) -> UResult<()>
{
    let log = level.log.clone();
    let assets = level.asset_manager.clone();
    let name = format!("{}.usav", name);

    let save_icon = icon.and_then(|v| v.capture());
    {
        let mut f = fs.write(&name)?;
        f.write_u32::<LittleEndian>(SAVE_VERSION)?;
        f.write_u32::<LittleEndian>(ty.as_u32())?;
        if let Some(icon) = save_icon {
            f.write_i32::<LittleEndian>(icon.len() as i32)?;
            f.write_all(&icon)?;
        } else {
            f.write_i32::<LittleEndian>(-1)?;
        }

        let players_sf = entities.with(|
            _em: EntityManager,
            network_id: Read<NetworkId>,
        | {
            SaveData::Players(players.iter()
                .map(|(k, v)| (*k, PlayerInfo {
                    name: v.name.clone(),
                    key: match v.key {
                        #[cfg(feature = "steam")]
                        player::PlayerKey::Steam(steam_id) => PlayerKey::Steam(steam_id.raw()),
                        #[cfg(not(feature = "steam"))]
                        player::PlayerKey::Username(ref name) => PlayerKey::Username(name.clone()),
                    },
                    money: v.money,
                    rating: v.rating,
                    history: v.history.clone().into(),
                    current_income: v.current_income,
                    current_outcome: v.current_outcome,
                    state: match v.state {
                        crate::player::State::BuildRoom{active_room} => PlayerState::BuildRoom {
                            active_room
                        },
                        crate::player::State::EditRoom{active_room} => PlayerState::EditRoom {
                            active_room
                        },
                        // Don't bother with this
                        crate::player::State::None | crate::player::State::EditEntity{..} => PlayerState::None,
                    },
                    config: v.config.clone(),
                    courses: v.courses.iter()
                        .map(|(id, v)| (*id, SavableCourse {
                            uid: v.uid,
                            name: v.name.clone(),
                            group: v.group.clone(),
                            cost: v.cost,
                            timetable: SavableCourse::conv_timetable(&network_id, &v.timetable),
                            deprecated: v.deprecated,
                        }))
                        .collect(),
                }))
                .collect())
        });

        serde_cbor::to_writer(&mut f, &players_sf)?;
        serde_cbor::to_writer(&mut f, &SaveData::GameState(GameState {
            day_tick: *day_tick,
        }))?;
        serde_cbor::to_writer(&mut f, &SaveData::Level(level.width, level.height))?;

        {
            let rooms = level.rooms.borrow();
            for (_id, room) in rooms.iter_rooms() {
                let room_info = SaveData::Room(RoomInfo {
                    key: room.key.clone(),
                    id: room.id,
                    owner: room.owner,
                    area: (
                        room.area.min.x,
                        room.area.min.y,
                        room.area.max.x,
                        room.area.max.y,
                    ),
                    state: room.state,
                    tile_update_state: room.tile_update_state.clone(),
                });
                serde_cbor::to_writer(&mut f, &room_info)?;
            }
            // TODO: Place these into the file in order of placement
            for (id, room) in rooms.iter_rooms() {
                let objs = if let Some(lvl) = room.building_level.as_ref() {
                        lvl.objects.iter()
                    } else { room.objects.iter() }
                    .filter_map(|v| v.as_ref())
                    .map(|v| &v.0)
                    .map(|v| ObjectInfo {
                        key: v.key.clone(),
                        position: (v.position.x, v.position.y),
                        rotation: v.rotation,
                        version: v.version,
                    });
                for obj in objs {
                    serde_cbor::to_writer(&mut f, &SaveData::Object(id, obj))?;
                }
            }

            entities.with(|
                em: EntityManager<'_>,
                living: Read<Living>,
                position: Read<Position>,
                target_position: Read<TargetPosition>,
                rotation: Read<Rotation>,
                target_rotation: Read<TargetRotation>,
                speed: Read<MovementSpeed>,
                room_owned: Read<RoomOwned>,
                owned: Read<Owned>,
                paid: Read<Paid>,
                tints: Read<Tints>,
                room_controller: Read<RoomController>,
                timetable: Read<TimeTable>,
                timetable_completed: Read<TimeTableCompleted>,
                timetable_start: Read<TimeTableStart>,
                activity: Read<Activity>,

                grades: Read<Grades>,
                idle: Read<Idle>,
                network_id: Read<NetworkId>,
                goto_room: Read<GotoRoom>,
                money: Read<Money>,

                mut student_vars: Write<StudentVars>,
                mut professor_vars: Write<ProfessorVars>,
                mut office_worker_vars: Write<OfficeWorkerVars>,
                mut janitor_vars: Write<JanitorVars>,

                frozen: Read<Frozen>,
                quitting: Read<Quitting>,
            | -> UResult<()> {
                for (e, living) in em.group_mask(&living, |m| m
                    .and_not(&frozen)
                    .and_not(&quitting)
                ) {
                    let info = EntityInfo {
                        key: living.key.clone(),
                        variant: living.variant,
                        name: (
                            (*living.name.0).to_owned(),
                            (*living.name.1).to_owned(),
                        ),
                        network_id: network_id.get_component(e).map(|v| v.0),
                        position: if let Some(tp) = target_position.get_component(e) {
                            Some(PositionInfo{ x: tp.x, y: tp.y, z: tp.z })
                        } else if let Some(p) = position.get_component(e) {
                            Some(PositionInfo{ x: p.x, y: p.y, z: p.z})
                        } else {
                            None
                        },
                        rotation: if let Some(tr) = target_rotation.get_component(e) {
                            Some(tr.rotation)
                        } else if let Some(r) = rotation.get_component(e) {
                            Some(r.rotation)
                        } else {
                            None
                        },
                        speed: if let Some(speed) = speed.get_component(e) {
                            Some(speed.base_speed)
                        } else {
                            None
                        },
                        room_owned: if let Some(ro) = room_owned.get_component(e) {
                            let room = level.get_room_info(ro.room_id);
                            if let Some(rc) = room_controller.get_component(room.controller) {
                                Some(RoomOwnedInfo {
                                    room_id: ro.room_id,
                                    kind: if rc.entities.iter().any(|o| *o == e) {
                                        OwnedKind::Owned
                                    } else {
                                        OwnedKind::Visitor
                                    },
                                    should_release_inactive: ro.should_release_inactive,
                                    active: ro.active,
                                })
                            } else {
                                None
                            }
                        } else {
                            None
                        },
                        owned: if let Some(o) = owned.get_component(e) {
                            Some(o.player_id)
                        } else {
                            None
                        },
                        paid: if let Some(paid) = paid.get_component(e) {
                            Some(PaidInfo {
                                cost: paid.cost,
                                wanted_cost: paid.wanted_cost,
                                last_payment: paid.last_payment,
                            })
                        } else {
                            None
                        },
                        tints: if let Some(tints) = tints.get_component(e) {
                            Some(tints.tints.clone())
                        } else {
                            None
                        },
                        vars: if let Some(vars) = student_vars.get_custom(e)
                            .map(|v| v.remove_type())
                            .or_else(|| professor_vars.get_custom(e)
                                .map(|v| v.remove_type()))
                            .or_else(|| office_worker_vars.get_custom(e)
                                .map(|v| v.remove_type()))
                            .or_else(|| janitor_vars.get_custom(e)
                                .map(|v| v.remove_type()))
                        {
                            Some(VarsInfo {
                                vars: vars.iter()
                                    .map(|(k, v)| (k.to_owned(), v))
                                    .collect(),
                            })
                        } else {
                            None
                        },
                        timetable: timetable.get_component(e).cloned(),
                        timetable_start: timetable_start.get_component(e).cloned(),
                        timetable_completed: timetable_completed.get_component(e).is_some(),
                        activity: activity.get_component(e).cloned(),
                        grades: grades.get_component(e).cloned(),
                        idle: if let Some(idle) = idle.get_component(e) {
                            Some(IdleInfo {
                                total_idle_time: idle.total_idle_time,
                                current_choice: idle.current_choice
                                    .and_then(|v| choices.student_idle.get_choice_name_by_index(v))
                                    .map(|v| v.into_owned()),
                            })
                        } else { None },
                        goto_room: goto_room.get_component(e).map(|v| GotoRoomInfo {
                            room_id: v.room_id,
                        }),
                        money: money.get_component(e).map(|v| MoneyInfo {
                            money: v.money,
                        }),
                    };
                    serde_cbor::to_writer(&mut f, &SaveData::Entity(info))?;
                }
                Ok(())
            })?;
        }

        for room_id in level.room_ids() {
            let ty = {
                let room = level.get_room_info(room_id);
                if room.controller.is_invalid() {
                    continue;
                }
                assume!(log, assets.loader_open::<room::Loader>(room.key.borrow()))
            };
            if let Some(controller) = ty.controller.as_ref() {
                let lua_room = crate::script_room::LuaRoom::from_room(&log, &*level.rooms.borrow(), entities, room_id, engine);
                let result = match engine.with_borrows()
                    .borrow_mut(entities)
                    .borrow_mut(players)
                    .invoke_function::<_, Ref<Table>>("invoke_module_method", (
                        Ref::new_string(engine, controller.module()),
                        Ref::new_string(engine, controller.resource()),
                        Ref::new_string(engine, "save"),
                        lua_room
                )) {
                    Ok(ret) => ret,
                    Err(err) => {
                        bail!("Failed to save room: {}", err);
                    },
                };

                let mut se = serde_cbor::ser::Serializer::new(vec![]);
                lua::with_table_deserializer(&result, |de| {
                    serde_transcode::transcode(de, &mut se)
                })?;
                let data = se.into_inner();

                serde_cbor::to_writer(&mut f, &SaveData::RoomScript(room_id, data))?;
            }
            let room = level.get_room_info(room_id);
            if let Some(rc) = entities.get_component::<RoomController>(room.controller) {
                let state = RoomEntityState {
                    timetabled_visitors: rc.timetabled_visitors.iter()
                        .map(|v| v.iter()
                            .map(|v| v.iter()
                                .filter_map(|v| entities.get_component::<NetworkId>(*v))
                                .map(|v| v.0)
                                .collect())
                            .collect())
                        .collect(),
                    active_staff: rc.active_staff
                        .and_then(|v| entities.get_component::<NetworkId>(v))
                        .map(|v| v.0),
                };
                serde_cbor::to_writer(&mut f, &SaveData::RoomEntityState(room_id, state))?;
            }
        }

        for ((player, idx), rc) in &running_choices.choices {
            let script = assume!(log, choices.student_idle.get_choice_by_index(*idx));
            let result = match engine.with_borrows()
                .borrow_mut(entities)
                .borrow_mut(players)
                .invoke_function::<_, Ref<Table>>("invoke_module_method", (
                    Ref::new_string(engine, script.script.module()),
                    Ref::new_string(engine, script.script.resource()),
                    Ref::new_string(engine, "save"),
                    rc.handle.clone(),
            )) {
                Ok(ret) => ret,
                Err(err) => {
                    bail!("Failed to save script: {}", err);
                },
            };

            let mut se = serde_cbor::ser::Serializer::new(vec![]);
            lua::with_table_deserializer(&result, |de| {
                serde_transcode::transcode(de, &mut se)
            })?;
            let data = se.into_inner();

            let name = assume!(log, choices.student_idle.get_choice_name_by_index(*idx));
            serde_cbor::to_writer(&mut f, &SaveData::IdleScript(*player, name.into_owned(), data))?;
        }

        if let Some(mission_state) = mission.and_then(|v| v.save(players, entities)) {
            let mut se = serde_cbor::ser::Serializer::new(vec![]);
            lua::with_table_deserializer(&mission_state, |de| {
                serde_transcode::transcode(de, &mut se)
            })?;
            let data = se.into_inner();
            serde_cbor::to_writer(&mut f, &SaveData::MissionState(data))?;
        }
    }
    Ok(())
}

fn init_players(
    sf_players: FNVMap<PlayerId, PlayerInfo>,
    players: &mut crate::PlayerInfoMap,
    mut world: Option<(&mut Level, &snapshot::Snapshots, &mut ecs::Container)>,
    staff_list: &[StaffInfo],
) {
    for (id, player) in sf_players {
        let name = player.name;
        let key = match player.key {
            #[cfg(feature = "steam")]
            PlayerKey::Steam(id) => player::PlayerKey::Steam(steamworks::SteamId::from_raw(id)),
            #[cfg(not(feature = "steam"))]
            PlayerKey::Username(name) => player::PlayerKey::Username(name),
        };
        let info = players.entry(id).or_insert_with(|| crate::player::PlayerInfo::new(key,name, id, &staff_list));
        info.money = player.money;
        info.rating = player.rating;
        if !player.history.is_empty() {
            info.history = player.history.into();
        }
        info.current_income = player.current_income;
        info.current_outcome = player.current_outcome;
        info.state = match player.state {
            PlayerState::None => crate::player::State::None,
            PlayerState::BuildRoom{active_room} => crate::player::State::BuildRoom{active_room},
            PlayerState::EditRoom{active_room} => {
                if let Some((level, _, _)) = world.as_mut() {
                    let limited = level.is_blocked_edit(active_room).is_err();
                    if limited {
                        let mut room = level.get_room_info_mut(active_room);
                        room.limited_editing = true;
                    }
                }
                crate::player::State::EditRoom{active_room}
            },
        };
        info.config = player.config;
        if let Some((level, snapshots, entities)) = world.as_mut() {
            info.courses.extend(player.courses.into_iter()
                .map(|(id, v)| (id, v.to_course(snapshots))));
            for c in info.courses.values() {
                c.init_world(&level.rooms.borrow(), entities);
            }
        }
    }
}

/// Loads just the players from the save file.
pub(crate) fn load_players<F: FileSystem>(
    fs: &F,
    log: &Logger,
    name: &str,
    ty: SaveType,
    asset_manager: &AssetManager,
    players: &mut crate::PlayerInfoMap,
) -> UResult<()>
{
    let path = format!("{}.usav", name);
    if !fs.exists(&path) {
        return Err(ErrorKind::NoSuchSave.into());
    }
    let mut f = BufReader::new(fs.read(&path)?);
    let version = f.read_u32::<LittleEndian>()?;
    match version {
        4 => {},
        SAVE_VERSION => {},
        _ => bail!("Invalid save version"),
    }
    if SaveType::from_u32(f.read_u32::<LittleEndian>()?) != Some(ty) {
        bail!("Incorrect save type")
    }
    let len = f.read_i32::<LittleEndian>()?;
    if len != -1 {
        f.seek(SeekFrom::Current(i64::from(len)))?;
    }

    let spl = match version {
        SAVE_VERSION => {
            let mut sf = SaveStreamDecode::new(f);
            sf.next().transpose()?
        }
        _ => unimplemented!(),
    };

    if let Some(SaveData::Players(sf_players)) = spl {
        let staff_list = load_staff_list(log, asset_manager);
        init_players(sf_players, players, None, &staff_list);
    } else {
        bail!("Invalid save file layout");
    }

    Ok(())
}

/// Loads the game's state with the given name
/// from the default location
pub(crate) fn load_game<F: FileSystem>(
    fs: &F,
    log: &Logger,
    name: &str,
    ty: SaveType,
    players: &mut crate::PlayerInfoMap,
    asset_manager: &AssetManager, entities: &mut Container,
    snapshots: &mut snapshot::Snapshots,
    engine: &script::Engine,
    choices: &choice::Choices,
    running_choices: &mut script_room::RunningChoices,
    mission: Option<&mut mission::MissionController>,
    day_tick: &mut DayTick,
) -> UResult<Level>
{
    let path = format!("{}.usav", name);
    load_game_impl(fs, log, &*path, ty, players, asset_manager, entities, snapshots, engine, choices, running_choices, mission, day_tick)
}

fn load_game_impl<F: FileSystem>(
    fs: &F,
    log: &Logger,
    path: &str,
    ty: SaveType,
    players: &mut crate::PlayerInfoMap,
    asset_manager: &AssetManager, entities: &mut Container,
    snapshots: &mut snapshot::Snapshots,
    engine: &script::Engine,
    choices: &choice::Choices,
    running_choices: &mut script_room::RunningChoices,
    mission: Option<&mut mission::MissionController>,
    day_tick: &mut DayTick,
) -> UResult<Level>
{
    if !fs.exists(path) {
        return Err(ErrorKind::NoSuchSave.into());
    }
    let mut f = BufReader::new(fs.read(&path)?);
    let version = f.read_u32::<LittleEndian>()?;
    match version {
        4 => {},
        SAVE_VERSION => {},
        _ => bail!("Invalid save version"),
    }
    if SaveType::from_u32(f.read_u32::<LittleEndian>()?) != Some(ty) {
        bail!("Incorrect save type")
    }
    let len = f.read_i32::<LittleEndian>()?;
    if len != -1 {
        f.seek(SeekFrom::Current(i64::from(len)))?;
    }

    match version {
        SAVE_VERSION => {
            let sf = SaveStreamDecode::new(f);
            load_game_generic(log, sf, players, asset_manager, entities, snapshots, engine, choices, running_choices, mission, day_tick)
        }
        _ => unimplemented!(),
    }
}

fn load_game_generic(
    log: &Logger,
    mut sf: impl Iterator<Item=UResult<SaveData>>,
    players: &mut crate::PlayerInfoMap,
    asset_manager: &AssetManager, entities: &mut Container,
    snapshots: &mut snapshot::Snapshots,
    engine: &script::Engine,
    choices: &choice::Choices,
    running_choices: &mut script_room::RunningChoices,
    mission: Option<&mut mission::MissionController>,
    day_tick: &mut DayTick,
) -> UResult<Level>
{
    let sf_players = if let Some(SaveData::Players(sf_players)) = sf.next().transpose()? {
        sf_players
    } else {
        bail!("Invalid save file layout - Players");
    };

    if let Some(SaveData::GameState(state)) = sf.next().transpose()? {
        *day_tick = state.day_tick;
    } else {
        bail!("Invalid save file layout - GameState");
    }

    let mut level = if let Some(SaveData::Level(width, height)) = sf.next().transpose()? {
        Level::new_raw(log.new(o!("type" => "level")), asset_manager, engine, width, height)?
    } else {
        bail!("Invalid save file layout - Level");
    };

    let mut mission_state = None;
    level.compute_path_data = false;
    for sd in sf {
        match sd? {
            SaveData::Room(room) => {
                let owner = room.owner;
                let room_id = room.id;
                let bound = Bound::new(
                    Location::new(room.area.0, room.area.1),
                    Location::new(room.area.2, room.area.3),
                );
                let id = assume!(log, level.place_room_id::<ServerEntityCreator, _>(engine, entities, room_id, owner, room.key.borrow(), bound));

                {
                    let mut rm = level.get_room_info_mut(id);
                    rm.tile_update_state = room.tile_update_state.clone();
                }

                let id = match room.state {
                    RoomState::Planning => id,
                    RoomState::Building => level.finalize_placement(id),
                    RoomState::Done => {
                        let id = level.finalize_placement(id);
                        level.finalize_room::<ServerEntityCreator, _>(engine, entities, id)?;
                        id
                    }
                };

                {
                    let mut rm = level.get_room_info_mut(id);
                    rm.state = room.state;
                }
            },
            SaveData::RoomEntityState(id, state) => {
                let rm = level.get_room_info(id);
                if let Some(rc) = entities.get_component_mut::<RoomController>(rm.controller) {
                    rc.active_staff = state.active_staff
                        .and_then(|v| snapshots.get_entity_by_id(v));
                    for (d, day) in state.timetabled_visitors.into_iter().zip(&mut rc.timetabled_visitors) {
                        for (p, period) in d.into_iter().zip(day) {
                            period.extend(p.into_iter()
                                .filter_map(|v| snapshots.get_entity_by_id(v)));
                        }
                    }
                }
            },
            SaveData::Object(room_id, object) => {
                level.begin_object_placement::<_, ServerEntityCreator>(room_id, engine, entities, object.key.borrow(), Some(object.version))?;
                if let Err(err) = level.move_active_object::<_, ServerEntityCreator>(
                    room_id, engine, entities,
                    object.position,
                    Some(object.version),
                    object.rotation
                ) {
                    match err {
                        UError(ErrorKind::RemoveInvalidPlacement(res), _) => {
                            error!(log, "Removing invalid placement {:?}: {}", object.key, res);
                            level.cancel_object_placement::<ServerEntityCreator>(room_id, entities);
                            continue;
                        },
                        err => Err(err).chain_err(|| ErrorKind::Msg(format!("Failed to move object: {:?}", object.key)))?,
                    }
                }
                level.finalize_object_placement::<_, ServerEntityCreator>(room_id, engine, entities, Some(object.version), object.rotation)?;
            },
            SaveData::Entity(entity) => {
                let ty = asset_manager.loader_open::<Loader<ServerComponent>>(entity.key.borrow())?;
                let variant = &ty.variants[entity.variant];
                let (fna, sna) = entity.name;
                let first_name = variant.name_list
                    .first
                    .iter()
                    .find(|v| fna == ***v)
                    .cloned()
                    .unwrap_or_else(|| fna.into());
                let second_name = variant.name_list
                    .second
                    .iter()
                    .find(|v| sna == ***v)
                    .cloned()
                    .unwrap_or_else(|| sna.into());
                let e = ty.create_entity(entities, entity.variant, Some((
                    first_name,
                    second_name,
                )));
                entity.network_id.map(|v| snapshots.assign_network_id(entities, e, v));
                if let (Some(pos), Some(epos)) = (entities.get_component_mut::<Position>(e), entity.position) {
                    pos.x = epos.x;
                    pos.y = epos.y;
                    pos.z = epos.z;
                }
                if let (Some(rot), Some(erot)) = (entities.get_component_mut::<Rotation>(e), entity.rotation) {
                    rot.rotation = erot;
                }
                if let (Some(speed), Some(espeed)) = (entities.get_component_mut::<MovementSpeed>(e), entity.speed) {
                    speed.base_speed = espeed;
                }
                if let Some(ro) = entity.room_owned {
                    let room = level.get_room_info(ro.room_id);
                    if room.state.is_done() {
                        {
                            if let Some(rc) = entities.get_component_mut::<RoomController>(room.controller) {
                                if let OwnedKind::Owned = ro.kind {
                                    rc.entities.push(e);
                                } else {
                                    rc.visitors.push(e);
                                }
                            }
                        }
                        entities.add_component(e, RoomOwned {
                            room_id: ro.room_id,
                            should_release_inactive: ro.should_release_inactive,
                            active: ro.active,
                        });
                        entities.add_component(e, Controlled::new_by(Controller::Room(ro.room_id)));
                    }
                }

                if let Some(timetable) = entity.timetable {
                    entities.add_component(e, timetable);
                }
                if let Some(v) = entity.timetable_start {
                    entities.add_component(e, v);
                }
                if entity.timetable_completed {
                    entities.add_component(e, TimeTableCompleted);
                }
                if let Some(v) = entity.activity{
                    entities.add_component(e, v);
                }
                if let Some(grades) = entity.grades {
                    entities.add_component(e, grades);
                }
                if let Some(owned) = entity.owned {
                    entities.add_component(e, Owned {
                        player_id: owned,
                    })
                }
                if let (Some(paid), Some(p)) = (entities.get_component_mut::<Paid>(e), entity.paid) {
                    paid.last_payment = p.last_payment;
                    paid.cost = p.cost;
                    paid.wanted_cost = ::std::cmp::max(p.wanted_cost, p.cost);
                }
                if let Some(tints) = entity.tints {
                    entities.add_component(e, Tints {
                        tints,
                    });
                }

                if let (Some(vars), Some(v)) = (get_vars(entities, e), entity.vars) {
                    for (k, v) in v.vars {
                        vars.set_raw(&k, v);
                    }
                }


                if let (Some(idle), Some(owned)) = (entity.idle, entity.owned) {
                    let idx = idle.current_choice
                            .and_then(|v| choices.student_idle.get_choice_index_by_name(v));
                    entities.add_component(e, Idle {
                        released: false,
                        total_idle_time: idle.total_idle_time,
                        current_choice: idx,
                    });
                    if let Some(idx) = idx {
                        let rc = running_choices.choices.entry((
                            owned,
                            idx,
                        )).or_insert_with(|| {
                            let name = assume!(log, choices.student_idle.get_choice_name_by_index(idx));
                            crate::script_room::RunningChoice::new(log, engine, owned, name, idx)
                        });
                        rc.pending_entity.push(e);
                    }
                }

                if let Some(goto) = entity.goto_room {
                    entities.with(|
                        _em: EntityManager<'_>,
                        mut goto_room: crate::ecs::Write<GotoRoom>,
                        mut room_controller: crate::ecs::Write<RoomController>,
                    | {
                        goto_room.add_component(e, GotoRoom::new(log, e, &level.rooms.borrow(), &mut room_controller, goto.room_id));
                    });
                }
                if let Some(money) = entity.money {
                    entities.add_component(e, Money {
                        money: money.money,
                    });
                } else {
                    // Give some spending cash for old entities
                    entities.add_component(e, Money {
                        money: UniDollar(10_000),
                    });
                }
            },
            SaveData::RoomScript(room_id, saved_state) => {
                let ty = {
                    let room = level.get_room_info_mut(room_id);
                    if room.controller.is_invalid() {
                        continue;
                    }
                    assume!(log, asset_manager.loader_open::<room::Loader>(room.key.borrow()))
                };
                if let Some(controller) = ty.controller.as_ref() {
                    let lua_room = crate::script_room::LuaRoom::from_room(log, &level.rooms.borrow(), entities, room_id, engine);

                    let mut de = serde_cbor::de::Deserializer::from_slice(&saved_state);
                    let state = lua::with_table_serializer(engine, |se| {
                        serde_transcode::transcode(&mut de, se)
                    })?;

                    engine.with_borrows()
                        .borrow_mut(&mut level)
                        .borrow_mut(entities)
                        .borrow_mut(players)
                        .invoke_function::<_, Ref<Unknown>>("invoke_module_method", (
                            Ref::new_string(engine, controller.module()),
                            Ref::new_string(engine, controller.resource()),
                            Ref::new_string(engine, "load"),
                            lua_room,
                            state
                    ))?;
                }
            },
            SaveData::IdleScript(player_id, name, saved_state) => {
                let mut de = serde_cbor::de::Deserializer::from_slice(&saved_state);
                let state = lua::with_table_serializer(engine, |se| {
                    serde_transcode::transcode(&mut de, se)
                })?;

                if let Some(idx) = choices.student_idle.get_choice_index_by_name(name) {
                    let rc = running_choices.choices.entry((
                        player_id,
                        idx,
                    )).or_insert_with(|| {
                        let name = assume!(log, choices.student_idle.get_choice_name_by_index(idx));
                        crate::script_room::RunningChoice::new(log, engine, player_id, name, idx)
                    });
                    rc.load_data = Some(state);
                }
            },
            SaveData::MissionState(state) => {
                let mut de = serde_cbor::de::Deserializer::from_slice(&state);
                let state = lua::with_table_serializer(engine, |se| {
                    serde_transcode::transcode(&mut de, se)
                })?;
                mission_state = Some(state);
            },
            _ => unimplemented!(),
        }
    }
    level.compute_path_data = true;
    {
        for room_id in level.room_ids() {
            let (key, area) = {
                let mut room = level.get_room_info_mut(room_id);
                room.rebuild_object_maps();
                (room.key.clone(), room.area)
            };
            level.rebuild_path_sections(area);
            let room_info = asset_manager.loader_open::<room::Loader>(key.borrow())?;
            let cost = room_info.cost_for_room(&level, room_id);
            level.get_room_info_mut(room_id).placement_cost = cost;
        }
    }

    let staff_list = load_staff_list(log, asset_manager);
    init_players(sf_players, players, Some((&mut level, snapshots, entities)), &staff_list);

    if let Some(v) = mission {
        v.init(players, entities, mission_state);
    }

    Ok(level)
}

struct SaveStreamDecode<R: IoRead> {
    r: R,
}

impl <R> SaveStreamDecode<R>
    where R: IoRead,
{
    fn new(r: R) -> SaveStreamDecode<R> {
        SaveStreamDecode {
            r,
        }
    }
}

impl <R> Iterator for SaveStreamDecode<R>
    where R: IoRead
{
    type Item = UResult<SaveData>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut r = serde_cbor::Deserializer::from_reader(&mut self.r);
        let res = SaveData::deserialize(&mut r);
        match res {
            Ok(val) => Some(Ok(val)),
            Err(ref err) if err.is_eof() => None,
            Err(err) => Some(Err(err.into()))
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[allow(clippy::large_enum_variant)]
enum SaveData {
    Players(FNVMap<PlayerId, PlayerInfo>),
    Level(u32, u32),
    GameState(GameState),
    Room(RoomInfo),
    Object(RoomId, ObjectInfo),
    RoomScript(RoomId, Vec<u8>),
    RoomEntityState(RoomId, RoomEntityState),
    Entity(EntityInfo),
    IdleScript(PlayerId, ResourceKey<'static>, Vec<u8>),
    MissionState(Vec<u8>),
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct GameState {
    day_tick: DayTick,
}

/// A player key is used to uniquely identify a player
/// between games/saves&loads.
///
/// This shouldn't change for a player whenever possible
/// (e.g. steam id instead of steam name).
#[derive(Debug, Serialize, Deserialize)]
enum PlayerKey {
    /// A 64bit steam id
    #[cfg(feature = "steam")]
    Steam(u64),
    #[cfg(not(feature = "steam"))]
    Username(String),
}


/// Information about a single player
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct PlayerInfo {
    /// The name of the player
    pub(crate) name: String,
    /// The unique player key
    key: PlayerKey,
    /// The player's money
    pub(crate) money: UniDollar,
    /// The player's current rating
    pub(crate) rating: i16,
    state: PlayerState,
    config: PlayerConfig,
    history: Vec<HistoryEntry>,
    current_income: UniDollar,
    current_outcome: UniDollar,
    courses: FNVMap<course::CourseId, SavableCourse>,
}

/// Contains the state and related information for a player
#[derive(Clone, Debug, Serialize, Deserialize)]
enum PlayerState {
    /// Default state
    None,
    /// Building a room
    BuildRoom {
        /// The id of the room being editted
        active_room: room::Id,
    },
    /// Editting/building a room
    EditRoom {
        /// The id of the room being editted
        active_room: room::Id,
    },
}

/// Information about a single entity
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct EntityInfo {
    /// The entity key
    pub(crate) key: ResourceKey<'static>,
    /// The variant type of the entity
    pub(crate) variant: usize,
    /// The first and second name of the entity
    pub(crate) name: (String, String),
    network_id: Option<u32>,
    position: Option<PositionInfo>,
    rotation: Option<Angle>,
    speed: Option<f32>,
    room_owned: Option<RoomOwnedInfo>,
    owned: Option<PlayerId>,
    paid: Option<PaidInfo>,
    tints: Option<Vec<(u8, u8, u8, u8)>>,
    vars: Option<VarsInfo>,

    timetable: Option<TimeTable>,
    timetable_start: Option<TimeTableStart>,
    timetable_completed: bool,
    activity: Option<Activity>,

    grades: Option<Grades>,
    idle: Option<IdleInfo>,
    goto_room: Option<GotoRoomInfo>,

    money: Option<MoneyInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
struct MoneyInfo {
    money: UniDollar,
}

#[derive(Debug, Serialize, Deserialize)]
struct GotoRoomInfo {
    room_id: RoomId,
}

#[derive(Debug, Serialize, Deserialize)]
struct PaidInfo {
    last_payment: Option<u32>,
    cost: UniDollar,
    wanted_cost: UniDollar,
}

#[derive(Debug, Serialize, Deserialize)]
struct IdleInfo {
    total_idle_time: i32,
    current_choice: Option<ResourceKey<'static>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PositionInfo {
    x: f32,
    y: f32,
    z: f32,
}

#[derive(Debug, Serialize, Deserialize)]
struct RoomOwnedInfo {
    room_id: RoomId,
    kind: OwnedKind,
    should_release_inactive: bool,
    active: bool,
}

#[derive(Debug, Serialize, Deserialize)]
enum OwnedKind {
    Visitor,
    Owned
}

#[derive(Debug, Serialize, Deserialize)]
struct VarsInfo {
    vars: FNVMap<String, u32>,
}

/// Stores information about a saved room
#[derive(Debug, Serialize, Deserialize)]
pub struct RoomInfo {
    /// The type of the room
    pub key: ResourceKey<'static>,
    /// The room's id
    pub id: RoomId,
    /// The room's owner's id
    pub owner: PlayerId,
    /// The position and size of the room
    pub area: (i32, i32, i32, i32),
    /// The state of the room
    pub state: RoomState,
    /// The stored state from the tile updater script
    pub tile_update_state: Option<Vec<u8>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct RoomEntityState {
    timetabled_visitors: Vec<Vec<Vec<u32>>>,
    active_staff: Option<u32>,
}

/// Object placement information
#[derive(Debug, Serialize, Deserialize)]
pub struct ObjectInfo {
    /// The key to the object that this placement is about
    pub key: ResourceKey<'static>,
    /// The position the object was placed at.
    ///
    /// This is the raw click position as passed to the script
    pub position: (f32, f32),
    /// The rotation value passed to the placement script
    ///
    /// Used when moving an object to keep its rotation the
    /// same
    pub rotation: i16,
    /// A script provided version number to use when replacing
    /// this object
    pub version: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SavableCourse {
    uid: course::CourseId,
    name: String,
    group: String,
    cost: UniDollar,
    timetable: [[SavableCourseEntry; NUM_TIMETABLE_SLOTS]; 7],
    #[serde(default)]
    deprecated: bool,
}

impl SavableCourse {
    fn to_course(self, snapshots: &snapshot::Snapshots) -> course::Course {
        let [t0, t1, t2, t3, t4, t5, t6] = self.timetable;
        course::Course {
            uid: self.uid,
            name: self.name,
            group: self.group,
            cost: self.cost,
            timetable: [
                Self::make_timetable_row(snapshots, t0),
                Self::make_timetable_row(snapshots, t1),
                Self::make_timetable_row(snapshots, t2),
                Self::make_timetable_row(snapshots, t3),
                Self::make_timetable_row(snapshots, t4),
                Self::make_timetable_row(snapshots, t5),
                Self::make_timetable_row(snapshots, t6),
            ],
            deprecated: self.deprecated,
        }
    }

    fn make_timetable_row(snapshots: &snapshot::Snapshots, v: [SavableCourseEntry; NUM_TIMETABLE_SLOTS]) -> [course::CourseEntry; NUM_TIMETABLE_SLOTS] {
        let [v0, v1, v2, v3] = v;
        [
            Self::make_timetable_entry(snapshots, v0),
            Self::make_timetable_entry(snapshots, v1),
            Self::make_timetable_entry(snapshots, v2),
            Self::make_timetable_entry(snapshots, v3),
        ]
    }

    fn make_timetable_entry(snapshots: &snapshot::Snapshots, v: SavableCourseEntry) -> course::CourseEntry {
        match v {
            SavableCourseEntry::Free => course::CourseEntry::Free,
            SavableCourseEntry::Lesson{key, rooms} => course::CourseEntry::Lesson {
                key,
                rooms: rooms.into_iter()
                    .filter_map(|v| Some(course::LessonRoom {
                        staff: snapshots.get_entity_by_id(v.staff)?,
                        room: v.room,
                    }))
                    .collect(),
            }
        }
    }

    fn conv_timetable(ids: &Read<NetworkId>, v: &[[course::CourseEntry; NUM_TIMETABLE_SLOTS]; 7]) -> [[SavableCourseEntry; NUM_TIMETABLE_SLOTS]; 7] {
        [
            Self::conv_timetable_day(ids, &v[0]),
            Self::conv_timetable_day(ids, &v[1]),
            Self::conv_timetable_day(ids, &v[2]),
            Self::conv_timetable_day(ids, &v[3]),
            Self::conv_timetable_day(ids, &v[4]),
            Self::conv_timetable_day(ids, &v[5]),
            Self::conv_timetable_day(ids, &v[6]),
        ]
    }

    fn conv_timetable_day(ids: &Read<NetworkId>, v: &[course::CourseEntry; NUM_TIMETABLE_SLOTS]) -> [SavableCourseEntry; NUM_TIMETABLE_SLOTS] {
        [
            Self::conv_timetable_entry(ids, &v[0]),
            Self::conv_timetable_entry(ids, &v[1]),
            Self::conv_timetable_entry(ids, &v[2]),
            Self::conv_timetable_entry(ids, &v[3]),
        ]
    }

    fn conv_timetable_entry(ids: &Read<NetworkId>, v: &course::CourseEntry) -> SavableCourseEntry {
        match v {
            course::CourseEntry::Free => SavableCourseEntry::Free,
            course::CourseEntry::Lesson{key, rooms} => SavableCourseEntry::Lesson {
                key: key.clone(),
                rooms: rooms.iter()
                    .filter_map(|v| Some(SavableLessonRoom {
                        room: v.room,
                        staff: ids.get_component(v.staff)?.0,
                    }))
                    .collect()
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
enum SavableCourseEntry {
    Lesson {
        key: ResourceKey<'static>,
        rooms: Vec<SavableLessonRoom>,
    },
    Free,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SavableLessonRoom {
    room: RoomId,
    staff: u32,
}