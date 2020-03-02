use cgmath;

use super::image;
use super::{atlas, exmodel, gl, pipeline, ModelKey, ModelKeyBorrow, PassFlag};
use crate::ecs;
use crate::entity;
use crate::prelude::*;
use crate::server::assets;
use crate::util::FNVMap;

pub struct Info {
    log: Logger,
    pub(super) models: FNVMap<ModelKey, Model>,

    water_normal: gl::Texture,

    attrib_position: gl::Attribute,
    attrib_normal: gl::Attribute,
    attrib_uv: gl::Attribute,
    attrib_matrix: gl::Attribute,
    attrib_tint: gl::Attribute,
    attrib_highlight: gl::Attribute,
}

pub(super) struct Model {
    pub(super) info: exmodel::Model,
    texture: (i32, atlas::Rect),

    time: f64,
    // TODO: Make this more general instead of a special case
    use_water: bool,
    has_highlights: bool,

    array: gl::VertexArray,
    _buffer: gl::Buffer,
    _index_buffer: gl::Buffer,
    index_ty: gl::Type,
    pub(super) matrix_buffer: gl::Buffer,

    pub(super) entity_map: FNVMap<ecs::Entity, usize>,
    max_count: usize,
    pub(super) dyn_info: Vec<DynInfo>,
}

#[repr(C)]
pub(super) struct DynInfo {
    pub(super) matrix: cgmath::Matrix4<f32>,
    tint: (u8, u8, u8, u8),
    highlight: (u8, u8, u8, u8),
}

impl Info {
    pub(super) fn new(
        log: &Logger,
        ctx: &mut pipeline::Context<'_>,
        asset_manager: &AssetManager,
    ) -> Info {
        let log = log.new(o!("source" => "static_model"));
        let (
            attrib_position,
            attrib_normal,
            attrib_uv,
            attrib_matrix,
            attrib_tint,
            attrib_highlight,
        ) = {
            let program = ctx.program("static");
            program.use_program();
            (
                assume!(log, program.attribute("attrib_position")),
                assume!(log, program.attribute("attrib_normal")),
                assume!(log, program.attribute("attrib_uv")),
                assume!(log, program.attribute("attrib_matrix")),
                assume!(log, program.attribute("attrib_tint")),
                assume!(log, program.attribute("attrib_highlight")),
            )
        };

        let img = assume!(
            log,
            asset_manager.loader_open::<image::Loader>(ResourceKey::new(
                "base",
                "models/garden/water_normal"
            ))
        );
        let img = assume!(log, img.wait_take_image());
        let mut normal_data = Vec::with_capacity((img.width * img.height * 3) as usize);
        for d in img.data.chunks_exact(4) {
            normal_data.push(d[0]);
            normal_data.push(d[1]);
            normal_data.push(d[2]);
        }

        let water_normal = gl::Texture::new();
        water_normal.bind(gl::TextureTarget::Texture2D);
        water_normal.image_2d_any(
            gl::TextureTarget::Texture2D,
            0,
            img.width,
            img.height,
            gl::TextureFormat::Rgb8,
            gl::TextureFormat::Rgb,
            gl::Type::UnsignedByte,
            Some(&normal_data),
        );
        water_normal.set_parameter::<gl::TextureMinFilter>(
            gl::TextureTarget::Texture2D,
            gl::TextureFilter::Linear,
        );
        water_normal.set_parameter::<gl::TextureMagFilter>(
            gl::TextureTarget::Texture2D,
            gl::TextureFilter::Linear,
        );
        water_normal.set_parameter::<gl::TextureWrapS>(
            gl::TextureTarget::Texture2D,
            gl::TextureWrap::Repeat,
        );
        water_normal.set_parameter::<gl::TextureWrapT>(
            gl::TextureTarget::Texture2D,
            gl::TextureWrap::Repeat,
        );

        Info {
            log,
            models: FNVMap::default(),

            water_normal,

            attrib_position,
            attrib_normal,
            attrib_uv,
            attrib_matrix,
            attrib_tint,
            attrib_highlight,
        }
    }
}

impl<'a> super::EntityRender<'a> for Info {
    type Component = crate::entity::StaticModel;
    type Params = (
        ecs::Read<'a, Rotation>,
        ecs::Read<'a, InvalidPlacement>,
        ecs::Read<'a, entity::Color>,
        ecs::Read<'a, entity::Highlighted>,
    );

    fn clear(&mut self) {
        for gl_model in self.models.values_mut() {
            gl_model.dyn_info.clear();
            gl_model.entity_map.clear();
        }
    }

