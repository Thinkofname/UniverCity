
use crate::util::{Location, FNVMap};
use std::mem;

use super::{model, pipeline};
use super::atlas::Rect;
use crate::server::level::{self, SECTION_SIZE, room};
use crate::render::gl;
use cgmath::{self, Matrix4};
use crate::util::Direction;
use crate::server::assets;
use crate::math::Frustum;
use crate::prelude::*;

mod window;

/// Handles terrain rendering for levels.
pub struct Terrain {
    log: Logger,
    asset_manager: assets::AssetManager,
    pub(super) width: u32,
    pub(super) height: u32,

    sections: Vec<TerrainSection>,
    edit_sections: FNVMap<room::Id, EditSection>,
    placement_guides: FNVMap<room::Id, PlacementGuide>,

    pub lowered_region: Option<Bound>,
    windows: Vec<window::Model>,
}

struct EditSection {
    model: model::Model<GLVertex>,
    touched: bool,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

struct PlacementGuide {
    model: model::Model<PlacementVertex>,
    bounds: (f32, f32, f32, f32),
    texture: gl::Texture,
}

#[repr(C)]
struct PlacementVertex {
    pos: cgmath::Vector3<f32>,
    lookup_x: f32,
    lookup_y: f32,
}

struct TerrainSection {
    x: usize,
    y: usize,
    array: gl::VertexArray,
    buffer: gl::Buffer,
    count: usize,
    max_count: usize,
}

#[derive(Clone, Copy, Debug)]
struct Vertex {
    x: i8,
    y: i8,
    z: i8,
}

const FACE_RIGHT: [Vertex; 6] = [
    Vertex {x: 0, y: 0, z: 0},
    Vertex {x: 0, y: 1, z: 0},
    Vertex {x: 1, y: 0, z: 0},
    Vertex {x: 0, y: 1, z: 0},
    Vertex {x: 1, y: 1, z: 0},
    Vertex {x: 1, y: 0, z: 0},
];

const FACE_LEFT: [Vertex; 6] = [
    Vertex {x: 0, y: 0, z: 0},
    Vertex {x: 0, y: 0, z: 1},
    Vertex {x: 0, y: 1, z: 1},
    Vertex {x: 0, y: 1, z: 0},
    Vertex {x: 0, y: 0, z: 0},
    Vertex {x: 0, y: 1, z: 1},
];

const FACE_FLOOR: [Vertex; 6] = [
    Vertex {x: 0, y: 0, z: 0},
    Vertex {x: 0, y: 0, z: 1},
    Vertex {x: 1, y: 0, z: 0},
    Vertex {x: 1, y: 0, z: 0},
    Vertex {x: 0, y: 0, z: 1},
    Vertex {x: 1, y: 0, z: 1},
];

#[repr(C)]
#[derive(Clone)]
pub struct GLVertex {
    x: f32,
    y: f32,
    z: f32,

    nx: f32,
    ny: f32,
    nz: f32,

    texture: f32,
    texture_offset_x: f32,
    texture_offset_y: f32,
    // Abused for selection color
    pub brightness: f32,
}

impl Terrain {
    pub(super) fn new(log: &Logger, asset_manager: &assets::AssetManager, level: &level::Level, ctx: &mut pipeline::Context<'_>) -> Terrain {
        let log = log.new(o!("source" => "terrain"));
        let prog = ctx.program("terrain");
        prog.use_program();
        let a_position = assume!(log, prog.attribute("attrib_position"));
        let a_texture = assume!(log, prog.attribute("attrib_texture"));
        let a_normal = assume!(log, prog.attribute("attrib_normal"));

        // Build placeholders for the terrain sections
        let sw = (level.width as usize + (SECTION_SIZE - 1)) / SECTION_SIZE;
        let sh = (level.height as usize + (SECTION_SIZE - 1)) / SECTION_SIZE;
        let mut sections = Vec::with_capacity(sw * sh);
        for x in 0 .. sw {
            for y in 0 .. sh {
                let array = gl::VertexArray::new();
                array.bind();
                let buffer = gl::Buffer::new();
                buffer.bind(gl::BufferTarget::Array);

                a_position.enable();
                a_position.vertex_pointer(3, gl::Type::Float, false, mem::size_of::<GLVertex>() as i32, 0);
                a_normal.enable();
                a_normal.vertex_pointer(3, gl::Type::Float, false, mem::size_of::<GLVertex>() as i32, 12);
                a_texture.enable();
                a_texture.vertex_pointer(4, gl::Type::Float, false, mem::size_of::<GLVertex>() as i32, 24);


                sections.push(TerrainSection {
                    x,
                    y,
                    array,
                    buffer,
                    count: 0,
                    max_count: 0,
                });
            }
        }

        Terrain {
            log,
            asset_manager: asset_manager.clone(),
            width: level.width,
            height: level.height,

            sections,
            edit_sections: FNVMap::default(),
            placement_guides: FNVMap::default(),
            lowered_region: None,
            windows: Vec::new(),
        }
    }

