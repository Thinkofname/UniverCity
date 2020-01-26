
use super::super::*;
use crate::state;
use crate::server::event;
use crate::server::assets;

pub struct StatsState {
    ui: Option<ui::Node>,
    tab: Tab,

    next_update: f64,
}

impl StatsState {
    pub fn new() -> StatsState {
        StatsState {
            ui: None,
            tab: Tab::Money,
            next_update: 0.0,
        }
    }
}

fn draw_money(instance: &mut crate::GameInstance, state: &mut crate::GameState, ui: &ui::Node) {
    use std::cmp::{min, max};
    let mut data = vec![];
    let mut min_val = UniDollar(0);
    let mut max_val = UniDollar(i64::min_value());
    for money in &instance.player.history {
        min_val = min(min_val, money.total);
        max_val = max(max_val, money.total);
    }
    if min_val == max_val {
        max_val += UniDollar(10);
    }
    max_val = min_val + UniDollar((((max_val - min_val).0 as f64 / 8.0).ceil() * 8.0) as i64);
    for money in &instance.player.history {
        let p = (money.total - min_val).0 as f64 / (max_val - min_val).0 as f64;
        data.push(p);
    }

    if let Some(area) = query!(ui, graph_area).next() {
        let spacing = (max_val - min_val) / 8;
        for i in 0 .. 9 {
            let n = node!{
                graph_label(pos = i) {
                    @text(format!("- {}", max_val - spacing * i))
                }
            };
            area.add_child(n);
        }
    }

    let mut graph = vec![0; 650 * 400 * 4];

    // Draw a grid
    for y in 0 .. 8 {
        for x in 0 .. 13 {
            for yy in 0 .. 50 {
                for xx in 0 .. 50 {
                    let idx = (x * 50 + xx + (y * 50 + yy) * 650) * 4;
                    let (r, g, b) = if xx == 0 || xx == 49 || yy == 0 || yy == 49 {
                        (167, 202, 214)
                    } else {
                        (255, 255, 255)
                    };
                    graph[idx    ] = r;
                    graph[idx + 1] = g;
                    graph[idx + 2] = b;
                    graph[idx + 3] = 255;
                }
            }
        }
    }

    for (xx, data) in data.windows(2).enumerate() {
        let a = (399.0 * data[0]) as i32;
        let b = (399.0 * data[1]) as i32;

        for (x, y) in draw_line(xx as i32 * 50, a, xx as i32 * 50 + 50, b) {
            if x < 650 && y < 400 {
                for yy in 0 .. y {
                    let idx = (x + (400 - 1 - yy as usize) * 650) * 4;
                    graph[idx    ] = 0;
                    graph[idx + 1] = 180;
                    graph[idx + 2] = 0;
                    graph[idx + 3] = 255;
                }
                let idx = (x + (400 - 1 - y as usize) * 650) * 4;
                graph[idx    ] = 0;
                graph[idx + 1] = 255;
                graph[idx + 2] = 0;
                graph[idx + 3] = 255;
            }
        }
    }

    state.renderer.update_image(ResourceKey::new("dynamic", "650@400@graph"), 650, 400, graph);
}