    fn compute_frame(
        &mut self,
        asset_manager: &assets::AssetManager,
        config: &Config,
        global_atlas: &mut super::GlobalAtlas,
        _em: &EntityManager<'_>,
        pos: &Read<Position>,
        _size: &Read<Size>,
        _model: &Read<entity::Model>,
        _model_tex: &Read<entity::ModelTexture>,
        _component: &Read<Self::Component>,
        (rot, invalid, color, highlight): &Self::Params,
        model_key: assets::ResourceKey<'_>,
        texture: Option<assets::ResourceKey<'_>>,
        ents: &[ecs::Entity],
        delta: f64,
    ) {
        use cgmath::prelude::*;
        use std::collections::hash_map::Entry;
        use std::mem;

        let key = ModelKey {
            key: ModelKeyBorrow(
                model_key.into_owned(),
                texture.as_ref().map(|v| v.borrow().into_owned()),
            ),
        };
        let model = match self.models.entry(key) {
            Entry::Occupied(val) => val.into_mut(),
            Entry::Vacant(val) => {
                let mut file = assume!(
                    self.log,
                    asset_manager.open_from_pack(
                        val.key().0.module_key(),
                        &format!("models/{}.umod", val.key().0.resource())
                    )
                );
                let minfo = assume!(self.log, exmodel::Model::read_from(&mut file));

                let array = gl::VertexArray::new();
                array.bind();

                let model_buffer = gl::Buffer::new();
                model_buffer.bind(gl::BufferTarget::Array);

                self.attrib_position.enable();
                self.attrib_position.vertex_pointer(
                    3,
                    gl::Type::Float,
                    false,
                    mem::size_of::<exmodel::Vertex>() as i32,
                    0,
                );
                self.attrib_normal.enable();
                self.attrib_normal.vertex_pointer(
                    3,
                    gl::Type::Float,
                    false,
                    mem::size_of::<exmodel::Vertex>() as i32,
                    12,
                );
                self.attrib_uv.enable();
                self.attrib_uv.vertex_pointer(
                    2,
                    gl::Type::Float,
                    false,
                    mem::size_of::<exmodel::Vertex>() as i32,
                    24,
                );

                model_buffer.set_data(
                    gl::BufferTarget::Array,
                    &minfo.verts,
                    gl::BufferUsage::Static,
                );

                let model_index_buffer = gl::Buffer::new();
                model_index_buffer.bind(gl::BufferTarget::ElementArray);
                let index_ty = if minfo.verts.len() <= 0xFF {
                    super::animated_model::set_indices_packed::<_, u8>(
                        minfo.faces.iter(),
                        &model_index_buffer,
                    );
                    gl::Type::UnsignedByte
                } else if minfo.verts.len() <= 0xFFFF {
                    super::animated_model::set_indices_packed::<_, u16>(
                        minfo.faces.iter(),
                        &model_index_buffer,
                    );
                    gl::Type::UnsignedShort
                } else {
                    super::animated_model::set_indices_packed::<_, u32>(
                        minfo.faces.iter(),
                        &model_index_buffer,
                    );
                    gl::Type::UnsignedInt
                };

                let matrix_buffer = gl::Buffer::new();
                matrix_buffer.bind(gl::BufferTarget::Array);

                for i in 0..4 {
                    let attrib = self.attrib_matrix.offset(i);
                    attrib.enable();
                    attrib.vertex_pointer(
                        4,
                        gl::Type::Float,
                        false,
                        mem::size_of::<DynInfo>() as i32,
                        i as i32 * 4 * 4,
                    );
                    attrib.divisor(1);
                }
                self.attrib_tint.enable();
                self.attrib_tint.vertex_pointer(
                    4,
                    gl::Type::UnsignedByte,
                    true,
                    mem::size_of::<DynInfo>() as i32,
                    4 * 4 * 4,
                );
                self.attrib_tint.divisor(1);
                self.attrib_highlight.enable();
                self.attrib_highlight.vertex_pointer(
                    4,
                    gl::Type::UnsignedByte,
                    true,
                    mem::size_of::<DynInfo>() as i32,
                    4 * 4 * 4 + 4,
                );
                self.attrib_highlight.divisor(1);

                let texture = {
                    let tex = texture.unwrap_or_else(|| {
                        assets::LazyResourceKey::parse(&minfo.texture)
                            .or_module(val.key().0.module_key())
                    });
                    super::RenderState::texture_info_for(
                        &self.log,
                        asset_manager,
                        global_atlas,
                        tex,
                    )
                };

                let use_water = val.key().0 == ResourceKey::new("base", "garden/pond_water");

                val.insert(Model {
                    info: minfo,
                    texture,

                    use_water,
                    has_highlights: false,
                    time: 0.0,

                    array,
                    _buffer: model_buffer,
                    _index_buffer: model_index_buffer,
                    index_ty,
                    matrix_buffer,
                    max_count: 0,
                    entity_map: FNVMap::default(),
                    dyn_info: vec![],
                })
            }
        };

        model.array.bind();
        model.dyn_info.clear();

        model.time += delta;
        model.time %= f64::from(0xFF_FF_FF);

        let log = &self.log;

        let selection_invalid = config.placement_invalid_colour.get();

        let mut has_highlights = false;
        for (idx, e) in ents.iter().enumerate() {
            model.entity_map.insert(*e, idx);
            let pos = assume!(log, pos.get_component(*e));
            let rot = assume!(log, rot.get_component(*e));
            let mat = cgmath::Decomposed {
                scale: 1.0 / 10.0,
                rot: cgmath::Quaternion::from_angle_y(cgmath::Rad(rot.rotation.raw())),
                disp: cgmath::Vector3::new(pos.x, pos.y, pos.z),
            };
            let tint = if invalid.get_component(*e).is_some() {
                (
                    selection_invalid.0,
                    selection_invalid.1,
                    selection_invalid.2,
                    255,
                )
            } else if let Some(col) = color.get_component(*e) {
                col.color
            } else {
                (255, 255, 255, 255)
            };
            let highlight = if let Some(h) = highlight.get_component(*e) {
                has_highlights = true;
                (h.color.0, h.color.1, h.color.2, 255)
            } else {
                (0, 0, 0, 0)
            };

            let mat: cgmath::Matrix4<f32> = mat.into();
            model.dyn_info.push(DynInfo {
                matrix: mat * model.info.transform,
                tint,
                highlight,
            });
        }
        model.has_highlights = has_highlights;
        model.matrix_buffer.bind(gl::BufferTarget::Array);
        if model.max_count < model.dyn_info.len() {
            model.matrix_buffer.set_data(
                gl::BufferTarget::Array,
                &model.dyn_info,
                gl::BufferUsage::Stream,
            );
            model.max_count = model.dyn_info.len();
        } else {
            model
                .matrix_buffer
                .set_data_range(gl::BufferTarget::Array, &model.dyn_info, 0);
        }
    }

