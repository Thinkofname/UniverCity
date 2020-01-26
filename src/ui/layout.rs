
use fungui::*;
use crate::ui::*;

#[derive(Default)]
pub(crate) struct Center {
    rect: Rect,
}

pub(crate) struct CenterChild {
    x: Option<i32>,
    y: Option<i32>,
    width: Option<i32>,
    height: Option<i32>,

    align_width: bool,
    align_height: bool,
}

impl Default for CenterChild {
    fn default() -> Self {
        CenterChild {
            x: None,
            y: None,
            width: None,
            height: None,

            align_width: true,
            align_height: true,
        }
    }
}

pub static ALIGN_WIDTH: StaticKey = StaticKey("align_width");
pub static ALIGN_HEIGHT: StaticKey = StaticKey("align_height");

impl LayoutEngine<UniverCityUI> for Center {
    type ChildData = CenterChild;

    fn name() -> &'static str { "center" }
    fn style_properties<'a, F>(mut prop: F)
        where F: FnMut(StaticKey) + 'a
    {
        prop(ALIGN_WIDTH);
        prop(ALIGN_HEIGHT);
    }

    fn new_child_data() -> CenterChild {
        CenterChild::default()
    }

    fn update_data(&mut self, _styles: &Styles<UniverCityUI>, _nc: &NodeChain<'_, UniverCityUI>, _rule: &Rule<UniverCityUI>) -> DirtyFlags {
        DirtyFlags::empty()
    }
    fn update_child_data(&mut self, styles: &Styles<UniverCityUI>, nc: &NodeChain<'_, UniverCityUI>, rule: &Rule<UniverCityUI>, data: &mut Self::ChildData) -> DirtyFlags {
        let mut flags = DirtyFlags::empty();
        eval!(styles, nc, rule.X => val => {
            let new = val.convert();
            if data.x != new {
                data.x = new;
                flags |= DirtyFlags::POSITION;
            }
        });
        eval!(styles, nc, rule.Y => val => {
            let new = val.convert();
            if data.y != new {
                data.y = new;
                flags |= DirtyFlags::POSITION;
            }
        });
        eval!(styles, nc, rule.WIDTH => val => {
            let new = val.convert();
            if data.width != new {
                data.width = new;
                flags |= DirtyFlags::SIZE;
            }
        });
        eval!(styles, nc, rule.HEIGHT => val => {
            let new = val.convert();
            if data.height != new {
                data.height = new;
                flags |= DirtyFlags::SIZE;
            }
        });
        eval!(styles, nc, rule.ALIGN_WIDTH => val => {
            let new = val.convert().unwrap_or(true);
            if data.align_width != new {
                data.align_width = new;
                flags |= DirtyFlags::POSITION;
            }
        });
        eval!(styles, nc, rule.ALIGN_HEIGHT => val => {
            let new = val.convert().unwrap_or(true);
            if data.align_height != new {
                data.align_height = new;
                flags |= DirtyFlags::POSITION;
            }
        });
        flags
    }

    fn reset_unset_data(&mut self, _used_keys: &FnvHashSet<StaticKey>) -> DirtyFlags {
        DirtyFlags::empty()
    }
    fn reset_unset_child_data(&mut self, used_keys: &FnvHashSet<StaticKey>, data: &mut Self::ChildData) -> DirtyFlags {
        let mut flags = DirtyFlags::empty();
        if !used_keys.contains(&X) && data.x.is_some() {
            data.x = None;
            flags |= DirtyFlags::POSITION;
        }
        if !used_keys.contains(&Y) && data.y.is_some() {
            data.y = None;
            flags |= DirtyFlags::POSITION;
        }
        if !used_keys.contains(&WIDTH) && data.width.is_some() {
            data.width = None;
            flags |= DirtyFlags::SIZE;
        }
        if !used_keys.contains(&HEIGHT) && data.height.is_some() {
            data.height = None;
            flags |= DirtyFlags::SIZE;
        }
        if !used_keys.contains(&ALIGN_WIDTH) && !data.align_width {
            data.align_width = true;
            flags |= DirtyFlags::POSITION;
        }
        if !used_keys.contains(&ALIGN_HEIGHT) && !data.align_height {
            data.align_height = true;
            flags |= DirtyFlags::POSITION;
        }

        flags
    }