fn draw_income_outcome(instance: &mut crate::GameInstance, state: &mut crate::GameState, ui: &ui::Node) {
    use std::cmp::{min, max};
    let mut data = vec![];
    let mut min_val = UniDollar(0);
    let mut max_val = UniDollar(i64::min_value());
    for money in &instance.player.history {
        min_val = min(min_val, -money.outcome);
        max_val = max(max_val, money.income);
    }
    if min_val == max_val {
        max_val += UniDollar(10);
    }
    max_val = min_val + UniDollar((((max_val - min_val).0 as f64 / 8.0).ceil() * 8.0) as i64);
    for money in &instance.player.history {
        let i = (money.income - min_val).0 as f64 / (max_val - min_val).0 as f64;
        let o = (-money.outcome - min_val).0 as f64 / (max_val - min_val).0 as f64;
        data.push((i, o));
    }

    if let Some(area) = query!(ui, graph_area).next() {
        let spacing = (max_val - min_val) / 8;
        for i in 0 .. 9 {
            let n = node!{
                graph_label(pos = i) {
                    @text(format!("- {}", max_val - spacing * i))
                }
            };
            area.add_child(n);
        }
    }

    let mut graph = vec![0; 650 * 400 * 4];

    // Draw a grid
    for y in 0 .. 8 {
        for x in 0 .. 13 {
            for yy in 0 .. 50 {
                for xx in 0 .. 50 {
                    let idx = (x * 50 + xx + (y * 50 + yy) * 650) * 4;
                    let (r, g, b) = if xx == 0 || xx == 49 || yy == 0 || yy == 49 {
                        (167, 202, 214)
                    } else {
                        (255, 255, 255)
                    };
                    graph[idx    ] = r;
                    graph[idx + 1] = g;
                    graph[idx + 2] = b;
                    graph[idx + 3] = 255;
                }
            }
        }
    }

    let mid_point = (UniDollar(0) - min_val).0 as f64 / (max_val - min_val).0 as f64;
    let mid_point = (399.0 * mid_point) as usize;

    for (xx, data) in data.windows(2).enumerate() {
        let a = (399.0 * data[0].0) as i32;
        let b = (399.0 * data[1].0) as i32;

        for (x, y) in draw_line(xx as i32 * 50, a, xx as i32 * 50 + 50, b) {
            if x < 650 && y < 400 {
                for yy in mid_point .. y {
                    let idx = (x + (400 - 1 - yy as usize) * 650) * 4;
                    graph[idx    ] = 0;
                    graph[idx + 1] = 180;
                    graph[idx + 2] = 0;
                    graph[idx + 3] = 255;
                }
                let idx = (x + (400 - 1 - y as usize) * 650) * 4;
                graph[idx    ] = 0;
                graph[idx + 1] = 255;
                graph[idx + 2] = 0;
                graph[idx + 3] = 255;
            }
        }

        let a = (399.0 * data[0].1) as i32;
        let b = (399.0 * data[1].1) as i32;

        for (x, y) in draw_line(xx as i32 * 50, a, xx as i32 * 50 + 50, b) {
            if x < 650 && y < 400 {
                for yy in y .. mid_point {
                    let idx = (x + (400 - 1 - yy as usize) * 650) * 4;
                    graph[idx    ] = 180;
                    graph[idx + 1] = 0;
                    graph[idx + 2] = 0;
                    graph[idx + 3] = 255;
                }
                let idx = (x + (400 - 1 - y as usize) * 650) * 4;
                graph[idx    ] = 255;
                graph[idx + 1] = 0;
                graph[idx + 2] = 0;
                graph[idx + 3] = 255;
            }
        }
    }

    state.renderer.update_image(ResourceKey::new("dynamic", "650@400@graph"), 650, 400, graph);
}

fn draw_students(instance: &mut crate::GameInstance, state: &mut crate::GameState, ui: &ui::Node) {
    use std::cmp::max;
    let mut data = vec![];
    let min_val = 0;
    let mut max_val = u32::min_value();
    for hist in &instance.player.history {
        max_val = max(max_val, hist.students);
    }
    if min_val == max_val {
        max_val += 10;
    }
    max_val = min_val + ((f64::from(max_val - min_val) / 8.0).ceil() * 8.0) as u32;
    for hist in &instance.player.history {
        let p = f64::from(hist.students - min_val) / f64::from(max_val - min_val);
        data.push(p);
    }

    if let Some(area) = query!(ui, graph_area).next() {
        let spacing = (max_val - min_val) / 8;
        for i in 0 .. 9u32 {
            let n = node!{
                graph_label(pos = i as i32) {
                    @text(format!("- {}", max_val - spacing * i))
                }
            };
            area.add_child(n);
        }
    }

    let mut graph = vec![0; 650 * 400 * 4];

    // Draw a grid
    for y in 0 .. 8 {
        for x in 0 .. 13 {
            for yy in 0 .. 50 {
                for xx in 0 .. 50 {
                    let idx = (x * 50 + xx + (y * 50 + yy) * 650) * 4;
                    let (r, g, b) = if xx == 0 || xx == 49 || yy == 0 || yy == 49 {
                        (167, 202, 214)
                    } else {
                        (255, 255, 255)
                    };
                    graph[idx    ] = r;
                    graph[idx + 1] = g;
                    graph[idx + 2] = b;
                    graph[idx + 3] = 255;
                }
            }
        }
    }

    for (xx, data) in data.windows(2).enumerate() {
        let a = (399.0 * data[0]) as i32;
        let b = (399.0 * data[1]) as i32;

        for (x, y) in draw_line(xx as i32 * 50, a, xx as i32 * 50 + 50, b) {
            if x < 650 && y < 400 {
                for yy in 0 .. y {
                    let idx = (x + (400 - 1 - yy as usize) * 650) * 4;
                    graph[idx    ] = 180;
                    graph[idx + 1] = 180;
                    graph[idx + 2] = 0;
                    graph[idx + 3] = 255;
                }
                let idx = (x + (400 - 1 - y as usize) * 650) * 4;
                graph[idx    ] = 255;
                graph[idx + 1] = 255;
                graph[idx + 2] = 0;
                graph[idx + 3] = 255;
            }
        }
    }

    state.renderer.update_image(ResourceKey::new("dynamic", "650@400@graph"), 650, 400, graph);
}

