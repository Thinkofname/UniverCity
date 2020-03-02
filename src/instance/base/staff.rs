use super::super::*;
use crate::server::assets;
use crate::server::event;
use crate::server::network;
use crate::server::player;
use crate::state;

use std::time;

pub struct BuyStaffState {
    ui: Option<ui::Node>,
    staff_list: Vec<StaffInfo>,
    selected_staff: usize,

    request_ticket: Option<network::RequestTicket<player::StaffPage>>,
    selected_uid: u32,
    next_update: f64,
    page: u8,
    num_pages: u8,

    current_page: player::StaffPageInfo,
}

impl BuyStaffState {
    pub fn new() -> BuyStaffState {
        BuyStaffState {
            ui: None,
            staff_list: vec![],
            selected_staff: 0,
            request_ticket: None,
            selected_uid: 0,
            next_update: 60.0 * 2.0, // Every 2 seconds
            page: 0,
            num_pages: 1,
            current_page: player::StaffPageInfo {
                unique_id: 0,
                page: 0,
                variant: 0,
                first_name: "".into(),
                surname: "".into(),
                description: "".into(),
                stats: [0.0; Stats::MAX],
                hire_price: UniDollar::default(),
            },
        }
    }
}

impl state::State for BuyStaffState {
    fn copy(&self) -> Box<dyn state::State> {
        Box::new(BuyStaffState {
            ui: self.ui.clone(),
            staff_list: self.staff_list.clone(),
            selected_staff: self.selected_staff,
            request_ticket: self.request_ticket,
            selected_uid: self.selected_uid,
            next_update: self.next_update,
            page: self.page,
            num_pages: self.num_pages,
            current_page: self.current_page.clone(),
        })
    }

    fn added(
        &mut self,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
    ) -> state::Action {
        let instance = assume!(state.global_logger, instance.as_mut());
        if !instance.player.state.is_none() {
            // Can't hire staff whilst one is already in progress
            return state::Action::Pop;
        }
        self.staff_list = load_staff_list(&instance.log, &instance.asset_manager);
        state::Action::Nothing
    }

    fn active(
        &mut self,
        _instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
    ) -> state::Action {
        let ui = state
            .ui_manager
            .create_node(assets::ResourceKey::new("base", "buy/staff"));
        self.ui = Some(ui.clone());

        if let Some(scroll_panel) = query!(ui, scroll_panel).next() {
            if let Some(content) = query!(scroll_panel, scroll_panel > content).next() {
                for (i, staff) in self.staff_list.iter().enumerate() {
                    let e_id = staff.entity.borrow();

                    let ty = assume!(
                        state.global_logger,
                        state
                            .asset_manager
                            .loader_open::<Loader<entity::ClientComponent>>(e_id)
                    );
                    let n = node! {
                        button(id=i as i32) {
                            content {
                                @text(ty.display_name.clone())
                            }
                        }
                    };
                    n.set_property("name", ty.display_name.clone());
                    n.set_property(
                        "on_click",
                        ui::MethodDesc::<ui::MouseUpEvent>::native(move |evt, _, _| {
                            evt.emit(SelectStaffEvent(i));
                            true
                        }),
                    );
                    content.add_child(n);
                }

                scroll_panel.set_property::<i32>("rows", self.staff_list.len() as i32);
            }
        }
        if let Some(prev) = query!(ui, button(id = "prev")).next() {
            prev.set_property(
                "on_click",
                ui::MethodDesc::<ui::MouseUpEvent>::native(|evt, _, _| {
                    evt.emit(Previous);
                    true
                }),
            );
        }
        if let Some(next) = query!(ui, button(id = "next")).next() {
            next.set_property(
                "on_click",
                ui::MethodDesc::<ui::MouseUpEvent>::native(|evt, _, _| {
                    evt.emit(Next);
                    true
                }),
            );
        }
        // Focus the first entry
        state.ui_manager.events().emit(SelectStaffEvent(0));
        state.ui_manager.events().emit(CloseWindowOthers(ui));
        state::Action::Nothing
    }

