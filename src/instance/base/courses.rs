
use super::super::*;
use crate::state;
use crate::server::event;
use server::network;

pub(super) const DAYS: &[&str] = &[
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday"
];

struct RemoveCourse(course::CourseId);

pub struct CourseList {
    ui: Option<ui::Node>,

    request: Option<network::RequestTicket<player::CourseList>>,
    next_request: u32,
    request_open: Option<network::RequestTicket<player::CourseInfo>>,
}

struct EditCourse(course::CourseId);

impl CourseList {
    pub fn new() -> CourseList {
        CourseList {
            ui: None,
            request: None,
            next_request: 0,
            request_open: None,
        }
    }
}

impl state::State for CourseList {
    fn copy(&self) -> Box<dyn state::State> {
        Box::new(CourseList {
            ui: self.ui.clone(),
            request: self.request,
            next_request: self.next_request,
            request_open: self.request_open,
        })
    }

    fn added(&mut self, instance: &mut Option<GameInstance>, state: &mut crate::GameState) -> state::Action {
        let instance = assume!(state.global_logger, instance.as_mut());
        if !instance.player.state.is_none() {
            // Can't view courses whilst editing anything
            return state::Action::Pop;
        }
        state::Action::Nothing
    }

    fn active(&mut self, instance: &mut Option<GameInstance>, state: &mut crate::GameState) -> state::Action {
        let ui = state.ui_manager.create_node(ResourceKey::new("base", "manage/courses"));
        self.ui = Some(ui.clone());

        let instance = assume!(state.global_logger, instance.as_mut());

        self.request = Some(instance.request_manager.request(player::CourseList {}));

        state.ui_manager.events().emit(CloseWindowOthers(ui));
        state::Action::Nothing
    }

    fn tick(&mut self, instance: &mut Option<GameInstance>, state: &mut crate::GameState) -> state::Action {
        let instance = assume!(state.global_logger, instance.as_mut());

        if self.request.is_none() {
            if self.next_request == 0 {
                self.request = Some(instance.request_manager.request(player::CourseList {}));
            } else {
                self.next_request -= 1;
            }
        }

        if !instance.player.state.is_none() {
            // Can't place a room whilst one is already in progress
            state::Action::Pop
        } else {
            state::Action::Nothing
        }
    }

    fn inactive(&mut self, _instance: &mut Option<GameInstance>, state: &mut crate::GameState) {
        if let Some(ui) = self.ui.take() {
            state.ui_manager.remove_node(ui);
        }
    }