    fn start_layout(&mut self, _ext: &mut NodeData, current: Rect, _flags: DirtyFlags, _children: ChildAccess<'_, Self, UniverCityUI>) -> Rect {
        self.rect = current;
        current
    }

    fn do_layout(&mut self, _value: &NodeValue<UniverCityUI>, _ext: &mut NodeData, data: &mut Self::ChildData, mut current: Rect, _flags: DirtyFlags) -> Rect {
        if let Some(v) = data.width {
            current.width = v;
        } else {
            current.width = self.rect.width;
        }
        if let Some(v) = data.height {
            current.height = v;
        } else {
            current.height = self.rect.height;
        }
        current
    }

    fn do_layout_end(&mut self, _value: &NodeValue<UniverCityUI>, _ext: &mut NodeData, data: &mut Self::ChildData, mut current: Rect, _flags: DirtyFlags) -> Rect {
        if data.align_width {
            current.x = (self.rect.width / 2) - (current.width / 2);
        } else {
            data.x.map(|v| current.x = v);
        }

        if data.align_height {
            current.y = (self.rect.height / 2) - (current.height / 2);
        } else {
            data.y.map(|v| current.y = v);
        }
        current
    }
}


#[derive(Default)]
pub(crate) struct Padded {
    padding: i32,
}
#[derive(Default)]
pub(crate) struct PaddedChild {
    x: Option<i32>,
    y: Option<i32>,
    width: Option<i32>,
    height: Option<i32>,
}

impl <E> LayoutEngine<E> for Padded
    where E: Extension
{
    type ChildData = PaddedChild;

    fn name() -> &'static str { "padded" }
    fn style_properties<'a, F>(mut prop: F)
        where F: FnMut(StaticKey) + 'a
    {
        prop(PADDING);
    }

    fn new_child_data() -> PaddedChild {
        PaddedChild::default()
    }

    fn update_data(&mut self, styles: &Styles<E>, nc: &NodeChain<'_, E>, rule: &Rule<E>) -> DirtyFlags {
        let mut flags = DirtyFlags::empty();
        eval!(styles, nc, rule.PADDING => val => {
            let new = val.convert().unwrap_or(0);
            if self.padding != new {
                self.padding = new;
                flags |= DirtyFlags::SIZE;
            }
        });
        flags
    }
    fn update_child_data(&mut self, styles: &Styles<E>, nc: &NodeChain<'_, E>, rule: &Rule<E>, data: &mut Self::ChildData) -> DirtyFlags {
        let mut flags = DirtyFlags::empty();
        eval!(styles, nc, rule.X => val => {
            let new = val.convert();
            if data.x != new {
                data.x = new;
                flags |= DirtyFlags::POSITION;
            }
        });
        eval!(styles, nc, rule.Y => val => {
            let new = val.convert();
            if data.y != new {
                data.y = new;
                flags |= DirtyFlags::POSITION;
            }
        });
        eval!(styles, nc, rule.WIDTH => val => {
            let new = val.convert();
            if data.width != new {
                data.width = new;
                flags |= DirtyFlags::SIZE;
            }
        });
        eval!(styles, nc, rule.HEIGHT => val => {
            let new = val.convert();
            if data.height != new {
                data.height = new;
                flags |= DirtyFlags::SIZE;
            }
        });
        flags
    }

    fn reset_unset_data(&mut self, used_keys: &FnvHashSet<StaticKey>) -> DirtyFlags {
        let mut flags = DirtyFlags::empty();
        if !used_keys.contains(&PADDING) && self.padding != 0 {
            self.padding = 0;
            flags |= DirtyFlags::SIZE;
        }
        flags
    }
    fn reset_unset_child_data(&mut self, used_keys: &FnvHashSet<StaticKey>, data: &mut Self::ChildData) -> DirtyFlags {
        let mut flags = DirtyFlags::empty();
        if !used_keys.contains(&X) && data.x.is_some() {
            data.x = None;
            flags |= DirtyFlags::POSITION;
        }
        if !used_keys.contains(&Y) && data.y.is_some() {
            data.y = None;
            flags |= DirtyFlags::POSITION;
        }
        if !used_keys.contains(&WIDTH) && data.width.is_some() {
            data.width = None;
            flags |= DirtyFlags::SIZE;
        }
        if !used_keys.contains(&HEIGHT) && data.height.is_some() {
            data.height = None;
            flags |= DirtyFlags::SIZE;
        }

        flags
    }
    fn finish_layout(&mut self, _ext: &mut E::NodeData, mut current: Rect, _flags: DirtyFlags, children: ChildAccess<'_, Self, E>) -> Rect {
        use std::cmp;
        let mut max = (0, 0);
        // TODO: Cache?
        for i in 0 .. children.len() {
            let (c, _, _) = children.get(i).expect("Missing child");
            max.0 = cmp::max(max.0, c.x + c.width);
            max.1 = cmp::max(max.1, c.y + c.height);
        }
        current.width = max.0 + self.padding;
        current.height = max.1 + self.padding;
        current
    }

    fn do_layout(&mut self, _value: &NodeValue<E>, _ext: &mut E::NodeData, data: &mut Self::ChildData, mut current: Rect, _flags: DirtyFlags) -> Rect {
        current.x = self.padding + data.x.unwrap_or(0);
        current.y = self.padding + data.y.unwrap_or(0);
        data.width.map(|v| current.width = v);
        data.height.map(|v| current.height = v);
        current
    }
}

