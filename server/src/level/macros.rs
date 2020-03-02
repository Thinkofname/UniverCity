/// Helper to place objects inside a room like a player would
#[macro_export]
macro_rules! place_objects {
    (
    init($level:expr, $engine:expr, $entities:expr)
    room($room:ident at $rloc:expr) {
        $(
            $tt:tt
        )*
    }
    ) => {{
        place_object_impl!{
            init($level, $engine, $entities)
            room($room at $rloc)
            $($tt)*
        }
    }};
}

#[macro_export]
#[doc(hidden)]
macro_rules! place_object_impl {
    (
        init($level:expr, $engine:expr, $entities:expr)
        room($room:ident at $rloc:expr)
        place $obj:ident at ($x:expr, $y:expr) rotated($rot:expr)
        $(
            $tt:tt
        )*
    ) => {
        {
            $level.begin_object_placement::<_, EC>($room, $engine, $entities, $obj.borrow(), None)?;
            $level.move_active_object::<_, EC>(
                $room, $engine, $entities,
                (($rloc).x as f32 + $x, ($rloc).y as f32 + $y),
                None,
                $rot
            )?;
            $level.finalize_object_placement::<_, EC>($room, $engine, $entities, None, $rot)?;
            let obj_info = $level.asset_manager.loader_open::<object::Loader>($obj.borrow())?;
            $level.get_room_info_mut($room).placement_cost += obj_info.cost;
        }
        place_object_impl!{
            init($level, $engine, $entities)
            room($room at $rloc)
            $($tt)*
        }
    };
    (
        init($level:expr, $engine:expr, $entities:expr)
        room($room:ident at $rloc:expr)
        place $obj:ident at ($x:expr, $y:expr)
        $(
            $tt:tt
        )*
    ) => {
        {
            $level.begin_object_placement::<_, EC>($room, $engine, $entities, $obj.borrow(), None)?;
            $level.move_active_object::<_, EC>(
                $room, $engine, $entities,
                (($rloc).x as f32 + $x, ($rloc).y as f32 + $y),
                None,
                0
            )?;
            $level.finalize_object_placement::<_, EC>($room, $engine, $entities, None, 0)?;
            let obj_info = $level.asset_manager.loader_open::<object::Loader>($obj.borrow())?;
            $level.get_room_info_mut($room).placement_cost += obj_info.cost;
        }
        place_object_impl!{
            init($level, $engine, $entities)
            room($room at $rloc)
            $($tt)*
        }
    };

    (
        init($level:expr, $engine:expr, $entities:expr)
        room($room:ident at $rloc:expr)
    ) => {

    };
}