    fn tick(
        &mut self,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
    ) -> state::Action {
        let instance = assume!(state.global_logger, instance.as_mut());
        if !instance.player.state.is_none() {
            // Can't hire staff whilst one is already in progress
            state::Action::Pop
        } else {
            self.next_update -= state.delta;
            if self.next_update < 0.0 {
                self.next_update = 60.0 * 2.0;
                let e_id = self.staff_list[self.selected_staff].entity.clone();
                self.request_ticket = Some(instance.request_manager.request(player::StaffPage {
                    page: self.page,
                    staff_key: e_id,
                }));
            }
            state::Action::Nothing
        }
    }

    fn inactive(&mut self, _instance: &mut Option<GameInstance>, state: &mut crate::GameState) {
        if let Some(ui) = self.ui.take() {
            state.ui_manager.remove_node(ui);
        }
    }

    fn ui_event_req(
        &mut self,
        req: &mut state::CaptureRequester,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
        evt: &mut event::EventHandler,
    ) -> state::Action {
        let instance = assume!(state.global_logger, instance.as_mut());

        let mut action = state::Action::Nothing;

        let ui = assume!(state.global_logger, self.ui.clone());

        evt.handle_event_if::<super::CloseWindowOthers, _, _>(
            |evt| {
                // Handle in event_if in order to not consume the event and let
                // other windows read it too
                if !evt.0.is_same(&ui) {
                    action = state::Action::Pop;
                }
                false
            },
            |_| {},
        );
        evt.handle_event::<SelectStaffEvent, _>(|evt| {
            self.page = 0;
            if let Some(btn) = query!(ui, button(id = self.selected_staff as i32)).next() {
                btn.set_property("selected", false);
            }
            self.selected_staff = evt.0;
            if let Some(btn) = query!(ui, button(id = self.selected_staff as i32)).next() {
                btn.set_property("selected", true);
            }
            let staff = &self.staff_list[evt.0];
            if let Some(icon) = query!(ui, preview > icon).next() {
                icon.set_property("img", "base:solid".to_owned());
            }
            if let Some(description) = query!(ui, preview > description > @text).next() {
                description.set_text("Loading...");
            }
            self.request_ticket = Some(instance.request_manager.request(player::StaffPage {
                page: self.page,
                staff_key: staff.entity.clone(),
            }));

            if let Some(skills) = query!(ui, skills).next() {
                for clip in query!(skills, skill > skill_clip).matches() {
                    clip.set_property("value", 0.5);
                }
            }
            if let Some(prev) = query!(ui, button(id = "prev")).next() {
                prev.set_property("disabled", true);
            }
            if let Some(next) = query!(ui, button(id = "next")).next() {
                next.set_property("disabled", true);
            }
        });

        if let Some(req) = self.request_ticket {
            network::RequestManager::handle_reply(evt, req, |rpl| {
                self.request_ticket = None;

                self.num_pages = rpl.num_pages;
                if let Some(page) = rpl.info {
                    self.page = page.page;
                    if let Some(description) = query!(ui, preview > description > @text).next() {
                        description.set_text(format!(
                            "{} {} - {}",
                            page.first_name, page.surname, page.description
                        ));
                    }
                    let money = instance.player.get_money();
                    let can_afford = money >= page.hire_price || page.hire_price == UniDollar(0);
                    if let Some(price_tag) = query!(ui, price_tag).next() {
                        price_tag.set_property("can_afford", can_afford);
                        if let Some(txt) = query!(price_tag, @text).next() {
                            txt.set_text(format!("Cost: {}", page.hire_price));
                        }
                    }

                    if let Some(hire) = query!(ui, button(id = "hire")).next() {
                        hire.set_property("disabled", !can_afford);
                    }
                    let e_id = self.staff_list[self.selected_staff].entity.borrow();

                    let ty = assume!(
                        state.global_logger,
                        instance
                            .asset_manager
                            .loader_open::<Loader<entity::ClientComponent>>(e_id)
                    );
                    let sub_ty = &ty.variants[page.variant as usize];

                    self.selected_uid = page.unique_id;

                    if let Some(icon) = query!(ui, preview > icon).next() {
                        icon.set_property("img", sub_ty.icon.as_string());
                    }

                    if let Some(skills) = query!(ui, skills).next() {
                        // Remove old skills
                        for skill in query!(skills, skill).matches() {
                            skills.remove_child(skill);
                        }

                        let variant = entity_variant(&ty);
                        for stat in variant.stats() {
                            let s = stat.as_string();
                            // Not needed for hiring
                            if s == "happiness" || s == "hunger" || s == "fatigue" {
                                continue;
                            }
                            let tooltip = stat.tooltip_string();
                            if tooltip == "*" && cfg!(not(feature = "debugutil")) {
                                continue;
                            }
                            let title = super::fix_case(s);
                            let n = node! {
                                skill(key=s.to_owned(), title = title.clone(), tooltip=tooltip.to_owned()) {
                                    skill_clip(value=f64::from(page.stats[stat.index])) {
                                        skill_bar
                                    }
                                    label {
                                        @text(title)
                                    }
                                }
                            };
                            skills.add_child(n);
                        }
                    }

                    if let Some(prev) = query!(ui, button(id = "prev")).next() {
                        prev.set_property("disabled", self.page == 0);
                    }
                    if let Some(next) = query!(ui, button(id = "next")).next() {
                        next.set_property("disabled", self.page == self.num_pages - 1);
                    }
                    self.current_page = page;
                } else {
                    self.page = 0;
                    if let Some(description) = query!(ui, preview > description > @text).next() {
                        description.set_text("No staff available for hire");
                    }
                    if let Some(price_tag) = query!(ui, price_tag).next() {
                        price_tag.set_property("can_afford", false);
                        if let Some(txt) = query!(price_tag, @text).next() {
                            txt.set_text("");
                        }
                    }

                    if let Some(hire) = query!(ui, button(id = "hire")).next() {
                        hire.set_property("disabled", true);
                    }
                    if let Some(skills) = query!(ui, skills).next() {
                        // Remove old skills
                        for skill in query!(skills, skill).matches() {
                            skills.remove_child(skill);
                        }
                    }

                    if let Some(prev) = query!(ui, button(id = "prev")).next() {
                        prev.set_property("disabled", true);
                    }
                    if let Some(next) = query!(ui, button(id = "next")).next() {
                        next.set_property("disabled", true);
                    }
                }
            });
        }

        /*
        evt.handle_event_if::<StaffPage, _, _>(|evt| evt.request_id == req_id, |page| {
            self.page = page.page;
            self.num_pages = page.num_pages;
            if let Some(description) = query!(ui, preview > description > @text).next() {
                description.set_text(format!("{} {} - {}", page.first_name, page.surname, page.description));
            }
            let money = instance.player.get_money();
            let can_afford = money >= page.hire_price || page.hire_price == UniDollar(0);
            if let Some(price_tag) = query!(ui, price_tag).next() {
                price_tag.set_property("can_afford", can_afford);
                if let Some(txt) = query!(price_tag, @text).next() {
                    txt.set_text(format!("Cost: {}", page.hire_price));
                }
            }

            if let Some(hire) = query!(ui, button(id="hire")).next() {
                hire.set_property("disabled", !can_afford);
            }
            let e_id = self.staff_list[self.selected_staff].entity.borrow();

            let ty = assume!(state.global_logger, instance.asset_manager.loader_open::<Loader<entity::ClientComponent>>(e_id));
            let sub_ty = &ty.variants[page.variant];

            self.selected_uid = page.unique_id;

            if let Some(icon) = query!(ui, preview > icon).next() {
                icon.set_property("img", sub_ty.icon.as_string());
            }

            if let Some(skills) = query!(ui, skills).next() {
                // Remove old skills
                for skill in query!(skills, skill).matches() {
                    skills.remove_child(skill);
                }

                let variant = entity_variant(&ty);
                for stat in variant.stats() {
                    let s = stat.as_string();
                    // Not needed for hiring
                    if s == "happiness" || s == "hunger" || s == "fatigue" {
                        continue;
                    }
                    let tooltip = stat.tooltip_string();
                    if tooltip == "*" && cfg!(not(feature = "debugutil")) {
                        continue;
                    }
                    let title = super::fix_case(s);
                    let n = node! {
                        skill(key=s.to_owned(), title = title.clone(), tooltip=tooltip.to_owned()) {
                            skill_clip(value=f64::from(page.stats[stat.index])) {
                                skill_bar
                            }
                            label {
                                @text(title)
                            }
                        }
                    };
                    skills.add_child(n);
                }
            }

            if let Some(prev) = query!(ui, button(id="prev")).next() {
                prev.set_property("disabled", self.page == 0);
            }
            if let Some(next) = query!(ui, button(id="next")).next() {
                next.set_property("disabled", self.page == self.num_pages - 1);
            }
            self.current_page = page;
        });
        evt.handle_event_if::<StaffPageInfo, _, _>(|evt| evt.request_id == req_id, |page| {
            self.page = 0;
            self.num_pages = page.num_pages;
            if let Some(description) = query!(ui, preview > description > @text).next() {
                description.set_text("No staff available for hire");
            }
            if let Some(price_tag) = query!(ui, price_tag).next() {
                price_tag.set_property("can_afford", false);
                if let Some(txt) = query!(price_tag, @text).next() {
                    txt.set_text("");
                }
            }

            if let Some(hire) = query!(ui, button(id="hire")).next() {
                hire.set_property("disabled", true);
            }
            if let Some(skills) = query!(ui, skills).next() {
                // Remove old skills
                for skill in query!(skills, skill).matches() {
                    skills.remove_child(skill);
                }
            }

            if let Some(prev) = query!(ui, button(id="prev")).next() {
                prev.set_property("disabled", true);
            }
            if let Some(next) = query!(ui, button(id="next")).next() {
                next.set_property("disabled", true);
            }
        });
        */
        evt.handle_event::<Previous, _>(|_| {
            self.page = self.page.saturating_sub(1);
            let e_id = self.staff_list[self.selected_staff].entity.clone();
            self.request_ticket = Some(instance.request_manager.request(player::StaffPage {
                page: self.page,
                staff_key: e_id,
            }));
            if let Some(prev) = query!(ui, button(id = "prev")).next() {
                prev.set_property("disabled", true);
            }
            if let Some(next) = query!(ui, button(id = "next")).next() {
                next.set_property("disabled", true);
            }
        });
        evt.handle_event::<Next, _>(|_| {
            self.page = if self.page < self.num_pages - 1 {
                self.page + 1
            } else {
                self.page
            };
            let e_id = self.staff_list[self.selected_staff].entity.clone();
            self.request_ticket = Some(instance.request_manager.request(player::StaffPage {
                page: self.page,
                staff_key: e_id,
            }));
            if let Some(prev) = query!(ui, button(id = "prev")).next() {
                prev.set_property("disabled", true);
            }
            if let Some(next) = query!(ui, button(id = "next")).next() {
                next.set_property("disabled", true);
            }
        });
        evt.handle_event_if::<super::CancelEvent, _, _>(
            |evt| evt.0.is_same(&ui),
            |_| {
                action = state::Action::Pop;
            },
        );
        evt.handle_event_if::<super::AcceptEvent, _, _>(
            |evt| evt.0.is_same(&ui),
            |_| {
                let e_id = self.staff_list[self.selected_staff].entity.borrow();

                let mut cmd: Command = PlaceStaff::new(e_id, self.selected_uid, (0.0, 0.0)).into();
                let mut proxy = super::GameProxy::proxy(state);
                try_cmd!(
                    instance.log,
                    cmd.execute(
                        &mut proxy,
                        &mut instance.player,
                        CommandParams {
                            log: &instance.log,
                            level: &mut instance.level,
                            engine: &instance.scripting,
                            entities: &mut instance.entities,
                            snapshots: &instance.snapshots,
                            mission_handler: instance.mission_handler.as_ref().map(|v| v.borrow()),
                        }
                    ),
                    {
                        instance.push_command(cmd, req);
                        action = state::Action::Switch(Box::new(PlaceStaffState::new(
                            Some(self.current_page.hire_price),
                            true,
                        )))
                    }
                );
            },
        );
        action
    }

