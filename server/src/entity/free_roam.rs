//! Handling of free roaming entities
use super::*;
use crate::prelude::*;
use crate::script;
use lua::{self, Ref, Coroutine};
use crate::common::ScriptData;

/// An entity that can be scripted without being apart
/// of a room.
pub struct FreeRoam {
    /// The name of the script that will handle this entity
    pub script: ResourceKey<'static>,
}
component!(FreeRoam => Map);

/// Ticks all free roaming entities for the server
pub(crate) fn server_tick(
        log: &Logger,
        entities: &mut Container,
        scripting: &script::Engine,
        player_info: &mut crate::PlayerInfoMap,
) {
    tick::<crate::script_room::Types, _>(
        log,
        entities, scripting,
        player_info,
        "server_handler"
    );
}

/// Ticks all free roaming entities
pub fn tick<E: script::ScriptTypes, C: 'static>(
        log: &Logger,
        entities: &mut Container,
        scripting: &lua::Lua,
        extra: &mut C,
        handler: &'static str,
) {
    let mask = entities.mask_for::<free_roam::FreeRoam>()
        .and_component::<Owned>(entities)
        .and_not_component::<Frozen>(entities)
        .and_not_component::<Quitting>(entities);

    for e in entities.iter_mask(&mask).collect::<Vec<_>>() {
        let props = entities.with(|
            _em: EntityManager<'_>,
            mut roam_props: ecs::Write<LuaRoamEntityProperties>,
        | {
            LuaRoamEntityProperties::get_or_create(&mut roam_props, e)
        });

        let scope;
        let script;
        {

            scope = Ref::new_table(scripting);
            entities.with(|
                _em: EntityManager<'_>,
                mut entity_ref: ecs::Write<E::EntityRef>,
                living: ecs::Read<Living>,
                object: ecs::Read<Object>,
            | {
                scope.insert(Ref::new_string(scripting, "entity"), E::from_entity(scripting, &mut entity_ref, &living, &object, e, None));
            });

            let free_roam = assume!(log, entities.get_component::<free_roam::FreeRoam>(e));
            let owned = assume!(log, entities.get_component::<Owned>(e));

            scope.insert(Ref::new_string(scripting, "player"), i32::from(owned.player_id.0));
            let player = owned.player_id;
            let log = log.clone();

            let script_module = free_roam.script.module_key().into_owned();
            scope.insert(Ref::new_string(scripting, "notify_player"), lua::closure2(move |lua, de_func: Ref<String>, data: Ref<Arc<bitio::Writer<Vec<u8>>>>| -> UResult<()> {
                let mut players = lua.write_borrow::<crate::PlayerInfoMap>();
                let player = assume!(log, players.get_mut(&player));

                let (script, method) = if let Some(pos) = de_func.char_indices().find(|v| v.1 == '#') {
                    de_func.split_at(pos.0)
                } else {
                    bail!("invalid method description")
                };
                let script = assets::LazyResourceKey::parse(&script)
                        .or_module(script_module.borrow())
                        .into_owned();
                let func = method[1..].into();

                player.notifications.push(crate::notify::Notification::Script {
                    script,
                    func,
                    data: ScriptData(Arc::clone(&data)),
                });
                Ok(())
            }));

            script = free_roam.script.clone();
        }

        let coroutine = match scripting.with_borrows()
            .borrow_mut(entities)
            .borrow_mut(extra)
            .invoke_function::<_, Ref<Coroutine>>("invoke_free_roam", (
                Ref::new_string(scripting, script.module()),
                Ref::new_string(scripting, script.resource()),
                Ref::new_string(scripting, handler),
                props.clone(),
                scope,
        )) {
            Err(err) => {
                error!(log, "Failed to tick entity"; "entity" => ?e, "error" => %err);
                continue;
            },
            Ok(val) => val,
        };

        let props = assume!(log, entities.get_component_mut::<LuaRoamEntityProperties>(e));
        props.coroutine = Some(coroutine);

    }
}