    fn ui_event_req(&mut self, req: &mut state::CaptureRequester, instance: &mut Option<GameInstance>, state: &mut crate::GameState, evt: &mut event::EventHandler) -> state::Action {
        let mut action = state::Action::Nothing;
        let ui = assume!(state.global_logger, self.ui.clone());

        let instance = assume!(state.global_logger, instance.as_mut());
        if let Some(req) = self.request {
            network::RequestManager::handle_reply(evt, req, |mut pck| {
                self.request = None;
                self.next_request = 20 * 5;

                let target = assume!(state.global_logger, query!(ui, scroll_panel > content).next());
                for c in target.children() {
                    target.remove_child(c);
                }

                pck.courses.0.sort_by(|a, b| a.deprecated.cmp(&b.deprecated));

                for c in pck.courses.0 {
                    let uid = c.uid;
                    let node = node! {
                        course_entry(
                            on_click = ui::MethodDesc::<ui::MouseUpEvent>::native(move |evt, _, _| {
                                evt.emit(EditCourse(uid));
                                true
                            }),
                            deprecated = c.deprecated
                        ) {
                            header {
                                title {
                                    @text(if c.deprecated {
                                        format!("{} (Marked to remove)", c.name)
                                    } else {
                                        c.name
                                    })
                                }
                            }
                            info {
                                info_text {
                                    @text(format!("Students: {}", c.students))
                                }
                                info_text {
                                    @text(format!("Cost: {}", c.cost))
                                }
                                spacer
                                info_text {
                                    @text(format!("Average Grade: {:?}", c.average_grade))
                                }
                                info_text {
                                    @text(format!("Problems: {}", c.problems))
                                }
                                button(id="remove".to_string(), on_click=ui::MethodDesc::<ui::MouseUpEvent>::native(move |evt, _, _| {
                                    evt.emit(RemoveCourse(uid));
                                    true
                                })) {
                                    content {
                                        @text("Remove")
                                    }
                                }
                            }
                        }
                    };

                    let timetable = node!(course_timetable_overview);
                    for (day, info) in DAYS.iter().zip(c.timetable.iter()) {
                        let d = node! {
                            course_day {
                                center {
                                    label {
                                        @text(*day)
                                    }
                                }
                            }
                        };
                        for p in info {
                            d.add_child(node!(course_period(booked=*p)));
                        }

                        timetable.add_child(d);
                    }
                    node.add_child(timetable);

                    target.add_child(node);
                }
            });
        }
        if let Some(req) = self.request_open {
            network::RequestManager::handle_reply(evt, req, |pck| {
                self.request_open = None;
                action = state::Action::Switch(Box::new(CourseEdit::new(pck.course, false)))
            });
        }
        evt.handle_event::<RemoveCourse, _>(|evt| {
            let mut cmd: Command = DeprecateCourse::new(evt.0).into();
            let mut proxy = super::GameProxy::proxy(state);
            try_cmd!(instance.log, cmd.execute(&mut proxy, &mut instance.player, CommandParams {
                log: &instance.log,
                level: &mut instance.level,
                engine: &instance.scripting,
                entities: &mut instance.entities,
                snapshots: &instance.snapshots,
                mission_handler: instance.mission_handler.as_ref().map(|v| v.borrow()),
            }), {
                instance.push_command(cmd, req);
                self.next_request = 20;
            });
        });

        evt.handle_event_if::<super::CloseWindowOthers, _, _>(|evt| {
            // Handle in event_if in order to not consume the event and let
            // other windows read it too
            if !evt.0.is_same(&ui) {
                action = state::Action::Pop;
            }
            false
        }, |_| {});
        evt.handle_event_if::<super::CancelEvent, _, _>(|evt| evt.0.is_same(&ui), |_| {
            action = state::Action::Pop;
        });
        evt.handle_event_if::<super::AcceptEvent, _, _>(|evt| evt.0.is_same(&ui), |_| {
            let lm = assume!(state.global_logger, instance.entities.get_component::<course::LessonManager>(Container::WORLD));
            let course = course::NetworkCourse {
                // With an id of 0 the server will generate an id when saved
                uid: course::CourseId(0),
                name: "Course Name".into(),
                group: lm.groups[0].clone(),
                timetable: Default::default(),
                cost: UniDollar(1000),
            };
            action = state::Action::Switch(Box::new(CourseEdit::new(course, true)))
        });
        evt.handle_event::<EditCourse, _>(|evt| {
            // Request the full course information from the server
            // and block the user interface until we get it
            self.request_open = Some(instance.request_manager.request(player::CourseInfo {
                uid: evt.0,
            }));
            ui.add_child(node!(darken_ui));
        });
        action
    }

    fn key_action(&mut self, _instance: &mut Option<GameInstance>, _state: &mut crate::GameState, action: keybinds::KeyAction, _mouse_pos: (i32, i32)) -> state::Action {
        use crate::keybinds::KeyAction::*;

        match action {
            SystemMenu => state::Action::Pop,
            _ => state::Action::Nothing,
        }
    }
}

struct EditPeriod(usize, usize);
struct BlockPeriod(usize, usize);
struct AutoFill;

struct CourseEdit {
    ui: Option<ui::Node>,
    course: course::NetworkCourse,

    autofill: Option<AutoFillState>,
    blocked_periods: [[bool; 4]; 7],

    is_new: bool,
    last_cost: String,
}

struct AutoFillState {
    lessons: Vec<AutoFillLesson>,
    current_day: usize,
    current_period: usize,
    current_lesson: usize,

    blocked_periods: [[bool; 4]; 7],

    request: Option<network::RequestTicket<player::LessonValidOptions>>,
    reply: Option<player::LessonValidOptionsReply>,

    first_try: bool,
}

struct AutoFillLesson {
    key: ResourceKey<'static>,
    valid_days: [bool; 7],
    valid_staff: FNVSet<ecs::Entity>,
    missing_lessons: i32,
}

impl AutoFillState {
    fn new(
        target: &course::NetworkCourse, blocked_periods: [[bool; 4]; 7],
        snapshots: &snapshot::Snapshots, lm: &course::LessonManager
    ) -> AutoFillState {
        let mut lessons: Vec<AutoFillLesson> = Vec::new();
        for (di, d) in target.timetable.iter().enumerate() {
            for p in d {
                if let course::NetworkCourseEntry::Lesson{ref key, ref rooms} = p {
                    let lesson = if let Some(l) = lessons.iter_mut().find(|v| v.key == *key) {
                        l
                    } else {
                        let l = lm.get(key.borrow()).expect("Missing lesson info");
                        lessons.push(AutoFillLesson {
                            key: key.clone(),
                            valid_days: [true; 7],
                            valid_staff: Default::default(),
                            missing_lessons: l.required_lessons as i32,
                        });
                        lessons.last_mut().expect("Missing just added element")
                    };
                    lesson.missing_lessons -= 1;
                    lesson.valid_days[di] = false;
                    lesson.valid_staff.extend(
                        rooms.0.iter()
                            .map(|v| v.staff)
                            .filter_map(|v| snapshots.get_entity_by_id(v))
                    );
                }
            }
        }
        AutoFillState {
            lessons,
            current_day: 0,
            current_period: 0,
            current_lesson: 0,
            first_try: true,

            request: None,
            reply: None,
            blocked_periods,
        }
    }