fn draw_grades(instance: &mut crate::GameInstance, state: &mut crate::GameState, ui: &ui::Node) {
    use std::cmp::max;
    let min_val = 0;
    let mut max_val = u32::min_value();
    for hist in &instance.player.history {
        let total: u32 = hist.grades.iter()
            .cloned()
            .sum();
        max_val = max(max_val, total);
    }
    if min_val == max_val {
        max_val += 10;
    }
    max_val = min_val + ((f64::from(max_val - min_val) / 8.0).ceil() * 8.0) as u32;

    if let Some(area) = query!(ui, graph_area).next() {
        let spacing = (max_val - min_val) / 8;
        for i in 0 .. 9u32 {
            let n = node!{
                graph_label(pos = i as i32) {
                    @text(format!("- {}", max_val - spacing * i))
                }
            };
            area.add_child(n);
        }
    }

    let mut graph = vec![0; 650 * 400 * 4];

    // Draw a grid
    for y in 0 .. 8 {
        for x in 0 .. 13 {
            for yy in 0 .. 50 {
                for xx in 0 .. 50 {
                    let idx = (x * 50 + xx + (y * 50 + yy) * 650) * 4;
                    let (r, g, b) = if xx == 0 || xx == 49 || yy == 0 || yy == 49 {
                        (167, 202, 214)
                    } else {
                        (255, 255, 255)
                    };
                    graph[idx    ] = r;
                    graph[idx + 1] = g;
                    graph[idx + 2] = b;
                }
            }
        }
    }

    const GRADES: &[(Grade, u8, u8, u8)] = &[
        (Grade::F, 255, 0, 0),
        (Grade::E, 204, 51, 0),
        (Grade::D, 153, 102, 0),
        (Grade::C, 102, 153, 0),
        (Grade::B, 51, 204, 0),
        (Grade::A, 0, 255, 0),
    ];
    for (xx, data) in instance.player.history.windows(2).enumerate() {
        let mut offset_a = 0;
        let mut offset_b = 0;

        for &(grade, rr, gg, bb) in GRADES {
            let a = f64::from(offset_a + data[0].grades[grade.as_index()] - min_val) / f64::from(max_val - min_val);
            let b = f64::from(offset_b + data[1].grades[grade.as_index()] - min_val) / f64::from(max_val - min_val);
            offset_a += data[0].grades[grade.as_index()];
            offset_b += data[1].grades[grade.as_index()];
            let a = (399.0 * a) as i32;
            let b = (399.0 * b) as i32;

            for (x, y) in draw_line(xx as i32 * 50, a, xx as i32 * 50 + 50, b) {
                if x < 650 && y < 400 {
                    for yy in 0 ..= y {
                        let idx = (x + (400 - 1 - yy as usize) * 650) * 4;
                        if graph[idx + 3] == 0 {
                            graph[idx    ] = rr;
                            graph[idx + 1] = gg;
                            graph[idx + 2] = bb;
                            graph[idx + 3] = 255;
                        }
                    }
                }
            }
        }
    }
    // Alpha is used to prevent drawing on top of each other for the grades
    for data in graph.chunks_exact_mut(4) {
        data[3] = 255;
    }

    state.renderer.update_image(ResourceKey::new("dynamic", "650@400@graph"), 650, 400, graph);
}

#[derive(Clone, Copy, Debug)]
enum Tab {
    Money,
    Income,
    Students,
    Grades,
}

impl Tab {
    fn name(self) -> &'static str {
        match self {
            Tab::Money => "money",
            Tab::Income => "income",
            Tab::Students => "students",
            Tab::Grades => "grades",
        }
    }
}