    pub(super) fn update(&mut self, ctx: &mut pipeline::Context<'_>, target_atlas: &mut super::GlobalAtlas, level: &mut level::Level) {
        for edit in self.edit_sections.values_mut() {
            edit.touched = false;
        }
        let mut data = vec![];
        for room_id in level.room_ids() {
            let dirty = {
                let mut room = level.get_room_info_mut(room_id);
                if let Some(virt) = room.building_level.as_mut() {
                    if virt.dirty {
                        virt.dirty = false;
                        true
                    } else {
                        false
                    }
                } else { false }
            };
            let room = level.get_room_info(room_id);
            if let Some(virt) = room.building_level.as_ref() {
                if dirty {
                    let mut verts = vec![];
                    let mut wall_bounds = virt.room_bounds;
                    wall_bounds.min -= (1, 1);

                    let lower_walls = virt.should_lower_walls;
                    let wall_height = if lower_walls {
                        4.0 / 16.0
                    } else {
                        1.0
                    };

                    for loc in wall_bounds {
                        data.clear();
                        if !virt.room_bounds.in_bounds(loc) {
                            Self::place_walls_limit(
                                &self.log,
                                &self.asset_manager, target_atlas,
                                &mut self.windows,
                                level, virt, loc, &mut data,
                                virt.room_bounds.in_bounds(loc.shift(Direction::South)), // South wall
                                virt.room_bounds.in_bounds(loc.shift(Direction::West)), // West wall
                                wall_height, Some(wall_bounds)
                            );
                        } else {
                            Self::make_tile(&self.log, &self.asset_manager, target_atlas, virt, loc, &mut data);
                            Self::place_walls_limit(
                                &self.log,
                                &self.asset_manager, target_atlas,
                                &mut self.windows,
                                level, virt, loc, &mut data, true, true,
                                wall_height, Some(virt.room_bounds)
                            );
                        }
                        for vert in &mut data {
                            vert.brightness = 1.0;
                        }
                        verts.extend_from_slice(&data);
                    }
                    let mut new = false;
                    let edit = self.edit_sections.entry(room_id).or_insert_with(|| {
                        new = true;
                        EditSection {
                            x: 0,
                            y: 0,
                            width: 0,
                            height: 0,
                            model: model::Model::new(
                                ctx,
                                "terrain_selection",
                                vec![
                                    model::Attribute{name: "attrib_position", count: 3, ty: gl::Type::Float, offset: 0, int: false},
                                    model::Attribute{name: "attrib_normal", count: 3, ty: gl::Type::Float, offset: 12, int: false},
                                    model::Attribute{name: "attrib_texture", count: 4, ty: gl::Type::Float, offset: 24, int: false},
                                ],
                                vec![]
                            ),
                            touched: true,
                        }
                    });
                    if new {
                        edit.model.uniforms.insert("u_textures", model::UniformValue::Int(super::GLOBAL_TEXTURE_LOCATION as i32));
                        edit.model.uniforms.insert("shadow_map", model::UniformValue::Int(super::SHADOW_MAP_LOCATION as i32));
                        edit.model.uniforms.insert("shadow_matrix", model::UniformValue::Matrix4(cgmath::SquareMatrix::identity()));
                        edit.model.uniforms.insert("shadow_projection", model::UniformValue::Matrix4(cgmath::SquareMatrix::identity()));
                    }
                    edit.model.set_verts(verts);
                    edit.x = wall_bounds.min.x;
                    edit.y = wall_bounds.min.y;
                    edit.width = wall_bounds.width();
                    edit.height = wall_bounds.height();
                    edit.model.uniforms.insert("u_cycle", model::UniformValue::Float(match room.state {
                        level::RoomState::Planning{..} => 0.0,
                        level::RoomState::Building{..} => 80.0,
                        _ => unreachable!(),
                    }));
                    edit.touched = true;
                } else if let Some(edit) = self.edit_sections.get_mut(&room_id) {
                    edit.touched = true;
                }
            }
        }

        // Find old edit sections and remove them
        self.edit_sections.retain(|_id, edit| edit.touched);

        let lower_regions = self.lowered_region.map(|v| (
            v,
            {
                let mut v = v;
                v.min -= (1, 1);
                v
            }
        ));

        let mut data = vec![];
        for section in &mut self.sections {
            if !level.get_and_clear_dirty_section(section.x, section.y) {
                continue;
            }
            data.clear();

            let min_x = if section.x == 0 { -1 } else { (section.x * SECTION_SIZE) as i32 };
            let min_y = if section.y == 0 { -1 } else { (section.y * SECTION_SIZE) as i32};
            let max_x = ((section.x + 1) * SECTION_SIZE) as i32;
            let max_y = ((section.y + 1) * SECTION_SIZE) as i32;

            for x in min_x .. max_x {
                if x >= level.width as i32 {
                    break;
                }
                for y in min_y .. max_y {
                    if y >= level.height as i32 {
                        break;
                    }
                    let loc = Location::new(x, y);
                    if level.get_tile_flags(loc).contains(level::TileFlag::BUILDING) {
                        continue;
                    }
                    if x >= 0 && y >= 0 {
                        Self::make_tile(&self.log, &self.asset_manager, target_atlas, level, loc, &mut data);
                    }
                    // Walls
                    let (wall_height, bounds) = if let Some(lower) = lower_regions {
                        if lower.1.in_bounds(loc) {
                            if !lower.0.in_bounds(loc) {
                                (4.0/16.0, Some(lower.1))
                            } else {
                                (4.0/16.0, Some(lower.0))
                            }
                        } else {
                            (1.0, None)
                        }
                    } else {
                        (1.0, None)
                    };

                    Self::place_walls_limit(
                        &self.log,
                        &self.asset_manager, target_atlas,
                        &mut self.windows,
                        level, level, loc, &mut data, true, true,
                        wall_height, bounds
                    );
                }
            }
            let count = data.len();
            section.array.bind();
            section.buffer.bind(gl::BufferTarget::Array);
            if section.max_count < count {
                section.buffer.set_data(gl::BufferTarget::Array, &data, gl::BufferUsage::Dynamic);
                section.max_count = count;
            } else {
                section.buffer.set_data_range(gl::BufferTarget::Array, &data, 0);
            }
            section.count = count;
        }
        self.update_guides(ctx, level);
    }