    fn handle_event(
        &mut self,
        evt: &mut event::EventHandler,
    ) {
        if let Some(req) = self.request {
            network::RequestManager::handle_reply(evt, req, |pck| {
                self.request = None;
                self.reply = Some(pck);
            });
        }
    }

    fn do_fill(
        &mut self,
        target: &mut course::NetworkCourse,
        snapshots: &snapshot::Snapshots,
        entities: &mut Container,
        req: &mut network::RequestManager,
    ) -> bool {
        use rand::seq::SliceRandom;
        if self.request.is_some() {
            return false;
        }
        let mut rng = rand::thread_rng();
        loop {
            if !self.lessons.iter().any(|v| v.missing_lessons > 0) {
                return true;
            }
            if self.current_lesson >= self.lessons.len() {
                self.current_lesson = 0;
                self.current_period += 1;
            }
            if self.current_period >= NUM_TIMETABLE_SLOTS {
                self.current_day += 1;
                self.current_period = 0;
            }
            if self.current_day >= 7 {
                if self.first_try {
                    self.first_try = false;
                    self.current_day = 0;
                    // Remove the one lesson a day limit
                    self.lessons.iter_mut()
                        .for_each(|v| v.valid_days = [true; 7]);
                } else {
                    return true;
                }
            }
            let cur = &mut target.timetable[self.current_day][self.current_period];
            if !cur.is_free() || self.blocked_periods[self.current_day][self.current_period] {
                self.current_lesson = 0;
                self.current_period += 1;
                continue;
            }

            let lesson = &mut self.lessons[self.current_lesson];
            if !lesson.valid_days[self.current_day] {
                self.current_lesson += 1;
                continue;
            }
            if let Some(reply) = self.reply.take() {
                if let Some(staff) = reply.staff.0.iter()
                    .filter_map(|v| snapshots.get_entity_by_id(v.0))
                    .find(|v| lesson.valid_staff.contains(v))
                {
                    if let Some(room) = reply.rooms.0.choose(&mut rng) {
                        *cur = course::NetworkCourseEntry::Lesson{
                            key: lesson.key.clone(),
                            rooms: delta_encode::AlwaysVec(vec![course::NetworkLessonRoom {
                                staff: entities.get_component::<NetworkId>(staff).map(|v| v.0).unwrap_or(0),
                                room: *room,
                            }])
                        };
                        lesson.missing_lessons -= 1;
                        if self.first_try {
                            lesson.valid_days[self.current_day] = false;
                        }
                    }
                }
            } else {
                self.request = Some(req.request(player::LessonValidOptions {
                    course: target.uid,
                    key: lesson.key.clone(),
                    day: self.current_day as u8,
                    period: self.current_period as u8,
                }));
                return false;
            }

            self.current_lesson += 1;
        }
    }
}

impl CourseEdit {
    pub fn new(course: course::NetworkCourse, is_new: bool) -> CourseEdit {
        CourseEdit {
            ui: None,
            course,
            is_new,
            last_cost: "0".into(),
            autofill: None,
            blocked_periods: [[false; 4]; 7]
        }
    }
}

impl state::State for CourseEdit {
    fn copy(&self) -> Box<dyn state::State> {
        Box::new(CourseEdit {
            ui: self.ui.clone(),
            course: self.course.clone(),
            is_new: self.is_new,
            last_cost: self.last_cost.clone(),
            autofill: None,
            blocked_periods: self.blocked_periods,
        })
    }

    fn added(&mut self, instance: &mut Option<GameInstance>, state: &mut crate::GameState) -> state::Action {
        let instance = assume!(state.global_logger, instance.as_mut());
        if !instance.player.state.is_none() {
            // Can't place a room whilst one is already in progress
            return state::Action::Pop;
        }
        state::Action::Nothing
    }