#[derive(Default)]
pub(crate) struct Tooltip {
    padding: i32,
    parent: Rect,
}
#[derive(Default)]
pub(crate) struct TooltipChild {
    x: Option<i32>,
    y: Option<i32>,
    width: Option<i32>,
    height: Option<i32>,
}

static PADDING: StaticKey = StaticKey("padding");


impl <E> LayoutEngine<E> for Tooltip
    where E: Extension
{
    type ChildData = TooltipChild;

    fn name() -> &'static str { "tooltip" }
    fn style_properties<'a, F>(mut prop: F)
        where F: FnMut(StaticKey) + 'a
    {
        prop(PADDING);
    }

    fn new_child_data() -> TooltipChild {
        TooltipChild::default()
    }

    fn update_data(&mut self, styles: &Styles<E>, nc: &NodeChain<'_, E>, rule: &Rule<E>) -> DirtyFlags {
        let mut flags = DirtyFlags::empty();
        eval!(styles, nc, rule.PADDING => val => {
            let new = val.convert().unwrap_or(0);
            if self.padding != new {
                self.padding = new;
                flags |= DirtyFlags::SIZE;
            }
        });
        flags
    }
    fn update_child_data(&mut self, styles: &Styles<E>, nc: &NodeChain<'_, E>, rule: &Rule<E>, data: &mut Self::ChildData) -> DirtyFlags {
        let mut flags = DirtyFlags::empty();
        eval!(styles, nc, rule.X => val => {
            let new = val.convert();
            if data.x != new {
                data.x = new;
                flags |= DirtyFlags::POSITION;
            }
        });
        eval!(styles, nc, rule.Y => val => {
            let new = val.convert();
            if data.y != new {
                data.y = new;
                flags |= DirtyFlags::POSITION;
            }
        });
        eval!(styles, nc, rule.WIDTH => val => {
            let new = val.convert();
            if data.width != new {
                data.width = new;
                flags |= DirtyFlags::SIZE;
            }
        });
        eval!(styles, nc, rule.HEIGHT => val => {
            let new = val.convert();
            if data.height != new {
                data.height = new;
                flags |= DirtyFlags::SIZE;
            }
        });
        flags
    }

    fn reset_unset_data(&mut self, used_keys: &FnvHashSet<StaticKey>) -> DirtyFlags {
        let mut flags = DirtyFlags::empty();
        if !used_keys.contains(&PADDING) && self.padding != 0 {
            self.padding = 0;
            flags |= DirtyFlags::SIZE;
        }
        flags
    }
    fn reset_unset_child_data(&mut self, used_keys: &FnvHashSet<StaticKey>, data: &mut Self::ChildData) -> DirtyFlags {
        let mut flags = DirtyFlags::empty();
        if !used_keys.contains(&X) && data.x.is_some() {
            data.x = None;
            flags |= DirtyFlags::POSITION;
        }
        if !used_keys.contains(&Y) && data.y.is_some() {
            data.y = None;
            flags |= DirtyFlags::POSITION;
        }
        if !used_keys.contains(&WIDTH) && data.width.is_some() {
            data.width = None;
            flags |= DirtyFlags::SIZE;
        }
        if !used_keys.contains(&HEIGHT) && data.height.is_some() {
            data.height = None;
            flags |= DirtyFlags::SIZE;
        }

        flags
    }
    fn start_layout(&mut self, _ext: &mut E::NodeData, current: Rect, _flags: DirtyFlags, _children: ChildAccess<'_, Self, E>) -> Rect {
        self.parent = current;
        current
    }

    fn do_layout(&mut self, _value: &NodeValue<E>, _ext: &mut E::NodeData, data: &mut Self::ChildData, mut current: Rect, _flags: DirtyFlags) -> Rect {
        data.x.map(|v| current.x = v);
        data.y.map(|v| current.y = v);
        data.width.map(|v| current.width = v);
        data.height.map(|v| current.height = v);
        current
    }
    fn do_layout_end(&mut self, _value: &NodeValue<E>, _ext: &mut E::NodeData, _data: &mut Self::ChildData, mut current: Rect, _flags: DirtyFlags) -> Rect {
        use std::cmp::max;
        current.x = max(current.x, self.padding);
        current.y = max(current.y, self.padding);

        if current.x + current.width > self.parent.width - self.padding {
            current.x = self.parent.width - self.padding - current.width;
        }
        if current.y + current.height > self.parent.height - self.padding {
            current.y = self.parent.height - self.padding - current.height;
        }
        current
    }
}