    fn update_guides(&mut self, ctx: &mut pipeline::Context<'_>, level: &level::Level) {
        for id in level.room_ids() {
            let room = level.get_room_info(id);
            if let Some(bounds) = room.get_placement_bounds() {
                if bounds == (0.0, 0.0, 0.0, 0.0) {
                    self.placement_guides.remove(&id);
                    continue;
                }
                let guide = self.placement_guides.entry(id).or_insert_with(|| PlacementGuide {
                    model: model::Model::new(
                        ctx,
                        "placement",
                        vec![],
                        vec![],
                    ),
                    bounds: (0.0, 0.0, 0.0, 0.0),
                    texture: gl::Texture::new(),
                });
                if guide.bounds == bounds {
                    continue;
                }
                guide.bounds = bounds;
                {
                    guide.texture.bind(gl::TextureTarget::Texture2D);
                    let min_x = (bounds.0 * 4.0).floor() as usize;
                    let min_y = (bounds.1 * 4.0).floor() as usize;
                    let max_x = (bounds.2 * 4.0).ceil() as usize;
                    let max_y = (bounds.3 * 4.0).ceil() as usize;
                    let width = max_x - min_x;
                    let height = max_y - min_y;

                    let mut data = vec![0u8; width * height];
                    for y in min_y .. max_y {
                        for x in min_x .. max_x {
                            let loc = Location::new((x / 4) as i32, (y / 4) as i32);
                            let didx = (x - min_x) + (y - min_y) * width;
                            if room.area.in_bounds(loc) {
                                data[didx] = if room.is_virt_placeable_scaled(x as i32, y as i32) {
                                    255
                                } else {
                                    0
                                };
                            } else if let Some(room) = level.get_room_owner(loc) {
                                let room = level.get_room_info(room);
                                data[didx] = if room.is_placeable_scaled(x as i32, y as i32) {
                                    255
                                } else {
                                    0
                                };
                            } else {
                                data[didx] = 255;
                            }
                        }
                    }

                    gl::pixel_store(gl::PixelStore::UnpackAlignment, 1);
                    guide.texture.image_2d_ex(
                        gl::TextureTarget::Texture2D, 0,
                        width as u32, height as u32,
                        gl::TextureFormat::R8, gl::TextureFormat::Red,
                        gl::Type::UnsignedByte, Some(&*data),
                    );
                    guide.texture.set_parameter::<gl::TextureMinFilter>(gl::TextureTarget::Texture2D, gl::TextureFilter::Nearest);
                    guide.texture.set_parameter::<gl::TextureMagFilter>(gl::TextureTarget::Texture2D, gl::TextureFilter::Nearest);
                    guide.texture.set_parameter::<gl::TextureWrapS>(gl::TextureTarget::Texture2D, gl::TextureWrap::ClampToEdge);
                    guide.texture.set_parameter::<gl::TextureWrapT>(gl::TextureTarget::Texture2D, gl::TextureWrap::ClampToEdge);
                    guide.texture.set_parameter::<gl::TextureBaseLevel>(gl::TextureTarget::Texture2D, 0);
                    guide.texture.set_parameter::<gl::TextureMaxLevel>(gl::TextureTarget::Texture2D, 0);
                    gl::pixel_store(gl::PixelStore::UnpackAlignment, 4);
                }

                let mut verts = Vec::with_capacity(6 * 5);
                fn push_squ(verts: &mut Vec<PlacementVertex>, x1: f32, z1: f32, x2: f32, z2: f32) {
                    verts.push(PlacementVertex {
                        pos: cgmath::Vector3 {x: x1, y: 0.001, z: z1},
                        lookup_x: 0.0,
                        lookup_y: 0.0,
                    });
                    verts.push(PlacementVertex {
                        pos: cgmath::Vector3 {x: x1, y: 0.001, z: z2},
                        lookup_x: 0.0,
                        lookup_y: 1.0,
                    });
                    verts.push(PlacementVertex {
                        pos: cgmath::Vector3 {x: x2, y: 0.001, z: z1},
                        lookup_x: 1.0,
                        lookup_y: 0.0,
                    });
                    verts.push(PlacementVertex {
                        pos: cgmath::Vector3 {x: x2, y: 0.001, z: z1},
                        lookup_x: 1.0,
                        lookup_y: 0.0,
                    });
                    verts.push(PlacementVertex {
                        pos: cgmath::Vector3 {x: x1, y: 0.001, z: z2},
                        lookup_x: 0.0,
                        lookup_y: 1.0,
                    });
                    verts.push(PlacementVertex {
                        pos: cgmath::Vector3 {x: x2, y: 0.001, z: z2},
                        lookup_x: 1.0,
                        lookup_y: 1.0,
                    });
                }
                push_squ(&mut verts, bounds.0, bounds.1, bounds.2, bounds.3);

                push_squ(&mut verts, 0.0, bounds.1, bounds.0, bounds.3);
                push_squ(&mut verts, bounds.2, bounds.1, level.width as f32 + 1.0, bounds.3);

                push_squ(&mut verts, bounds.0, 0.0, bounds.2, bounds.1);
                push_squ(&mut verts, bounds.0, bounds.3, bounds.2, level.height as f32 + 1.0);

                for v in &mut verts[6..] {
                    v.lookup_x = -1.0;
                }

                guide.model = model::Model::new(
                    ctx,
                    "placement",
                    vec![
                        model::Attribute {name: "attrib_position", count: 3, ty: gl::Type::Float, offset: 0, int: false},
                        model::Attribute {name: "attrib_lookup", count: 2, ty: gl::Type::Float, offset: 12, int: false},
                    ],
                    verts
                );
            } else {
                self.placement_guides.remove(&id);
            }
        }
    }