    fn active(&mut self, instance: &mut Option<GameInstance>, state: &mut crate::GameState) -> state::Action {
        let ui = state.ui_manager.create_node(ResourceKey::new("base", "manage/course_edit"));
        self.ui = Some(ui.clone());

        let instance = assume!(state.global_logger, instance.as_mut());
        let lm = assume!(state.global_logger, instance.entities.get_component::<course::LessonManager>(Container::WORLD));

        if let Some(name) = query!(ui, textbox(id="name") > content > @text).next() {
            name.set_text(self.course.name.as_str());
        }

        if let Some(autofill) = query!(ui, button(id="autofill")).next() {
            autofill.set_property("on_click", ui::MethodDesc::<ui::MouseUpEvent>::native(|evt, _, _| {
                evt.emit(AutoFill);
                true
            }));
        }

        if let Some(subjects) = query!(ui, dropdown(id="subjects")).next() {
            if self.is_new {
                let mut count = 0;

                let mut value = 1;
                for group in &lm.groups {
                    // TODO: Filter subjects that can't be used currently
                    //       somehow?
                    subjects.set_property(&format!("option{}", count + 1), super::fix_case(group));
                    if self.course.group == *group {
                        value = count + 1;
                    }

                    count += 1;
                }

                subjects.set_property("value", value);
                subjects.set_property("options", count);
            } else {
                subjects.set_property("option1", super::fix_case(&self.course.group));
                subjects.set_property("disabled", true);
            }
        }

        if let Some(overview) = query!(ui, course_timetable_overview).next() {
            for (idx, day) in DAYS.iter().enumerate() {
                let n = node! {
                    course_day(weekend=day.starts_with('S')) {
                        center {
                            label {
                                @text(*day)
                            }
                        }
                    }
                };
                for period in 0 .. 4 {
                    let info = &self.course.timetable[idx][period];
                    let p = node!{
                        course_period(
                            booked=!info.is_free(),
                            blocked=self.blocked_periods[idx][period],
                            day = idx as i32,
                            period = period as i32
                        )
                    };
                    let tooltip = match info {
                        course::NetworkCourseEntry::Free => "Free Period".to_string(),
                        course::NetworkCourseEntry::Lesson{ref key, ref rooms} => {
                            let lesson = assume!(state.global_logger, lm.get(key.borrow()));
                            format!("{}\nBooked rooms: {}", lesson.name, rooms.0.len())
                        },
                    };
                    p.set_property("tooltip", tooltip);
                    p.set_property("on_click", ui::MethodDesc::<ui::MouseUpEvent>::native(move |evt, _, p| {
                        if p.button == ui::MouseButton::Left {
                            evt.emit(EditPeriod(idx, period));
                        } else {
                            evt.emit(BlockPeriod(idx, period));
                        }
                        true
                    }));
                    n.add_child(p);
                }
                overview.add_child(n);
            }
        }

        state::Action::Nothing
    }

    fn tick(&mut self, instance: &mut Option<GameInstance>, state: &mut crate::GameState) -> state::Action {
        let instance = assume!(state.global_logger, instance.as_mut());

        let ui = assume!(state.global_logger, self.ui.clone());

        let remove = if let Some(autofill) = self.autofill.as_mut() {
            autofill.do_fill(&mut self.course, &instance.snapshots, &mut instance.entities, &mut instance.request_manager)
        } else {
            false
        };
        if remove {
            let mut new = Self::new(self.course.clone(), self.is_new);
            new.blocked_periods = self.blocked_periods;
            return state::Action::Switch(Box::new(new));
        }

        let lm = assume!(state.global_logger, instance.entities.get_component::<course::LessonManager>(Container::WORLD));

        if let Some(subjects) = query!(ui, dropdown(id="subjects")).next() {
            let val: i32 = subjects.get_property("value").unwrap_or(1) - 1;
            let sgroup = &lm.groups[val as usize];
            if self.course.group != *sgroup && self.is_new {
                self.course.group = sgroup.clone();
                self.course.timetable = Default::default();
                return state::Action::Switch(Box::new(Self::new(self.course.clone(), self.is_new)));
            }
        }
        if let Some(name) = query!(ui, textbox(id="name") > content > @text).next() {
            let name = assume!(state.global_logger, name.text());
            if *name != self.course.name {
                self.course.name = (*name).into();
            }
        }

        if let Some(cost_ui) = query!(ui, textbox(id="cost") > content > @text).next() {
            let cost = assume!(state.global_logger, cost_ui.text());
            if *cost != self.last_cost && !cost.is_empty() {
                if let Ok(c) = cost.parse() {
                    self.course.cost = UniDollar(c);
                    self.last_cost = (*cost).into();
                } else {
                    drop(cost);
                    cost_ui.set_text(&*self.last_cost);
                }
            }
        }

        if !instance.player.state.is_none() {
            state::Action::Pop
        } else {
            state::Action::Nothing
        }
    }

    fn inactive(&mut self, _instance: &mut Option<GameInstance>, state: &mut crate::GameState) {
        if let Some(ui) = self.ui.take() {
            state.ui_manager.remove_node(ui);
        }
    }