#[derive(Default)]
pub struct Rows {
    offset: i32,
    width: i32,
}

pub struct RowsChild {
    height: Option<i32>,
}

impl LayoutEngine<UniverCityUI> for Rows {
    type ChildData = RowsChild;
    fn name() -> &'static str {
        "rows"
    }

    fn style_properties<'a, F>(_prop: F)
        where F: FnMut(StaticKey) + 'a
    {
    }

    fn new_child_data() -> Self::ChildData {
        RowsChild {
            height: None,
        }
    }

    fn update_child_data(&mut self, styles: &Styles<UniverCityUI>, nc: &NodeChain<'_, UniverCityUI>, rule: &Rule<UniverCityUI>, data: &mut RowsChild) -> DirtyFlags {
        let mut flags = DirtyFlags::empty();
        eval!(styles, nc, rule.HEIGHT => val => {
            let new = val.convert().unwrap_or(0);
            if data.height != Some(new) {
                data.height = Some(new);
                flags |= DirtyFlags::SIZE;
            }
        });

        flags
    }
    fn reset_unset_child_data(&mut self, used_keys: &FnvHashSet<StaticKey>, data: &mut RowsChild) -> DirtyFlags {
        let mut flags = DirtyFlags::empty();
        if !used_keys.contains(&HEIGHT) && data.height.is_some() {
            data.height = None;
            flags |= DirtyFlags::SIZE;
        }
        flags
    }

    fn check_parent_flags(&mut self, flags: DirtyFlags) -> DirtyFlags {
        if flags.contains(DirtyFlags::SIZE) {
            DirtyFlags::SIZE
        } else {
            DirtyFlags::empty()
        }
    }

    fn start_layout(&mut self, _ext: &mut NodeData, current: Rect, _flags: DirtyFlags, _children: ChildAccess<'_, Self, UniverCityUI>) -> Rect {
        self.offset = 0;
        self.width = current.width;
        current
    }
    fn finish_layout(&mut self, _ext: &mut NodeData, mut current: Rect, _flags: DirtyFlags, _children: ChildAccess<'_, Self, UniverCityUI>) -> Rect {
        current.height = self.offset;
        current
    }
    fn do_layout(&mut self, _value: &NodeValue<UniverCityUI>, _ext: &mut NodeData, data: &mut Self::ChildData, _current: Rect, _flags: DirtyFlags) -> Rect {
        Rect {
            x: 0,
            y: self.offset,
            width: self.width,
            height: data.height.unwrap_or(16),
        }
    }
    fn do_layout_end(&mut self, _value: &NodeValue<UniverCityUI>, _ext: &mut NodeData, _data: &mut Self::ChildData, current: Rect, _flags: DirtyFlags) -> Rect {
        self.offset += current.height;
        current
    }
}