    fn key_action(
        &mut self,
        _instance: &mut Option<GameInstance>,
        _state: &mut crate::GameState,
        action: keybinds::KeyAction,
        _mouse_pos: (i32, i32),
    ) -> state::Action {
        use crate::keybinds::KeyAction::*;

        match action {
            SystemMenu => state::Action::Pop,
            _ => state::Action::Nothing,
        }
    }
}

fn entity_variant(ty: &Type<entity::ClientComponent>) -> StatVariant {
    for c in &ty.components {
        if let entity::ClientComponent::Server(ServerComponent::Vars { ref vars }) = *c {
            match vars.as_str() {
                "student" => return Stats::STUDENT,
                "professor" => return Stats::PROFESSOR,
                "office_worker" => return Stats::OFFICE_WORKER,
                "janitor" => return Stats::JANITOR,
                _ => {}
            }
        }
    }
    Stats::STUDENT
}

struct SelectStaffEvent(usize);
struct Previous;
struct Next;

pub struct PlaceStaffState {
    last_move: time::Instant,
    can_remove: bool,
    ui: Option<ui::Node>,
    cost: Option<UniDollar>,
}

impl PlaceStaffState {
    pub fn new(cost: Option<UniDollar>, can_remove: bool) -> PlaceStaffState {
        PlaceStaffState {
            last_move: time::Instant::now(),
            ui: None,
            can_remove,
            cost,
        }
    }