    fn ui_event_req(&mut self, req: &mut state::CaptureRequester, instance: &mut Option<GameInstance>, state: &mut crate::GameState, evt: &mut event::EventHandler) -> state::Action {
        let instance = assume!(state.global_logger, instance.as_mut());

        if let Some(autofill) = self.autofill.as_mut() {
            autofill.handle_event(evt);
        }

        let mut action = state::Action::Nothing;
        let ui = assume!(state.global_logger, self.ui.clone());
        evt.handle_event_if::<super::CancelEvent, _, _>(|evt| evt.0.is_same(&ui), |_| {
            action = state::Action::Switch(Box::new(CourseList::new()))
        });
        evt.handle_event_if::<super::AcceptEvent, _, _>(|evt| evt.0.is_same(&ui), |_| {
            let mut cmd: Command = UpdateCourse::new(self.course.clone()).into();
            let mut proxy = super::GameProxy::proxy(state);
            try_cmd!(instance.log, cmd.execute(&mut proxy, &mut instance.player, CommandParams {
                log: &instance.log,
                level: &mut instance.level,
                engine: &instance.scripting,
                entities: &mut instance.entities,
                snapshots: &instance.snapshots,
                mission_handler: instance.mission_handler.as_ref().map(|v| v.borrow()),
            }), {
                instance.push_command(cmd, req);
                action = state::Action::Switch(Box::new(CourseList::new()))
            });
        });
        evt.handle_event::<EditPeriod, _>(|EditPeriod(day, period)| {
            action = state::Action::Switch(Box::new(CoursePeriodEdit::new(self.course.clone(), self.is_new, day, period)))
        });
        evt.handle_event::<BlockPeriod, _>(|BlockPeriod(day, period)| {
            self.blocked_periods[day][period] = !self.blocked_periods[day][period];
            if let Some(p) = query!(ui, course_period(day = day as i32,period = period as i32)).next() {
                p.set_property("blocked", self.blocked_periods[day][period]);
            }
        });
        evt.handle_event::<AutoFill, _>(|_| {
            ui.add_child(node!(autofill {
                info {
                    content {
                        @text("AutoFill in progress")
                    }
                }
            }));
            let lm = assume!(state.global_logger, instance.entities.get_component::<course::LessonManager>(Container::WORLD));
            self.autofill = Some(AutoFillState::new(&self.course, self.blocked_periods, &instance.snapshots, &lm));
        });
        action
    }

    fn key_action(&mut self, _instance: &mut Option<GameInstance>, _state: &mut crate::GameState, action: keybinds::KeyAction, _mouse_pos: (i32, i32)) -> state::Action {
        use crate::keybinds::KeyAction::*;

        match action {
            SystemMenu => state::Action::Switch(Box::new(CourseList::new())),
            _ => state::Action::Nothing,
        }
    }
}

struct CoursePeriodEdit {
    ui: Option<ui::Node>,

    course: course::NetworkCourse,
    is_new_course: bool,
    day: usize,
    period: usize,

    request: Option<network::RequestTicket<player::LessonValidOptions>>,

    selected_lesson: i32,
    focused_entity: Option<i32>,
    focused_room: Option<i32>,

    matching_entities: Vec<ecs::Entity>,
    matching_rooms: Vec<room::Id>,
}

struct AddRoom;
struct RemoveRoom(usize);

impl CoursePeriodEdit {
    pub fn new(course: course::NetworkCourse, is_new_course: bool, day: usize, period: usize) -> CoursePeriodEdit {
        CoursePeriodEdit {
            ui: None,

            course,
            is_new_course,
            day,
            period,

            request: None,

            selected_lesson: 1,
            focused_entity: None,
            focused_room: None,

            matching_entities: Vec::new(),
            matching_rooms: Vec::new(),
        }
    }
}