    pub(super) fn gen_verts_for_tile<L: level::LevelView>(
        &mut self,
        target_atlas: &mut super::GlobalAtlas,
        real_level: &Level,
        level: &L,
        loc: Location
    ) -> Vec<GLVertex> {
        let mut data = vec![];
        Self::make_tile(&self.log, &self.asset_manager, target_atlas, level, loc, &mut data);
        Self::place_walls(&self.log, &self.asset_manager, target_atlas, &mut self.windows, real_level, level, loc, &mut data);
        data
    }


    fn place_walls<L: level::LevelView>(
        log: &Logger,
        asset_manager: &assets::AssetManager,
        target_atlas: &mut super::GlobalAtlas,
        windows: &mut Vec<window::Model>,
        real_level: &Level,
        level: &L, loc: Location,
        data: &mut Vec<GLVertex>,
    ) {
        Self::place_walls_limit(log, asset_manager, target_atlas, windows, real_level, level, loc, data, true, true, 1.0, None)
    }

    fn place_walls_limit<L>(
        log: &Logger,
        asset_manager: &assets::AssetManager,
        target_atlas: &mut super::GlobalAtlas,
        windows: &mut Vec<window::Model>,
        real_level: &Level,
        level: &L, loc: Location,
        data: &mut Vec<GLVertex>,
        enable_south: bool, enable_west: bool,
        wall_height: f32, bounds: Option<Bound>,
    )
        where L: level::LevelView,
    {
        let cx = loc.x as f32;
        let cy = loc.y as f32;
        let t_self = level.get_tile(loc);
        let t_room_o = level.get_room_owner(loc);
        let t_room_i = t_room_o.map(|v| real_level.get_room_info(v));
        let t_room = t_room_i.map(|v| assume!(log, asset_manager.loader_open::<room::Loader>(v.key.borrow())));

        // Texture mapping functions, swapped depending on direction.
        fn tex_func(vx: f32, vy: f32, _: f32) -> (f32, f32) {
            (vx, 1.0 - vy)
        }
        fn tex_func2(_: f32, vy: f32, vz: f32) -> (f32, f32) {
            (vz, 1.0 - vy)
        }
        let cull_check = |loc: Location, dir: Direction, ndir: Direction| {
            level.get_wall_info(loc.shift(dir), ndir).is_none()
        };

        // Details for each wall side.
        let walls = [
            (Direction::South, FACE_RIGHT, FACE_LEFT,
                &tex_func as &dyn Fn(f32, f32, f32) -> (f32, f32),
                &tex_func2 as &dyn Fn(f32, f32, f32) -> (f32, f32),
                (0.0, 0.0, 1.0),
                (1.0, 0.0, 0.0),
            ),
            (Direction::West, FACE_LEFT, FACE_RIGHT,
                &tex_func2,
                &tex_func,
                (1.0, 0.0, 0.0),
                (0.0, 0.0, 1.0),
            ),
        ];
        for &(direction, face_a, face_b, tex_func, tex_func2, nor_a, nor_b) in &walls {
            let wall_info = level.get_wall_info(loc, direction);
            if level.get_tile_flags(loc.shift(direction)).contains(level::TileFlag::BUILDING) {
                continue;
            }
            if (direction == Direction::South && !enable_south)
                || (direction == Direction::West && !enable_west) {
                continue;
            }
            if let Some(wall_info) = wall_info {
                if wall_info.flag == level::TileWallFlag::Door {
                    continue;
                }
                let (mul_x, mul_y) = direction.offset();
                let (mulf_x, mulf_y) = (mul_x as f32, mul_y as f32);
                let (shift_x, shift_y) = (mulf_x * (15.0/16.0), mulf_y * (15.0/16.0));

                // tuple contents: (priority, tex, tex_top)
                let a_wall = if let Some(wall) = t_room.as_ref().and_then(|v| v.wall.as_ref()) {
                    if t_self.should_place_wall(level, loc, direction) {
                        Some((
                            wall.priority,
                            wall.texture.borrow(),
                            wall.texture_top.as_ref()
                                .map(|v| v.borrow())
                                .unwrap_or_else(|| wall.texture.borrow())
                        ))
                    } else {
                        None
                    }
                } else { None };

                let t_shifted = level.get_tile(loc.shift(direction));
                let t_shifted_room_o = level.get_room_owner(loc.shift(direction));
                let t_shifted_room_i = t_shifted_room_o.map(|v| real_level.get_room_info(v));
                let t_shifted_room = t_shifted_room_i.map(|v| assume!(log, asset_manager.loader_open::<room::Loader>(v.key.borrow())));
                let b_wall = if let Some(wall) = t_shifted_room.as_ref().and_then(|v| v.wall.as_ref()) {
                    if t_shifted.should_place_wall(level, loc.shift(direction), direction.reverse()) {
                        Some((
                            wall.priority,
                            wall.texture.borrow(),
                            wall.texture_top.as_ref()
                                .map(|v| v.borrow())
                                .unwrap_or_else(|| wall.texture.borrow())
                        ))
                    } else {
                        None
                    }
                } else { None };

                // Select the tile with the wall defined if there is
                // only one of the tiles with a wall.
                //
                // In the case both have a wall the large faces of
                // the wall will use the texture from the side the tile
                // is on (a on one side, b on the other). The top and sides
                // will prefer a's textures but will use b's if the priority
                // is set
                let (tex_f, tex_b, tex_s, tex_top) = match (&a_wall, &b_wall) {
                    (&Some(ref a_wall), &None) => (a_wall.1.borrow(), a_wall.1.borrow(), a_wall.1.borrow(), a_wall.2.borrow()),
                    (&None, &Some(ref b_wall)) => (b_wall.1.borrow(), b_wall.1.borrow(), b_wall.1.borrow(), b_wall.2.borrow()),
                    (&Some(ref a_wall), &Some(ref b_wall)) => if b_wall.0 {
                        (a_wall.1.borrow(), b_wall.1.borrow(), b_wall.1.borrow(), b_wall.2.borrow())
                    } else {
                        (a_wall.1.borrow(), b_wall.1.borrow(), a_wall.1.borrow(), a_wall.2.borrow())
                    },
                    (&None, &None) => panic!("Invalid wall placement"),
                };

                // Texture for the wall
                let texture_f_id = Self::get_texture_id(log, asset_manager, target_atlas, tex_f);
                let texture_b_id = Self::get_texture_id(log, asset_manager, target_atlas, tex_b);
                let texture_s_id = Self::get_texture_id(log, asset_manager, target_atlas, tex_s);
                let texture_top_id = Self::get_texture_id(log, asset_manager, target_atlas, tex_top);

                if wall_height >= 1.0 {
                    if let level::TileWallFlag::Window(id) = wall_info.flag {
                        if id as usize <= windows.len() {
                            for i in windows.len() as u8 ..= id {
                                let key = level.get_window(i);
                                windows.push(window::Model::load(log, asset_manager, target_atlas, key));
                            }
                        }

                        let mdl = &windows[id as usize];
                        let verts = if direction == Direction::West {
                            &mdl.verts_x
                        } else {
                            &mdl.verts_z
                        };
                        for v in verts {
                            let mut v = v.clone();
                            v.x += cx;
                            v.z += cy;
                            if v.texture <= -4.0 {
                                v.texture = texture_s_id.0 as f32;
                                v.texture_offset_x += texture_s_id.1.x as f32;
                                v.texture_offset_y += texture_s_id.1.y as f32;
                            } else if v.texture <= -3.0 {
                                v.texture = texture_top_id.0 as f32;
                                v.texture_offset_x += texture_top_id.1.x as f32;
                                v.texture_offset_y += texture_top_id.1.y as f32;
                            } else if v.texture <= -2.0 {
                                v.texture = texture_f_id.0 as f32;
                                v.texture_offset_x += texture_f_id.1.x as f32;
                                v.texture_offset_y += texture_f_id.1.y as f32;
                            } else if v.texture <= -1.0 {
                                v.texture = texture_b_id.0 as f32;
                                v.texture_offset_x += texture_b_id.1.x as f32;
                                v.texture_offset_y += texture_b_id.1.y as f32;
                            }
                            data.push(v);
                        }

                        continue;
                    }
                }

                // Intersection handling
                let mut min = 0.0;
                let mut max = 1.0;
                if direction == Direction::West {
                    match (
                        level.get_wall_info(loc.shift(Direction::South), Direction::West),
                        level.get_wall_info(loc, Direction::South),
                        level.get_wall_info(loc.shift(Direction::West), Direction::South)
                    ) {
                        // Wall above, shrink
                        (None, Some(_), Some(_)) |
                        (None, None, Some(_)) |
                        (None, Some(_), None) => max = 15.0 / 16.0,
                        // Override if we are part of a wall and the south
                        // ones aren't
                        (Some(_), None, Some(_)) |
                        (Some(_), Some(_), None) => max = 1.0,
                        _ => {},
                    }
                    match (
                        level.get_wall_info(loc.shift(Direction::North), Direction::West),
                        level.get_wall_info(loc, Direction::North),
                        level.get_wall_info(loc.shift(Direction::West), Direction::North)
                    ) {
                        // Wall below, shrink
                        (None, Some(_), Some(_)) |
                        (None, None, Some(_)) |
                        (None, Some(_), None) => min = 1.0 / 16.0,
                        // Override if we are part of a wall and the south
                        // ones aren't
                        (Some(_), None, Some(_)) |
                        (Some(_), Some(_), None) => min = 0.0,
                        _ => {},
                    }
                } else {
                    match (
                        level.get_wall_info(loc.shift(Direction::East), Direction::South),
                        level.get_wall_info(loc, Direction::East),
                        level.get_wall_info(loc.shift(Direction::South), Direction::East)
                    ) {
                        (None, None, Some(_)) |
                        (None, Some(_), None) => min = -1.0 / 16.0,
                        (None, Some(_), Some(_)) => min = 1.0 / 16.0,
                        _ => {},
                    }
                    match (
                        level.get_wall_info(loc.shift(Direction::West), Direction::South),
                        level.get_wall_info(loc, Direction::West),
                        level.get_wall_info(loc.shift(Direction::South), Direction::West)
                    ) {
                        (None, None, Some(_)) |
                        (None, Some(_), None) => max = 17.0 / 16.0,
                        (None, Some(_), Some(_)) => max = 15.0 / 16.0,
                        _ => {},
                    }
                }

                let (shift, size, size_full) = if mul_y == 1 {
                    ((min, 0.0, 0.0), (max - min, wall_height, 2.0 / 16.0), (max - min, wall_height, 1.0))
                } else {
                    ((0.0, 0.0, min), (2.0 / 16.0, wall_height, max - min), (1.0, wall_height, max - min))
                };

                let nor_a_r = (-nor_a.0, -nor_a.1, -nor_a.2);
                let nor_b_r = (-nor_b.0, -nor_b.1, -nor_b.2);

                Self::make_wall(
                    data, face_a.iter(), texture_f_id,
                    (cx, 0.0, cy), (shift_x + min * mulf_y, 0.0, shift_y + min * mulf_x), size_full,
                    nor_a_r,
                    tex_func
                );
                Self::make_wall(
                    data, face_a.iter().rev(), texture_b_id,
                    (cx, 0.0, cy), (shift_x + (2.0/16.0) * mulf_x + min * mulf_y, 0.0, shift_y + (2.0/16.0) * mulf_y + min * mulf_x), size_full,
                    nor_a,
                    tex_func
                );

                // Cull the sides if a wall is to the side of this wall.
                if cull_check(loc, Direction::from_offset(-mul_y, -mul_x), direction) {
                    Self::make_wall(
                        data, face_b.iter(), texture_s_id,
                        (cx, 0.0, cy), (shift_x + shift.0, 0.0, shift_y + shift.2), size,
                        nor_b_r,
                        tex_func2
                    );
                } else if let Some(bounds) = bounds {
                    if wall_height >= 1.0 && !bounds.in_bounds(loc.shift(Direction::from_offset(-mul_y, -mul_x))) {
                        let mut size = size;
                        size.1 = 1.0 - wall_height;
                        Self::make_wall(
                            data, face_b.iter().rev(), texture_s_id,
                            (cx, 0.0, cy), (shift_x + shift.0, wall_height, shift_y + shift.2), size,
                            nor_b,
                            tex_func2
                        );
                    }
                }
                if cull_check(loc, Direction::from_offset(mul_y, mul_x), direction) {
                    Self::make_wall(
                        data, face_b.iter().rev(), texture_s_id,
                        (cx, 0.0, cy), (size_full.0 * mulf_y + shift_x + shift.0, 0.0, size_full.2 * mulf_x + shift_y + shift.2), size,
                        nor_b,
                        tex_func2
                    );
                } else if let Some(bounds) = bounds {
                    if wall_height >= 1.0 && !bounds.in_bounds(loc.shift(Direction::from_offset(mul_y, mul_x))) {
                        let mut size = size;
                        size.1 = 1.0 - wall_height;
                        Self::make_wall(
                            data, face_b.iter(), texture_s_id,
                            (cx, 0.0, cy), (size_full.0 * mulf_y + shift_x + shift.0, wall_height, size_full.2 * mulf_x + shift_y + shift.2), size,
                            nor_b_r,
                            tex_func2
                        );
                    }
                }
                // Top of the wall
                Self::make_wall(
                    data, FACE_FLOOR.iter(), texture_top_id,
                    (cx, 0.0, cy), (shift_x + shift.0, wall_height, shift_y + shift.2), size,
                    (0.0, 1.0, 0.0),
                    |vx, _, vz| if direction == Direction::South { (vx, 1.0 - vz) } else { (vz, 1.0 - vx) }
                );
            }
        }
    }