    fn render(
        &mut self,
        ctx: &mut pipeline::Context<'_>,
        matrix: super::EntityMatrix<'_>,
        flags: PassFlag,
    ) {
        for is_water in &[false, true] {
            let program = if *is_water {
                ctx.program("static_water")
            } else {
                ctx.program("static")
            };
            program.use_program();
            program
                .uniform("projection_matrix")
                .map(|v| v.set_matrix4(matrix.projection));
            program
                .uniform("view_matrix")
                .map(|v| v.set_matrix4(matrix.view_matrix));
            program
                .uniform("shadow_matrix")
                .map(|v| v.set_matrix4(assume!(self.log, matrix.shadow_view_matrix)));
            program
                .uniform("shadow_projection")
                .map(|v| v.set_matrix4(assume!(self.log, matrix.shadow_projection)));
            program
                .uniform("shadow_map")
                .map(|v| v.set_int(super::SHADOW_MAP_LOCATION as i32));
            program
                .uniform("u_textures")
                .map(|v| v.set_int(super::GLOBAL_TEXTURE_LOCATION as i32));
            program.uniform("water_normal").map(|v| {
                gl::active_texture(6);
                self.water_normal.bind(gl::TextureTarget::Texture2D);
                v.set_int(6);
            });
            for model in self.models.values() {
                if flags.contains(PassFlag::HIGHLIGHTS) && !model.has_highlights {
                    continue;
                }
                if !model.dyn_info.is_empty() {
                    if model.use_water != *is_water {
                        continue;
                    }
                    program
                        .uniform("u_time")
                        .map(|v| v.set_float(model.time as f32));
                    program
                        .uniform("u_atlas")
                        .map(|v| v.set_int(model.texture.0));
                    let rect = model.texture.1;
                    program
                        .uniform("u_texture_info")
                        .map(|v| v.set_int4(rect.x, rect.y, rect.width, rect.height));

                    model.array.bind();
                    gl::draw_elements_instanced(
                        gl::DrawType::Triangles,
                        model.info.faces.len() * 3,
                        model.index_ty,
                        model.dyn_info.len(),
                    );
                }
            }
        }
    }
}