fn gen_room_list(
    instance: &mut GameInstance,
    state: &mut crate::GameState,
    ui: &ui::Node,
    entry: &course::NetworkCourseEntry,
    matching_entities: &[ecs::Entity],
    matching_rooms: &[room::Id],
) {
    let erooms = if let course::NetworkCourseEntry::Lesson{ref rooms, ..} = &entry {
        rooms
    } else {
        return;
    };

    let rooms = assume!(state.global_logger, query!(ui, content(id="rooms")).next());
    // Remove existing entries
    for c in rooms.children() {
        rooms.remove_child(c);
    }

    for (idx, rm) in erooms.0.iter().enumerate() {
        let node = node! {
            period_room {
                room_selection {
                    @text("Room: ")
                    dropdown(
                        idx = idx as i32,
                        value = 1,
                        options = 1,
                        option1 = "Select Room".to_string()
                    ) {
                        content {
                            @text("")
                        }
                    }
                }
                staff_selection {
                    @text("Professor: ")
                    dropdown(
                        idx = idx as i32,
                        value = 1,
                        options = 1,
                        option1 = "Select Professor".to_string()
                    ) {
                        content {
                            @text("")
                        }
                    }
                }
                buttons {
                    spacer
                    button(
                        on_click = ui::MethodDesc::<ui::MouseUpEvent>::native(move |evt, _, _| {
                            evt.emit(RemoveRoom(idx));
                            true
                        }),
                        id = "remove".to_string()
                    ) {
                        content {
                            @text("Remove")
                        }
                    }
                }
            }
        };
        rooms.add_child(node.clone());

        let mut index = 2;
        let list = assume!(state.global_logger,
            query!(node, staff_selection > dropdown).next()
        );

        let mut selected = 1;
        for e in &*matching_entities {
            if Some(*e) == instance.snapshots.get_entity_by_id(rm.staff) {
                selected = index;
            }
            let living = assume!(state.global_logger, instance.entities.get_component::<Living>(*e));
            list.set_property(&format!("option{}", index), format!("{} {}", living.name.0, living.name.1));
            index += 1;
        }
        list.set_property("options", index - 1);
        list.set_property("value", selected);

        let list = assume!(state.global_logger,
            query!(node, room_selection > dropdown).next()
        );
        index = 2;
        selected = 1;
        for (id, room) in matching_rooms.iter()
            .map(|v| (*v, instance.level.get_room_info(*v)))
        {
            if id == rm.room {
                selected = index;
            }
            let info = assume!(state.global_logger, state.asset_manager.loader_open::<room::Loader>(room.key.borrow()));
            list.set_property(&format!("option{}", index), info.name.clone());
            index += 1;
        }
        list.set_property("options", index - 1);
        list.set_property("value", selected);
    }
}

impl state::State for CoursePeriodEdit {
    fn copy(&self) -> Box<dyn state::State> {
        Box::new(CoursePeriodEdit {
            ui: self.ui.clone(),

            course: self.course.clone(),
            is_new_course: self.is_new_course,
            day: self.day,
            period: self.period,

            request: self.request,

            focused_entity: self.focused_entity,
            focused_room: self.focused_room,
            selected_lesson: self.selected_lesson,

            matching_entities: self.matching_entities.clone(),
            matching_rooms: self.matching_rooms.clone(),
        })
    }

    fn added(&mut self, instance: &mut Option<GameInstance>, state: &mut crate::GameState) -> state::Action {
        let instance = assume!(state.global_logger, instance.as_mut());
        if !instance.player.state.is_none() {
            // Can't place a room whilst one is already in progress
            return state::Action::Pop;
        }
        state::Action::Nothing
    }

    fn active(&mut self, instance: &mut Option<GameInstance>, state: &mut crate::GameState) -> state::Action {
        let ui = state.ui_manager.create_node(ResourceKey::new("base", "manage/course_edit_period"));
        self.ui = Some(ui.clone());

        let instance = assume!(state.global_logger, instance.as_mut());

        if let Some(lesson) = query!(ui, dropdown(id = "lesson")).next() {
            let lm = assume!(state.global_logger, instance.entities.get_component::<course::LessonManager>(Container::WORLD));
            let mut count = 1;
            lesson.set_property("option1", "Free Period".to_string());
            for l in lm.lessons_in_group(&self.course.group) {
                if self.course.timetable[self.day][self.period].is_lesson_type(l.key.borrow()) {
                    self.selected_lesson = count + 1;
                }
                // TODO: Filter impossible lessons (or gray out?)
                lesson.set_property(&format!("option{}", count + 1), l.name.clone());
                count += 1;
            }
            lesson.set_property("options", count);
            lesson.set_property("value", self.selected_lesson);
        }

        if let course::NetworkCourseEntry::Lesson{ref key, ..} = self.course.timetable[self.day][self.period] {
            ui.add_child(node!(darken_ui));
            self.request = Some(instance.request_manager.request(player::LessonValidOptions {
                course: self.course.uid,
                key: key.clone(),
                day: self.day as u8,
                period: self.period as u8,
            }));
        }

        if let Some(add) = query!(ui, button(id="add_room")).next() {
            if self.selected_lesson == 1 {
                add.set_property("disabled", true);
            }
            add.set_property("on_click", ui::MethodDesc::<ui::MouseUpEvent>::native(|evt, _, _| {
                evt.emit(AddRoom);
                true
            }));
        }

        state::Action::Nothing
    }