    fn make_tile<L: level::LevelView>(
        log: &Logger,
        asset_manager: &assets::AssetManager,
        target_atlas: &mut super::GlobalAtlas,
        level: &L, loc: Location,
        data: &mut Vec<GLVertex>
    ) {
        let cx = loc.x as f32;
        let cy = loc.y as f32;
        let t_self = level.get_tile(loc);
        // Find the texture for the floor.
        let tex = t_self.get_texture_for(level, loc);
        let texture_id = Self::get_texture_id(log, asset_manager, target_atlas, tex);
        // Tile's floor texture
        Self::make_wall(
            data, FACE_FLOOR.iter(),
            texture_id,
            (cx, 0.0, cy), (0.0, 0.0, 0.0), (1.0, 1.0, 1.0),
            (0.0, 1.0, 0.0),
            |vx, _, vz| (vx, 1.0 - vz)
        );
    }

    fn make_wall<'a, F, V>(
            data: &mut Vec<GLVertex>, verts: V,
            texture: (i32, Rect),
            base: (f32, f32, f32), offset: (f32, f32, f32),
            scale: (f32, f32, f32), normal: (f32, f32, f32), tmap: F)
        where F: Fn(f32, f32, f32) -> (f32, f32),
              V: Iterator<Item=&'a Vertex> {
        for vert in verts {
            let (tx, ty) = tmap(
                offset.0 + f32::from(vert.x) * scale.0,
                offset.1 + f32::from(vert.y) * scale.1,
                offset.2 + f32::from(vert.z) * scale.2,
            );
            let mw = 0.5/(texture.1.width as f32);
            let mh = 0.5/(texture.1.height as f32);
            data.push(GLVertex {
                x: base.0 + offset.0 + (f32::from(vert.x) * scale.0),
                y: base.1 + offset.1 + (f32::from(vert.y) * scale.1),
                z: base.2 + offset.2 + (f32::from(vert.z) * scale.2),

                nx: normal.0,
                ny: normal.1,
                nz: normal.2,

                texture: texture.0 as f32,
                texture_offset_x: texture.1.x as f32 + tx.max(mw).min(1.0 - mw) * texture.1.width as f32,
                texture_offset_y: texture.1.y as f32 + ty.max(mh).min(1.0 - mh) * texture.1.height as f32,
                brightness: 1.0,
            });
        }
    }

    pub fn get_render_bounds(&self, frustum: &Frustum) -> (f32, f32, f32) {
        use std::f32;
        let mut total_x = 0.0;
        let mut total_y = 0.0;
        let mut render_count = 0.0;

        let mut min_pos = (f32::INFINITY, f32::INFINITY);
        let mut max_pos = (0.0f32, 0.0f32);

        for sec_x in 0 .. ((self.width + 3) / 4) {
            for sec_y in 0 .. ((self.height + 3) / 4) {
                let sx = (sec_x * 4) as f32;
                let sy = (sec_y * 4) as f32;
                let min = cgmath::Vector3::new(
                    sx,
                    0.0,
                    sy
                );
                let max = cgmath::Vector3::new(
                    sx + 4.0,
                    2.5,
                    sy + 4.0,
                );
                if frustum.contains_aabb(min, max) {
                    render_count += 1.0;
                    total_x += sx + 2.0;
                    total_y += sy + 2.0;

                    min_pos.0 = min_pos.0.min(min.x);
                    min_pos.1 = min_pos.1.min(min.z);
                    max_pos.0 = max_pos.0.max(max.x);
                    max_pos.1 = max_pos.1.max(max.z);
                }
            }
        }

        let size = (max_pos.1 - min_pos.1).hypot(max_pos.0 - min_pos.0);

        (
            total_x / render_count,
            total_y / render_count,
            size,
        )
    }

    pub fn draw(
        &mut self, ctx: &mut pipeline::Context<'_>,
        frustum: &Frustum,
        projection: &Matrix4<f32>,
        view_matrix: &Matrix4<f32>,
        shadow_view_matrix: Option<&Matrix4<f32>>,
        shadow_projection: Option<&Matrix4<f32>>,
    ) {
        {
            let prog = ctx.program("terrain");
            prog.use_program();
            prog.uniform("view_matrix").map(|v| v.set_matrix4(view_matrix));
            prog.uniform("projection_matrix").map(|v| v.set_matrix4(projection));
            prog.uniform("u_textures").map(|v| v.set_int(super::GLOBAL_TEXTURE_LOCATION as _));
            prog.uniform("shadow_map").map(|v| v.set_int(super::SHADOW_MAP_LOCATION as _));
            if let (Some(sv), Some(sp)) = (shadow_view_matrix, shadow_projection) {
                prog.uniform("shadow_matrix").map(|v| v.set_matrix4(sv));
                prog.uniform("shadow_projection").map(|v| v.set_matrix4(sp));
            }

            for section in &self.sections {
                let min = cgmath::Vector3::new(
                    (section.x * SECTION_SIZE) as f32 - 1.5,
                    0.0,
                    (section.y * SECTION_SIZE) as f32 - 1.5
                );
                let max = cgmath::Vector3::new(
                    (section.x * SECTION_SIZE) as f32 + SECTION_SIZE as f32 + 1.5,
                    2.5,
                    (section.y * SECTION_SIZE) as f32 + SECTION_SIZE as f32 + 1.5,
                );
                if section.count > 0 && frustum.contains_aabb(min, max){
                    section.array.bind();
                    gl::draw_arrays(gl::DrawType::Triangles, 0, section.count);
                }
            }
        }

        // Placement guidelines
        if shadow_view_matrix.is_some() {
            for placement in self.placement_guides.values_mut() {
                placement.texture.bind(gl::TextureTarget::Texture2D);
                gl::active_texture(0);
                placement.model.uniforms.entry("u_placement_map").or_insert(model::UniformValue::Int(0));
                placement.model.draw(ctx, projection, view_matrix);
            }
        }

        // Edit areas
        for edit in self.edit_sections.values_mut() {
            let min = cgmath::Vector3::new(edit.x as f32 - 1.5, 0.0, edit.y as f32 - 1.5);
            let max = cgmath::Vector3::new(
                edit.x as f32 + edit.width as f32 + 1.5,
                2.5,
                edit.y as f32 + edit.height as f32 + 1.5,
            );
            if !frustum.contains_aabb(min, max) {
                continue;
            }
            if let (Some(sv), Some(sp)) = (shadow_view_matrix, shadow_projection) {
                *assume!(self.log, edit.model.uniforms.get_mut("shadow_matrix")) = model::UniformValue::Matrix4(*sv);
                *assume!(self.log, edit.model.uniforms.get_mut("shadow_projection")) = model::UniformValue::Matrix4(*sp);
            }
            edit.model.draw(ctx, projection, view_matrix);
        }
    }

    fn get_texture_id(log: &Logger, asset_manager: &assets::AssetManager, target_atlas: &mut super::GlobalAtlas, tex: assets::ResourceKey<'_>) -> (i32, Rect) {
        super::RenderState::texture_info_for(
            log,
            asset_manager,
            target_atlas,
            tex
        )
    }
}