    fn update_staff(&mut self, instance: &mut GameInstance) {
        let ui = assume!(instance.log, self.ui.clone());
        if let player::State::EditEntity { entity: Some(e) } = instance.player.state {
            if let Some(staff_name) = query!(ui, staff_name > @text).next() {
                let living = assume!(instance.log, instance.entities.get_component::<Living>(e));
                let ty = assume!(
                    instance.log,
                    instance
                        .asset_manager
                        .loader_open::<Loader<entity::ClientComponent>>(living.key.borrow())
                );
                let name = format!("{} {} - {}", living.name.0, living.name.1, ty.display_name);
                staff_name.set_text(name);
            }
            let cost = self.cost.unwrap_or_default();
            if let Some(price_tag) = query!(ui, price_tag > @text).next() {
                price_tag.set_text(format!("Cost: {}", cost));
            }
            if let Some(price_tag) = query!(ui, price_tag).next() {
                let money = instance.player.get_money();
                price_tag.set_property("can_afford", money >= cost || cost == UniDollar(0));
                if !self.can_remove {
                    price_tag
                        .parent()
                        .map(|v| v.remove_child(price_tag.clone()));
                }
            }
            if let Some(cancel) = query!(ui, button(id = "cancel")).next() {
                cancel.set_property("disabled", !self.can_remove);
            }
        }
    }
}

