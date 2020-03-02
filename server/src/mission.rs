//! Provides controls for interfacing with mission control
//! scripts

use crate::common;
use crate::prelude::*;
use lua;
use std::cell::RefCell;
use std::sync::Arc;

/// Manages mission scripts
pub struct MissionController {
    log: Logger,
    engine: ScriptEngine,
    _info: common::MissionEntry,
    pub(crate) handler: ResourceKey<'static>,
    /// List of mission generated commands
    pub generated_commands: RefCell<Vec<Command>>,
}

impl MissionController {
    /// Creates a mission controller that will run the
    /// named script.
    pub fn new(log: Logger, engine: ScriptEngine, mission: ResourceKey<'_>) -> MissionController {
        let by_name: lua::Ref<lua::Table> =
            assume!(log, engine.get(lua::Scope::Global, "missions_by_name"));
        let key = lua::Ref::new_string(&engine, mission.as_string());
        let info = assume!(log, by_name.get::<_, lua::Ref<lua::Table>>(key));
        let info = assume!(log, lua::from_table::<common::MissionEntry>(&info));

        MissionController {
            log,
            engine,
            handler: info.get_handler_key().into_owned(),
            _info: info,
            generated_commands: RefCell::new(Vec::new()),
        }
    }

    /// Calls the init function for the mission
    pub(crate) fn init(
        &mut self,
        players: &mut crate::PlayerInfoMap,
        entities: &mut Container,
        previous: Option<lua::Ref<lua::Table>>,
    ) {
        if let Err(err) = self
            .engine
            .with_borrows()
            .borrow(&MissionAllowed)
            .borrow(self)
            .borrow_mut(entities)
            .borrow_mut(players)
            .invoke_function::<_, ()>(
                "invoke_module_method",
                (
                    lua::Ref::new_string(&self.engine, self.handler.module()),
                    lua::Ref::new_string(&self.engine, self.handler.resource()),
                    lua::Ref::new_string(&self.engine, "server_init"),
                    previous,
                ),
            )
        {
            warn!(self.log, "Failed to init mission: {}", err);
        }
    }

    /// Calls the update function for the mission
    pub(crate) fn update(&mut self, players: &mut crate::PlayerInfoMap, entities: &mut Container) {
        if let Err(err) = self
            .engine
            .with_borrows()
            .borrow(&MissionAllowed)
            .borrow(self)
            .borrow_mut(entities)
            .borrow_mut(players)
            .invoke_function::<_, ()>(
                "invoke_module_method",
                (
                    lua::Ref::new_string(&self.engine, self.handler.module()),
                    lua::Ref::new_string(&self.engine, self.handler.resource()),
                    lua::Ref::new_string(&self.engine, "server_update"),
                ),
            )
        {
            warn!(self.log, "Failed to update mission: {}", err);
        }
    }

    /// Calls the save method for the mission
    pub(crate) fn save(
        &mut self,
        players: &mut crate::PlayerInfoMap,
        entities: &mut Container,
    ) -> Option<lua::Ref<lua::Table>> {
        match self
            .engine
            .with_borrows()
            .borrow(&MissionAllowed)
            .borrow(self)
            .borrow_mut(entities)
            .borrow_mut(players)
            .invoke_function::<_, lua::Ref<lua::Table>>(
                "invoke_module_method",
                (
                    lua::Ref::new_string(&self.engine, self.handler.module()),
                    lua::Ref::new_string(&self.engine, self.handler.resource()),
                    lua::Ref::new_string(&self.engine, "save"),
                ),
            ) {
            Ok(val) => Some(val),
            Err(err) => {
                warn!(self.log, "Failed to save mission: {}", err);
                None
            }
        }
    }
}

/// Used to restrict certain api calls to only run during
/// mission functions
pub struct MissionAllowed;

/// Sets up a interface for scripts to interface with
pub fn init_missionlib(lua: &lua::Lua) {
    use lua::{Ref, Scope, Table};

    lua.set(
        Scope::Global,
        "control_get_players",
        lua::closure(|lua| -> Ref<Table> {
            let _limit = lua.get_borrow::<MissionAllowed>();
            let players = lua.read_borrow::<crate::PlayerInfoMap>();
            players.keys().fold(Ref::new_table(lua), |tbl, v| {
                tbl.insert(tbl.length() + 1, i32::from(v.0));
                tbl
            })
        }),
    );
    lua.set(
        Scope::Global,
        "control_give_money",
        lua::closure2(|lua, id: i32, amount: i32| {
            let _limit = lua.get_borrow::<MissionAllowed>();
            let mut players = lua.write_borrow::<crate::PlayerInfoMap>();
            players
                .get_mut(&PlayerId(id as i16))
                .map(|v| v.change_money(UniDollar(i64::from(amount))))
        }),
    );
    lua.set(
        Scope::Global,
        "control_submit_command",
        lua::closure1(|lua, cmd: Ref<Command>| {
            let _limit = lua.get_borrow::<MissionAllowed>();
            let ctrl = lua.get_borrow::<MissionController>();
            let mut list = ctrl.generated_commands.borrow_mut();
            list.push(Command::clone(&cmd));
        }),
    );
}

/// Sets up a interface for scripts to interface with
pub fn init_commandlib(lua: &lua::Lua) {
    use lua::{Ref, Scope};
    lua.set(
        Scope::Global,
        "control_cmd_exec_mission",
        lua::closure1(
            |lua, data: Ref<Arc<bitio::Writer<Vec<u8>>>>| -> Ref<Command> {
                let _limit = lua.get_borrow::<MissionAllowed>();
                Ref::new(
                    lua,
                    ExecMission::new(common::ScriptData(Arc::clone(&data))).into(),
                )
            },
        ),
    );
}