    fn tick(&mut self, instance: &mut Option<GameInstance>, state: &mut crate::GameState) -> state::Action {
        let instance = assume!(state.global_logger, instance.as_mut());
        if !instance.player.state.is_none() {
            state::Action::Pop
        } else {
            let ui = assume!(state.global_logger, self.ui.as_ref());

            let lm = assume!(state.global_logger, instance.entities.get_component::<course::LessonManager>(Container::WORLD));

            if let Some(lesson) = query!(ui, dropdown(id="lesson")).next() {
                let val = lesson.get_property("value").unwrap_or(1);
                if val != self.selected_lesson {
                    // Clear the booked rooms
                    self.course.timetable[self.day][self.period] = if val == 1 {
                        if let Some(details) = query!(ui, lesson_details).next() {
                            for c in details.children() {
                                details.remove_child(c);
                            }
                            details.add_child(ui::Node::new_text("Free period.\nStudents are able to relax during this time"));
                        }
                        course::NetworkCourseEntry::Free
                    } else {
                        let lesson = assume!(state.global_logger, lm.lessons_in_group(&self.course.group).nth((val - 2) as usize));
                        if let Some(details) = query!(ui, lesson_details).next() {
                            for c in details.children() {
                                details.remove_child(c);
                            }
                            details.add_child(ui::Node::new_text(lesson.description.clone()));
                            details.add_child(ui::Node::new_text(format!("\n\nRequired lessons: {}", lesson.required_lessons)));
                            details.add_child(ui::Node::new_text("\n\nCan be taught by:"));
                            for key in &lesson.valid_staff {
                                let staff = assume!(state.global_logger, state.asset_manager.loader_open::<Loader<entity::ClientComponent>>(key.borrow()));
                                details.add_child(ui::Node::new_text("\n"));
                                details.add_child(ui::Node::new("bullet_point"));
                                let n = ui::Node::new_text(format!(" {}", staff.display_name));
                                n.set_property("staff", true);
                                details.add_child(n);
                            }
                            details.add_child(ui::Node::new_text("\n\nCan be taught in:"));
                            for key in &lesson.valid_rooms {
                                let room = assume!(state.global_logger, state.asset_manager.loader_open::<room::Loader>(key.borrow()));
                                details.add_child(ui::Node::new_text("\n"));
                                details.add_child(ui::Node::new("bullet_point"));
                                let n = ui::Node::new_text(format!(" {}", room.name));
                                n.set_property("room", true);
                                details.add_child(n);
                            }
                        }
                        course::NetworkCourseEntry::Lesson {
                            key: lesson.key.clone(),
                            rooms: AlwaysVec(Vec::new()),
                        }
                    };

                    let rooms = assume!(state.global_logger, query!(ui, content(id="rooms")).next());
                    for pr in query!(rooms, period_room).matches() {
                        rooms.remove_child(pr);
                    }

                    self.selected_lesson = val;

                    if let Some(add) = query!(ui, button(id="add_room")).next() {
                        add.set_property("disabled", self.selected_lesson == 1);
                    }

                    if let course::NetworkCourseEntry::Lesson{ref key, ..} = self.course.timetable[self.day][self.period] {
                        ui.add_child(node!(darken_ui));
                        self.request = Some(instance.request_manager.request(player::LessonValidOptions {
                            course: self.course.uid,
                            key: key.clone(),
                            day: self.day as u8,
                            period: self.period as u8,
                        }));
                    }
                }
            }

            let mut used_staff = FNVMap::default();
            let mut used_rooms = FNVMap::default();

            let mut valid = match &self.course.timetable[self.day][self.period] {
                course::NetworkCourseEntry::Free => true,
                course::NetworkCourseEntry::Lesson{ref rooms, ..} => {
                    for rm in &rooms.0 {
                        if let Some(staff) = instance.snapshots.get_entity_by_id(rm.staff) {
                            *used_staff.entry(staff).or_insert(0) += 1;
                        }
                        *used_rooms.entry(rm.room).or_insert(0) += 1;
                    }
                    !rooms.0.is_empty()
                },
            };

            let mut set = false;
            for list in query!(ui, staff_selection > dropdown).matches() {
                if let Some(val) = list.get_property::<i32>("$value_hover") {
                    set = true;
                    if self.focused_entity != Some(val) && val >= 2 {
                        self.focused_entity = Some(val);


                        if let Some(e) = self.matching_entities.get(val as usize - 2)
                            .cloned()
                            .filter(|v| instance.entities.is_valid(*v))
                        {
                            return state::Action::Push(Box::new(super::EntityInfoState::new(e)));
                        }
                    }
                }
                if let (Some(val), Some(idx)) = (list.get_property::<i32>("value"), list.get_property::<i32>("idx")) {
                    if let course::NetworkCourseEntry::Lesson{ref mut rooms, ..} = &mut self.course.timetable[self.day][self.period] {
                        if val >= 2 {
                            if let Some(e) = self.matching_entities.get(val as usize - 2)
                                .cloned()
                                .filter(|v| instance.entities.is_valid(*v))
                            {
                                let self_valid = list.get_property("valid").unwrap_or(true);
                                let this_valid = used_staff.get(&e)
                                    .cloned()
                                    .unwrap_or(0) == 1;

                                if self_valid != this_valid {
                                    list.set_property("valid", this_valid);
                                }
                                if !this_valid {
                                    valid = false;
                                }
                                if let Some(e) = instance.entities.get_component::<NetworkId>(e).map(|v| v.0) {
                                    rooms.0[idx as usize].staff = e;
                                }
                            }
                        } else {
                            valid = false;
                        }
                    }
                }
            }
            if !set {
                self.focused_entity = None;
            }

            let mut set = false;
            let player = instance.player.id;
            for list in query!(ui, room_selection > dropdown).matches() {
                if let Some(val) = list.get_property::<i32>("$value_hover") {
                    set = true;
                    if self.focused_room != Some(val) && val >= 2 {
                        self.focused_room = Some(val);

                        if let Some(room) = self.matching_rooms.get(val as usize - 2)
                            .map(|v| instance.level.get_room_info(*v))
                            .filter(|v| v.owner == player)
                        {
                            state.renderer.suggest_camera_position(
                                room.area.min.x as f32 + room.area.width() as f32 / 2.0,
                                room.area.min.y as f32 + room.area.height() as f32 / 2.0,
                                45.0
                            );
                        }
                        break;
                    }
                }
                if let (Some(val), Some(idx)) = (list.get_property::<i32>("value"), list.get_property::<i32>("idx")) {
                    if let course::NetworkCourseEntry::Lesson{ref mut rooms, ..} = &mut self.course.timetable[self.day][self.period] {
                        if val >= 2 {
                            if let Some(room) = self.matching_rooms.get(val as usize - 2)
                                .map(|v| instance.level.get_room_info(*v))
                                .filter(|v| v.owner == player)
                            {
                                let self_valid = list.get_property("valid").unwrap_or(true);
                                let this_valid = used_rooms.get(&room.id)
                                    .cloned()
                                    .unwrap_or(0) == 1;

                                if self_valid != this_valid {
                                    list.set_property("valid", this_valid);
                                }
                                if !this_valid {
                                    valid = false;
                                }
                                rooms.0[idx as usize].room = room.id;
                            }
                        }
                        if rooms.0.iter().any(|v| v.room == RoomId(0)) {
                            valid = false;
                        }
                    }
                }
            }
            if !set {
                self.focused_room = None;
            }

            if let Some(save) = query!(ui, button(id="save")).next() {
                let disabled = save.get_property("disabled").unwrap_or(false);
                if disabled == valid {
                    save.set_property("disabled", !valid);
                }
            }


            state::Action::Nothing
        }
    }