#[derive(Default)]
pub struct RowsInv {
    offset: i32,
    width: i32,
}

pub struct RowsInvChild {
    height: i32,
}

impl LayoutEngine<UniverCityUI> for RowsInv {
    type ChildData = RowsInvChild;
    fn name() -> &'static str {
        "rows_inv"
    }

    fn style_properties<'a, F>(_prop: F)
        where F: FnMut(StaticKey) + 'a
    {
    }

    fn new_child_data() -> Self::ChildData {
        RowsInvChild {
            height: 16,
        }
    }

    fn update_child_data(&mut self, styles: &Styles<UniverCityUI>, nc: &NodeChain<'_, UniverCityUI>, rule: &Rule<UniverCityUI>, data: &mut RowsInvChild) -> DirtyFlags {
        let mut flags = DirtyFlags::empty();
        eval!(styles, nc, rule.HEIGHT => val => {
            let new = val.convert().unwrap_or(16);
            if data.height != new {
                data.height = new;
                flags |= DirtyFlags::SIZE;
            }
        });

        flags
    }
    fn reset_unset_child_data(&mut self, used_keys: &FnvHashSet<StaticKey>, data: &mut RowsInvChild) -> DirtyFlags {
        let mut flags = DirtyFlags::empty();
        if !used_keys.contains(&HEIGHT) && data.height != 16 {
            data.height = 16;
            flags |= DirtyFlags::SIZE;
        }
        flags
    }

    fn check_parent_flags(&mut self, flags: DirtyFlags) -> DirtyFlags {
        if flags.contains(DirtyFlags::SIZE) {
            DirtyFlags::SIZE
        } else {
            DirtyFlags::empty()
        }
    }

    fn start_layout(&mut self, _ext: &mut NodeData, current: Rect, _flags: DirtyFlags, _children: ChildAccess<'_, Self, UniverCityUI>) -> Rect {
        self.offset = current.height;
        self.width = current.width;
        current
    }
    fn do_layout(&mut self, _value: &NodeValue<UniverCityUI>, _ext: &mut NodeData, data: &mut Self::ChildData, _current: Rect, _flags: DirtyFlags) -> Rect {
        self.offset -= data.height;
        Rect {
            x: 0,
            y: self.offset,
            width: self.width,
            height: data.height,
        }
    }
}

#[derive(Default)]
pub(crate) struct Clipped {
    parent: Rect,
}
#[derive(Default)]
pub(crate) struct ClippedChild {
    x: Option<i32>,
    y: Option<i32>,
    width: Option<i32>,
    height: Option<i32>,
}

fn apply_clip(
    mut obj: Rect,
    parent: Rect
) -> Rect {
    if obj.x < 0 {
        obj.width += obj.x;
        obj.x = 0;
    }
    if obj.x + obj.width > parent.width {
        obj.width = parent.width - obj.x;
    }

    if obj.y < 0 {
        obj.height += obj.y;
        obj.y = 0;
    }
    if obj.y + obj.height > parent.height {
        obj.height = parent.height - obj.y;
    }

    obj
}