impl state::State for PlaceStaffState {
    fn copy(&self) -> Box<dyn state::State> {
        Box::new(PlaceStaffState {
            last_move: self.last_move,
            ui: self.ui.clone(),
            can_remove: self.can_remove,
            cost: self.cost,
        })
    }

    fn active(
        &mut self,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
    ) -> state::Action {
        let ui = state
            .ui_manager
            .create_node(assets::ResourceKey::new("base", "buy/place_staff"));
        self.ui = Some(ui.clone());
        let instance = assume!(state.global_logger, instance.as_mut());

        self.update_staff(instance);

        state
            .keybinds
            .add_collection(keybinds::KeyCollection::PlaceStaff);
        state::Action::Nothing
    }

    fn inactive(&mut self, _instance: &mut Option<GameInstance>, state: &mut crate::GameState) {
        if let Some(ui) = self.ui.take() {
            state.ui_manager.remove_node(ui);
        }
    }

    fn removed(&mut self, _instance: &mut Option<GameInstance>, state: &mut crate::GameState) {
        state
            .keybinds
            .remove_collection(keybinds::KeyCollection::PlaceStaff);
    }

    fn tick(
        &mut self,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
    ) -> state::Action {
        let instance = assume!(state.global_logger, instance.as_mut());

        if let player::State::EditEntity { entity: None } = instance.player.state {
            // Waiting for the server to sync
            let mask = instance
                .entities
                .mask_for::<server::entity::SelectedEntity>();
            let other = instance.entities.iter_mask(&mask).find(|v| {
                let sel = assume!(
                    state.global_logger,
                    instance
                        .entities
                        .get_component::<server::entity::SelectedEntity>(*v)
                );
                sel.holder == instance.player.id
            });
            if let Some(other) = other {
                instance.player.state = player::State::EditEntity {
                    entity: Some(other),
                };
                instance
                    .entities
                    .add_component(other, server::entity::Frozen);
                {
                    let pos = assume!(
                        state.global_logger,
                        instance
                            .entities
                            .get_component_mut::<server::entity::Position>(other)
                    );
                    pos.y = 0.2;
                }
                self.update_staff(instance);
            }
        }

        state::Action::Nothing
    }