impl state::State for StatsState {
    fn copy(&self) -> Box<dyn state::State> {
        Box::new(StatsState {
            ui: self.ui.clone(),
            tab: self.tab,
            next_update: self.next_update,
        })
    }

    fn active(&mut self, instance: &mut Option<GameInstance>, state: &mut crate::GameState) -> state::Action {
        let ui = state.ui_manager.create_node(assets::ResourceKey::new("base", "manage/stats"));
        self.ui = Some(ui.clone());

        let instance = assume!(state.global_logger, instance.as_mut());

        if let Some(btn) = query!(ui, button(tab="money")).next() {
            btn.set_property("on_click", ui::MethodDesc::<ui::MouseUpEvent>::native(|evt, _, _| {
                evt.emit(Tab::Money);
                true
            }));
        }
        if let Some(btn) = query!(ui, button(tab="income")).next() {
            btn.set_property("on_click", ui::MethodDesc::<ui::MouseUpEvent>::native(|evt, _, _| {
                evt.emit(Tab::Income);
                true
            }));
        }
        if let Some(btn) = query!(ui, button(tab="students")).next() {
            btn.set_property("on_click", ui::MethodDesc::<ui::MouseUpEvent>::native(|evt, _, _| {
                evt.emit(Tab::Students);
                true
            }));
        }
        if let Some(btn) = query!(ui, button(tab="grades")).next() {
            btn.set_property("on_click", ui::MethodDesc::<ui::MouseUpEvent>::native(|evt, _, _| {
                evt.emit(Tab::Grades);
                true
            }));
        }

        draw_money(instance, state, &ui);
        self.next_update = 60.0 * 10.0;

        state.ui_manager.events().emit(CloseWindowOthers(ui));
        state::Action::Nothing
    }

    fn tick(&mut self, instance: &mut Option<GameInstance>, state: &mut crate::GameState) -> state::Action {
        self.next_update -= state.delta;

        let instance = assume!(state.global_logger, instance.as_mut());
        let ui = assume!(state.global_logger, self.ui.clone());
        if self.next_update <= 0.0 {
            self.next_update = 60.0 * 10.0;
            for node in query!(ui, graph_label).matches() {
                node.parent().map(|v| v.remove_child(node));
            }
            match self.tab {
                Tab::Money => draw_money(instance, state, &ui),
                Tab::Income => draw_income_outcome(instance, state, &ui),
                Tab::Students => draw_students(instance, state, &ui),
                Tab::Grades => draw_grades(instance, state, &ui),
            }
        }

        state::Action::Nothing
    }

    fn inactive(&mut self, _instance: &mut Option<GameInstance>, state: &mut crate::GameState) {
        if let Some(ui) = self.ui.take() {
            state.ui_manager.remove_node(ui);
        }
    }

    fn ui_event(&mut self, _instance: &mut Option<GameInstance>, state: &mut crate::GameState, evt: &mut event::EventHandler) -> state::Action {
        let mut action = state::Action::Nothing;
        let ui = assume!(state.global_logger, self.ui.clone());
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
        evt.handle_event::<Tab, _>(|tab| {
            self.tab = tab;
            // Force a redraw
            self.next_update = -1.0;
            if let Some(btn) = query!(ui, button(selected=true)).next() {
                btn.set_property("selected", false);
            }
            if let Some(btn) = query!(ui, button(tab=tab.name())).next() {
                btn.set_property("selected", true);
            }
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

fn draw_line(x1: i32, y1: i32, x2: i32, y2: i32) -> impl Iterator<Item=(usize, usize)> {
    let dx = 50;
    let dy = (y2 - y1).abs();
    let sx = 1;
    let sy = if y1 < y2 { 1 } else { -1 };
    let error = (if dx > dy { dx } else { dy }) / 2;

    ::std::iter::repeat(())
        .scan((x1, y1, error), move |state, _| {
            if state.0 >= x2 && state.1 * sy >= y2 * sy {
                None
            } else {
                let res = (state.0 as usize, state.1 as usize);

                let err = state.2;
                if err > -dx {
                    state.2 -= dy;
                    state.0 += sx;
                }
                if err < dy {
                    state.2 += dx;
                    state.1 += sy;
                }

                Some(res)
            }
        })
}