impl <E> LayoutEngine<E> for Clipped
    where E: Extension
{
    type ChildData = ClippedChild;

    fn name() -> &'static str { "clipped" }
    fn style_properties<'a, F>(_prop: F)
        where F: FnMut(StaticKey) + 'a
    {
    }

    fn new_child_data() -> ClippedChild {
        ClippedChild::default()
    }

    fn update_child_data(&mut self, styles: &Styles<E>, nc: &NodeChain<'_, E>, rule: &Rule<E>, data: &mut Self::ChildData) -> DirtyFlags {
        let mut flags = DirtyFlags::empty();
        eval!(styles, nc, rule.X => val => {
            let new = val.convert();
            if data.x != new {
                data.x = new;
                flags |= DirtyFlags::POSITION;
            }
        });
        eval!(styles, nc, rule.Y => val => {
            let new = val.convert();
            if data.y != new {
                data.y = new;
                flags |= DirtyFlags::POSITION;
            }
        });
        eval!(styles, nc, rule.WIDTH => val => {
            let new = val.convert();
            if data.width != new {
                data.width = new;
                flags |= DirtyFlags::SIZE;
            }
        });
        eval!(styles, nc, rule.HEIGHT => val => {
            let new = val.convert();
            if data.height != new {
                data.height = new;
                flags |= DirtyFlags::SIZE;
            }
        });
        flags
    }

    fn reset_unset_data(&mut self, _used_keys: &FnvHashSet<StaticKey>) -> DirtyFlags {
        DirtyFlags::empty()
    }
    fn reset_unset_child_data(&mut self, used_keys: &FnvHashSet<StaticKey>, data: &mut Self::ChildData) -> DirtyFlags {
        let mut flags = DirtyFlags::empty();
        if !used_keys.contains(&X) && data.x.is_some() {
            data.x = None;
            flags |= DirtyFlags::POSITION;
        }
        if !used_keys.contains(&Y) && data.y.is_some() {
            data.y = None;
            flags |= DirtyFlags::POSITION;
        }
        if !used_keys.contains(&WIDTH) && data.width.is_some() {
            data.width = None;
            flags |= DirtyFlags::SIZE;
        }
        if !used_keys.contains(&HEIGHT) && data.height.is_some() {
            data.height = None;
            flags |= DirtyFlags::SIZE;
        }

        flags
    }
    fn start_layout(&mut self, _ext: &mut E::NodeData, current: Rect, _flags: DirtyFlags, _children: ChildAccess<'_, Self, E>) -> Rect {
        self.parent = current;
        current
    }

    fn do_layout(&mut self, _value: &NodeValue<E>, _ext: &mut E::NodeData, data: &mut Self::ChildData, mut current: Rect, _flags: DirtyFlags) -> Rect {
        data.x.map(|v| current.x = v);
        data.y.map(|v| current.y = v);
        data.width.map(|v| current.width = v);
        data.height.map(|v| current.height = v);
        apply_clip(current, self.parent)
    }
    fn do_layout_end(&mut self, _value: &NodeValue<E>, _ext: &mut E::NodeData, _data: &mut Self::ChildData, current: Rect, _flags: DirtyFlags) -> Rect {
        apply_clip(current, self.parent)
    }
}

#[derive(Default)]
pub struct Grid {
    columns: i32,
    rows: i32,
    cell_width: i32,
    cell_height: i32,
    spacing: i32,
    margin: i32,

    force_size: bool,

    recompute: bool,
}

pub struct GridChild {
    x: i32,
    y: i32,
}

static SPACING: StaticKey = StaticKey("spacing");
static MARGIN: StaticKey = StaticKey("margin");
static COLUMNS: StaticKey = StaticKey("columns");
static ROWS: StaticKey = StaticKey("rows");
static FORCE_SIZE: StaticKey = StaticKey("force_size");