    fn key_action_req(
        &mut self,
        req: &mut state::CaptureRequester,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
        action: keybinds::KeyAction,
        mouse_pos: (i32, i32),
    ) -> state::Action {
        use crate::keybinds::KeyAction::*;
        let instance = assume!(state.global_logger, instance.as_mut());
        match action {
            PlacementFinish => {
                let (lx, ly) = state.renderer.mouse_to_level(mouse_pos.0, mouse_pos.1);
                let mut cmd: Command = FinalizeStaffPlace::new((lx, ly)).into();
                let mut proxy = super::GameProxy::proxy(state);
                try_cmd!(
                    instance.log,
                    cmd.execute(
                        &mut proxy,
                        &mut instance.player,
                        CommandParams {
                            log: &instance.log,
                            level: &mut instance.level,
                            engine: &instance.scripting,
                            entities: &mut instance.entities,
                            snapshots: &instance.snapshots,
                            mission_handler: instance.mission_handler.as_ref().map(|v| v.borrow()),
                        }
                    ),
                    {
                        instance.push_command(cmd, req);
                        return state::Action::Pop;
                    }
                );
            }
            _ => {}
        }
        state::Action::Nothing
    }

    fn mouse_move_req(
        &mut self,
        req: &mut state::CaptureRequester,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
        mouse_pos: (i32, i32),
    ) -> state::Action {
        let instance = assume!(state.global_logger, instance.as_mut());
        let (lx, ly) = state.renderer.mouse_to_level(mouse_pos.0, mouse_pos.1);
        if let player::State::EditEntity { entity: Some(e) } = instance.player.state {
            let mut cmd: Command = MoveStaff::new((lx, ly)).into();
            let mut proxy = super::GameProxy::proxy(state);
            try_cmd!(
                instance.log,
                cmd.execute(
                    &mut proxy,
                    &mut instance.player,
                    CommandParams {
                        log: &instance.log,
                        level: &mut instance.level,
                        engine: &instance.scripting,
                        entities: &mut instance.entities,
                        snapshots: &instance.snapshots,
                        mission_handler: instance.mission_handler.as_ref().map(|v| v.borrow()),
                    }
                ),
                {
                    // For most commands the command must be also executed
                    // on the server to make sure the client and server stay
                    // in sync. This command however spams a lot therefor
                    // we limit this one specially.
                    // Whilst the server may be out of sync because of this
                    // the result of this move is only used for displaying
                    // to other clients (The end placement sends the final
                    // position anyway) so it can safely be ignored.
                    let now = time::Instant::now();
                    if now - self.last_move > time::Duration::from_secs(1) / 20 {
                        instance.push_command(cmd, req);
                        self.last_move = now;
                    }
                    if can_visit(
                        &*instance.level.tiles.borrow(),
                        &*instance.level.rooms.borrow(),
                        (lx * 4.0) as usize,
                        (ly * 4.0) as usize,
                    ) {
                        instance
                            .entities
                            .remove_component::<server::entity::InvalidPlacement>(e);
                    } else {
                        instance
                            .entities
                            .add_component(e, server::entity::InvalidPlacement);
                    }
                }
            );
        }
        state::Action::Nothing
    }

    fn ui_event_req(
        &mut self,
        req: &mut state::CaptureRequester,
        instance: &mut Option<GameInstance>,
        state: &mut crate::GameState,
        evt: &mut event::EventHandler,
    ) -> state::Action {
        let instance = assume!(state.global_logger, instance.as_mut());

        let mut action = state::Action::Nothing;

        let ui = assume!(state.global_logger, self.ui.clone());

        evt.handle_event_if::<super::CancelEvent, _, _>(
            |evt| evt.0.is_same(&ui),
            |_| {
                let mut cmd: Command = CancelPlaceStaff::new().into();
                let mut proxy = super::GameProxy::proxy(state);
                try_cmd!(
                    instance.log,
                    cmd.execute(
                        &mut proxy,
                        &mut instance.player,
                        CommandParams {
                            log: &instance.log,
                            level: &mut instance.level,
                            engine: &instance.scripting,
                            entities: &mut instance.entities,
                            snapshots: &instance.snapshots,
                            mission_handler: instance.mission_handler.as_ref().map(|v| v.borrow()),
                        }
                    ),
                    {
                        instance.push_command(cmd, req);
                        action = state::Action::Pop;
                    }
                );
            },
        );
        action
    }
}