    fn inactive(&mut self, _instance: &mut Option<GameInstance>, state: &mut crate::GameState) {
        if let Some(ui) = self.ui.take() {
            state.ui_manager.remove_node(ui);
        }
    }

    fn ui_event(&mut self, instance: &mut Option<GameInstance>, state: &mut crate::GameState, evt: &mut event::EventHandler) -> state::Action {
        let instance = assume!(state.global_logger, instance.as_mut());

        let mut action = state::Action::Nothing;
        let ui = assume!(state.global_logger, self.ui.clone());

        if let Some(req) = self.request {
            network::RequestManager::handle_reply(evt, req, |pck| {
                self.request = None;
                if let Some(darken) = query!(ui, darken_ui).next() {
                    ui.remove_child(darken);
                }

                self.matching_entities = pck.staff.0.into_iter()
                    .filter_map(|v| instance.snapshots.get_entity_by_id(v.0))
                    .collect();
                self.matching_rooms = pck.rooms.0;

                gen_room_list(
                    instance,
                    state,
                    &ui,
                    &self.course.timetable[self.day][self.period],
                    &self.matching_entities,
                    &self.matching_rooms,
                );
            });
        }

        evt.handle_event_if::<super::AcceptEvent, _, _>(|evt| evt.0.is_same(&ui), |_| {
            action = state::Action::Switch(Box::new(CourseEdit::new(self.course.clone(), self.is_new_course)))
        });
        evt.handle_event::<RemoveRoom, _>(|RemoveRoom(idx)| {
            if let course::NetworkCourseEntry::Lesson{ref mut rooms, ..} = &mut self.course.timetable[self.day][self.period] {
                rooms.0.remove(idx);
            }

            gen_room_list(
                instance,
                state,
                &ui,
                &self.course.timetable[self.day][self.period],
                &self.matching_entities,
                &self.matching_rooms,
            );
        });
        evt.handle_event::<AddRoom, _>(|_| {
            if let course::NetworkCourseEntry::Lesson{ref mut rooms, ..} = &mut self.course.timetable[self.day][self.period] {
                rooms.0.push(course::NetworkLessonRoom {
                    staff: 0xFF_FF_FF_FF,
                    room: room::Id(0),
                });
            }

            gen_room_list(
                instance,
                state,
                &ui,
                &self.course.timetable[self.day][self.period],
                &self.matching_entities,
                &self.matching_rooms,
            );
        });
        action
    }
}