impl LayoutEngine<UniverCityUI> for Grid {
    type ChildData = GridChild;
    fn name() -> &'static str {
        "grid"
    }

    fn style_properties<'a, F>(mut prop: F)
        where F: FnMut(StaticKey) + 'a
    {
        prop(SPACING);
        prop(MARGIN);
        prop(COLUMNS);
        prop(ROWS);
        prop(FORCE_SIZE);
    }

    fn new_child_data() -> Self::ChildData {
        GridChild {
            x: 0,
            y: 0,
        }
    }

    fn update_data(&mut self, styles: &Styles<UniverCityUI>, nc: &NodeChain<'_, UniverCityUI>, rule: &Rule<UniverCityUI>) -> DirtyFlags {
        let mut flags = DirtyFlags::empty();
        eval!(styles, nc, rule.SPACING => val => {
            let new = val.convert().unwrap_or(0);
            if self.spacing != new {
                self.spacing = new;
                self.recompute = true;
                flags |= DirtyFlags::SIZE;
            }
        });
        eval!(styles, nc, rule.MARGIN => val => {
            let new = val.convert().unwrap_or(0);
            if self.margin != new {
                self.margin = new;
                self.recompute = true;
                flags |= DirtyFlags::SIZE;
            }
        });
        eval!(styles, nc, rule.COLUMNS => val => {
            let new = val.convert().unwrap_or(1);
            if self.columns != new {
                self.columns = new;
                self.recompute = true;
                flags |= DirtyFlags::SIZE;
            }
        });
        eval!(styles, nc, rule.ROWS => val => {
            let new = val.convert().unwrap_or(1);
            if self.rows != new {
                self.rows = new;
                self.recompute = true;
                flags |= DirtyFlags::SIZE;
            }
        });
        eval!(styles, nc, rule.FORCE_SIZE => val => {
            let new = val.convert().unwrap_or(false);
            if self.force_size != new {
                self.force_size = new;
                self.recompute = true;
                flags |= DirtyFlags::SIZE;
            }
        });

        flags
    }
    fn reset_unset_data(&mut self, used_keys: &FnvHashSet<StaticKey>) -> DirtyFlags {
        let mut flags = DirtyFlags::empty();
        if !used_keys.contains(&SPACING) && self.spacing != 0 {
            self.spacing = 0;
            self.recompute = true;
            flags |= DirtyFlags::SIZE;
        }
        if !used_keys.contains(&MARGIN) && self.margin != 0 {
            self.margin = 0;
            self.recompute = true;
            flags |= DirtyFlags::SIZE;
        }
        if !used_keys.contains(&COLUMNS) && self.columns != 1 {
            self.columns = 1;
            self.recompute = true;
            flags |= DirtyFlags::SIZE;
        }
        if !used_keys.contains(&ROWS) && self.rows != 1 {
            self.rows = 1;
            self.recompute = true;
            flags |= DirtyFlags::SIZE;
        }
        if !used_keys.contains(&FORCE_SIZE) && self.force_size {
            self.force_size = false;
            self.recompute = true;
            flags |= DirtyFlags::SIZE;
        }
        flags
    }
    fn reset_unset_child_data(&mut self, _used_keys: &FnvHashSet<StaticKey>, _data: &mut Self::ChildData) -> DirtyFlags {
        if self.recompute {
            DirtyFlags::SIZE | DirtyFlags::POSITION
        } else {
            DirtyFlags::empty()
        }
    }

    fn check_parent_flags(&mut self, flags: DirtyFlags) -> DirtyFlags {
        if flags.contains(DirtyFlags::SIZE) {
            DirtyFlags::SIZE
        } else {
            DirtyFlags::empty()
        }
    }

    fn start_layout(&mut self, _ext: &mut NodeData, current: Rect, flags: DirtyFlags, children: ChildAccess<'_, Self, UniverCityUI>) -> Rect {
        if self.recompute
            || flags.contains(DirtyFlags::SIZE)
            || flags.contains(DirtyFlags::LAYOUT)
            || flags.contains(DirtyFlags::CHILDREN)
        {
            if self.columns == 0 || self.rows == 0 {
                return current;
            }
            let width = current.width - self.margin * 2;
            let height = current.height - self.margin * 2;
            self.cell_width = (width - (self.spacing * (self.columns - 1))) / self.columns;
            self.cell_height = (height - (self.spacing * (self.rows - 1))) / self.rows;

            let mut x = 0;
            let mut y = 0;
            for i in 0 .. children.len() {
                let mut c = children.get(i)
                    .expect("len != actual child count")
                    .2;
                let (_, c) = c.split();
                c.x = x;
                c.y = y;
                x += 1;
                if x >= self.columns {
                    x = 0;
                    y += 1;
                }
            }
        }
        current
    }
    fn finish_layout(&mut self, _ext: &mut NodeData, current: Rect, _flags: DirtyFlags, _children: ChildAccess<'_, Self, UniverCityUI>) -> Rect {
        self.recompute = false;
        current
    }
    fn do_layout(&mut self, _value: &NodeValue<UniverCityUI>, _ext: &mut NodeData, data: &mut Self::ChildData, _current: Rect, _flags: DirtyFlags) -> Rect {
        Rect {
            x: self.margin + data.x * (self.cell_width + self.spacing),
            y: self.margin + data.y * (self.cell_height + self.spacing),
            width: self.cell_width,
            height: self.cell_height,
        }
    }
}
