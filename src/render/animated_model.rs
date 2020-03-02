use cgmath;

use super::{atlas, exmodel, gl, image, pipeline, ModelKey, ModelKeyBorrow, PassFlag};
use crate::ecs;
use crate::entity;
use crate::prelude::*;
use crate::server::assets;
use crate::server::common::AnimationSet;
use crate::util::FNVMap;

pub struct Info {
    log: Logger,
    pub info: InfoTick,
    pub(super) gl_models: FNVMap<ModelKey, GLModel>,

    attributes: Attributes,

    standard_tint: gl::Texture,
}

pub struct InfoTick {
    pub(crate) models: FNVMap<ResourceKey<'static>, Model>,
    pub(crate) animations: FNVMap<assets::ResourceKey<'static>, exmodel::Animation>,
}
component!(InfoTick => mut World);

struct Attributes {
    position: gl::Attribute,
    normal: gl::Attribute,
    uv: gl::Attribute,
    bones: gl::Attribute,
    bone_weights: gl::Attribute,
    matrix: gl::Attribute,
    tint: gl::Attribute,
    highlight: gl::Attribute,
    bone_offset: gl::Attribute,
    tint_offset: gl::Attribute,
}

pub(super) struct GLModel {
    texture: (i32, atlas::Rect),
    tint_texture: Option<gl::Texture>,
    tint_count: usize,

    array: gl::VertexArray,
    _buffer: gl::Buffer,
    _index_buffer: gl::Buffer,
    index_ty: gl::Type,
    matrix_buffer: gl::Buffer,

    bone_matrix_buffer: gl::Buffer,
    bone_matrix_texture: gl::Texture,

    tint_col_buffer: gl::Buffer,
    tint_col_texture: gl::Texture,

    max_count: usize,
    pub(super) entity_map: FNVMap<ecs::Entity, usize>,
    pub(super) dyn_info: Vec<DynInfo>,
    pub(super) bone_info: Vec<cgmath::Matrix4<f32>>,
    // Used for attachments
    // TODO: Waste of memory in a lot of cases
    pub(super) bone_node_info: Vec<cgmath::Matrix4<f32>>,
    tint_info: Vec<(u8, u8, u8, u8)>,

    has_highlights: bool,
}

pub(crate) struct Model {
    pub(crate) info: exmodel::AniModel,

    pub(crate) bones: FNVMap<String, usize>,
    bone_copy_map: Vec<usize>,

    pub(crate) animations: FNVMap<assets::ResourceKey<'static>, Animation>,
    pub(crate) loaded_sets: FNVMap<usize, AnimationSet>,
    pub(crate) root_node: AniNode,
}

#[repr(C)]
#[derive(Clone)]
pub(super) struct DynInfo {
    pub(super) matrix: cgmath::Matrix4<f32>,
    tint: (u8, u8, u8, u8),
    highlight: (u8, u8, u8, u8),
    pub(super) bone_offset: i32,
    tint_offset: i32,
}

impl Info {
    pub(super) fn new(
        log: &Logger,
        asset_manager: &AssetManager,
        ctx: &mut pipeline::Context<'_>,
    ) -> Info {
        let (
            attrib_position,
            attrib_normal,
            attrib_uv,
            attrib_bones,
            attrib_bone_weights,
            attrib_matrix,
            attrib_tint,
            attrib_highlight,
            attrib_bone_offset,
            attrib_tint_offset,
        ) = {
            let program = ctx.program("animated");
            program.use_program();
            (
                program
                    .attribute("attrib_position")
                    .expect("Missing `attrib_position`"),
                program
                    .attribute("attrib_normal")
                    .expect("Missing `attrib_normal`"),
                program.attribute("attrib_uv").expect("Missing `attrib_uv`"),
                program
                    .attribute("attrib_bones")
                    .expect("Missing `attrib_bones`"),
                program
                    .attribute("attrib_bone_weights")
                    .expect("Missing `attrib_bone_weights`"),
                program
                    .attribute("attrib_matrix")
                    .expect("Missing `attrib_matrix`"),
                program
                    .attribute("attrib_tint")
                    .expect("Missing `attrib_tint`"),
                program
                    .attribute("attrib_highlight")
                    .expect("Missing `attrib_highlight`"),
                program
                    .attribute("attrib_bone_offset")
                    .expect("Missing `attrib_bone_offset`"),
                program
                    .attribute("attrib_tint_offset")
                    .expect("Missing `attrib_tint_offset`"),
            )
        };
        let log = log.new(o!("source" => "animated_model"));

        let img = assume!(
            log,
            asset_manager.loader_open::<image::Loader>(ResourceKey::new("base", "no_tint"))
        );
        let tex_data = img.wait_take_image().expect("Missing default tint texture");

        let texture = gl::Texture::new();
        texture.bind(gl::TextureTarget::Texture2D);
        texture.image_2d_ex(
            gl::TextureTarget::Texture2D,
            0,
            tex_data.width,
            tex_data.height,
            gl::TextureFormat::R8,
            gl::TextureFormat::Red,
            gl::Type::UnsignedByte,
            Some(
                &tex_data
                    .data
                    .chunks_exact(4)
                    .map(|v| v[0])
                    .collect::<Vec<_>>(),
            ),
        );
        texture.set_parameter::<gl::TextureMinFilter>(
            gl::TextureTarget::Texture2D,
            gl::TextureFilter::Nearest,
        );
        texture.set_parameter::<gl::TextureMagFilter>(
            gl::TextureTarget::Texture2D,
            gl::TextureFilter::Nearest,
        );
        texture.set_parameter::<gl::TextureWrapS>(
            gl::TextureTarget::Texture2D,
            gl::TextureWrap::ClampToEdge,
        );
        texture.set_parameter::<gl::TextureWrapT>(
            gl::TextureTarget::Texture2D,
            gl::TextureWrap::ClampToEdge,
        );
        texture.set_parameter::<gl::TextureBaseLevel>(gl::TextureTarget::Texture2D, 0);
        texture.set_parameter::<gl::TextureMaxLevel>(gl::TextureTarget::Texture2D, 0);

        Info {
            log,
            info: InfoTick {
                models: FNVMap::default(),
                animations: FNVMap::default(),
            },
            gl_models: FNVMap::default(),
            attributes: Attributes {
                position: attrib_position,
                normal: attrib_normal,
                uv: attrib_uv,
                bones: attrib_bones,
                bone_weights: attrib_bone_weights,
                matrix: attrib_matrix,
                tint: attrib_tint,
                highlight: attrib_highlight,
                bone_offset: attrib_bone_offset,
                tint_offset: attrib_tint_offset,
            },

            standard_tint: texture,
        }
    }
}

impl<'a> super::EntityRender<'a> for Info {
    type Component = entity::AnimatedModel;
    type Params = (
        ecs::Read<'a, Rotation>,
        ecs::Read<'a, InvalidPlacement>,
        ecs::Read<'a, entity::Color>,
        ecs::Read<'a, Tints>,
        ecs::Read<'a, entity::Highlighted>,
    );

    fn clear(&mut self) {
        for gl_model in self.gl_models.values_mut() {
            gl_model.entity_map.clear();
            gl_model.dyn_info.clear();
            gl_model.bone_info.clear();
            gl_model.bone_node_info.clear();
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
        _model_a: &Read<entity::Model>,
        _model_tex: &Read<entity::ModelTexture>,
        component: &Read<Self::Component>,
        (rot, invalid, color, tints, highlight): &Self::Params,
        model_key: assets::ResourceKey<'_>,
        texture: Option<assets::ResourceKey<'_>>,
        ents: &[ecs::Entity],
        _delta: f64,
    ) {
        use cgmath::prelude::*;
        use rayon::prelude::*;
        use std::collections::hash_map::Entry;

        let key = ModelKey {
            key: ModelKeyBorrow(
                model_key.into_owned(),
                texture.as_ref().map(|v| v.borrow().into_owned()),
            ),
        };

        let model = match self.info.models.entry(key.0.clone()) {
            Entry::Occupied(val) => val.into_mut(),
            Entry::Vacant(val) => init_model(&self.log, asset_manager, val),
        };
        let gl_model = match self.gl_models.entry(key) {
            Entry::Occupied(val) => val.into_mut(),
            Entry::Vacant(val) => init_gl_model(
                &self.log,
                asset_manager,
                global_atlas,
                &self.attributes,
                model,
                val,
            ),
        };

        gl_model.array.bind();
        gl_model.dyn_info.clear();
        gl_model.bone_info.clear();

        for (idx, e) in ents.iter().enumerate() {
            gl_model.entity_map.insert(*e, idx);
        }

        let selection_invalid = config.placement_invalid_colour.get();

        let root_inv = model
            .info
            .root_node
            .transform
            .invert()
            .expect("Failed to invert root transform");
        let identity: cgmath::Matrix4<f32> = cgmath::Matrix4::identity();

        gl_model.bone_info.resize(
            ents.len() * (1 + model.bone_copy_map.len()),
            model.info.transform,
        );
        gl_model
            .tint_info
            .resize(ents.len() * gl_model.tint_count, (255, 255, 255, 255));
        gl_model
            .bone_node_info
            .resize(ents.len() * model.bones.len(), model.info.transform);
        gl_model.dyn_info.resize(
            ents.len(),
            DynInfo {
                matrix: identity,
                tint: (0, 0, 0, 0),
                highlight: (0, 0, 0, 0),
                bone_offset: 0,
                tint_offset: 0,
            },
        );

        let minfo = &model.info;
        let animations = &model.animations;
        let root_node = &model.root_node;
        let tint_count = gl_model.tint_count;
        let bone_node_count = model.bones.len();
        let bone_copy_map = &model.bone_copy_map;

        let has_highlights = ents
            .par_iter()
            .zip(
                gl_model
                    .bone_info
                    .par_chunks_mut(1 + model.info.bones.len()),
            )
            .zip(gl_model.bone_node_info.par_chunks_mut(bone_node_count))
            .zip(gl_model.tint_info.par_chunks_mut(gl_model.tint_count))
            .zip(gl_model.dyn_info.par_iter_mut())
            .enumerate()
            .with_min_len(4)
            .map(|(idx, ((((e, bones), bone_info), tint_i), r#dyn))| {
                let pos = pos.get_component(*e).expect("Missing component (Position)");
                let rot = rot.get_component(*e).expect("Missing component (Rotation)");
                let mat = cgmath::Decomposed {
                    scale: 1.0 / 10.0,
                    rot: cgmath::Quaternion::from_angle_y(cgmath::Rad(rot.rotation.raw())),
                    disp: cgmath::Vector3::new(pos.x, pos.y, pos.z),
                };
                let is_invalid = invalid.get_component(*e).is_some();
                let tint = if is_invalid {
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

                let bone_offset = (1 + minfo.bones.len()) * idx;
                let tint_offset = tint_count * idx;

                let ainfo = component
                    .get_component(*e)
                    .expect("Missing component (Model)");

                for tint in &mut tint_i[..] {
                    *tint = (255, 255, 255, 255);
                }
                if let Some(tint_info) = tints.get_component(*e) {
                    for (tint, t) in tint_i[1..].iter_mut().zip(tint_info.tints.iter()) {
                        *tint = *t;
                    }
                }

                if let Some(animation) = ainfo.animation_set.get(
                    ainfo
                        .animation_queue
                        .first()
                        .expect("Entity missing animation in queue"),
                ) {
                    let mut time = ainfo.time;
                    let ani = {
                        let mut ani = None;
                        let len = animation.animations.len();
                        for (idx, a) in animation.animations.iter().enumerate() {
                            let a = match animations.get(a) {
                                Some(v) => v,
                                None => continue,
                            };
                            if time < a.duration || idx == len - 1 {
                                ani = Some(a);
                                time = time.min(a.duration);
                                break;
                            }
                            time -= a.duration;
                        }
                        ani
                    };
                    if let Some(ani) = ani {
                        compute_nodes(ani, root_node, time as f32, bone_info, root_inv);

                        for ((bone, copy), bones) in
                            minfo.bones.iter().zip(bone_copy_map).zip(&mut bones[1..])
                        {
                            *bones = bone_info[*copy] * bone.offset;
                        }
                    }
                }

                let mut hightlighted = false;
                let highlight = if let Some(h) = highlight.get_component(*e) {
                    hightlighted = true;
                    (h.color.0, h.color.1, h.color.2, 255)
                } else {
                    (0, 0, 0, 0)
                };

                let mat: cgmath::Matrix4<f32> = mat.into();
                *r#dyn = DynInfo {
                    matrix: mat,
                    tint,
                    highlight,
                    bone_offset: bone_offset as i32,
                    tint_offset: tint_offset as i32,
                };
                hightlighted
            })
            .reduce(|| false, |a, b| a | b);
        gl_model.has_highlights = has_highlights;

        gl_model.dyn_info.truncate(ents.len());
        gl_model
            .bone_info
            .truncate(ents.len() * (1 + model.info.bones.len()));

        gl_model.matrix_buffer.bind(gl::BufferTarget::Array);
        if gl_model.max_count < gl_model.dyn_info.len() {
            gl_model.matrix_buffer.set_data(
                gl::BufferTarget::Array,
                &gl_model.dyn_info,
                gl::BufferUsage::Stream,
            );
            gl_model.bone_matrix_buffer.bind(gl::BufferTarget::Texture);
            gl_model.bone_matrix_buffer.set_data(
                gl::BufferTarget::Texture,
                &gl_model.bone_info,
                gl::BufferUsage::Stream,
            );
            gl_model.tint_col_buffer.bind(gl::BufferTarget::Texture);
            gl_model.tint_col_buffer.set_data(
                gl::BufferTarget::Texture,
                &gl_model.tint_info,
                gl::BufferUsage::Stream,
            );
            gl_model.max_count = gl_model.dyn_info.len();
        } else {
            gl_model
                .matrix_buffer
                .set_data_range(gl::BufferTarget::Array, &gl_model.dyn_info, 0);
            gl_model.bone_matrix_buffer.bind(gl::BufferTarget::Texture);
            gl_model.bone_matrix_buffer.set_data_range(
                gl::BufferTarget::Texture,
                &gl_model.bone_info,
                0,
            );
            gl_model.tint_col_buffer.bind(gl::BufferTarget::Texture);
            gl_model.tint_col_buffer.set_data_range(
                gl::BufferTarget::Texture,
                &gl_model.tint_info,
                0,
            );
        }
    }

    fn render(
        &mut self,
        ctx: &mut pipeline::Context<'_>,
        matrix: super::EntityMatrix<'_>,
        flags: PassFlag,
    ) {
        let program = ctx.program("animated");
        program.use_program();
        program
            .uniform("projection_matrix")
            .map(|v| v.set_matrix4(matrix.projection));
        program
            .uniform("view_matrix")
            .map(|v| v.set_matrix4(matrix.view_matrix));
        program.uniform("shadow_matrix").map(|v| {
            v.set_matrix4(
                matrix
                    .shadow_view_matrix
                    .expect("Missing shadow view matrix"),
            )
        });
        program
            .uniform("shadow_projection")
            .map(|v| v.set_matrix4(assume!(self.log, matrix.shadow_projection)));
        program
            .uniform("shadow_map")
            .map(|v| v.set_int(super::SHADOW_MAP_LOCATION as i32));
        program.uniform("u_bone_matrices").map(|v| v.set_int(0));
        program.uniform("u_tints").map(|v| v.set_int(1));
        program.uniform("u_tint_info").map(|v| v.set_int(2));
        program
            .uniform("u_textures")
            .map(|v| v.set_int(super::GLOBAL_TEXTURE_LOCATION as i32));

        for (key, gl_model) in &mut self.gl_models {
            let model = assume!(self.log, self.info.models.get(&key.0));

            if flags.contains(PassFlag::HIGHLIGHTS) && !gl_model.has_highlights {
                continue;
            }

            if !gl_model.dyn_info.is_empty() {
                gl_model.array.bind();
                gl::active_texture(0);
                gl_model
                    .bone_matrix_texture
                    .bind(gl::TextureTarget::TextureBuffer);

                gl::active_texture(1);
                gl_model
                    .tint_col_texture
                    .bind(gl::TextureTarget::TextureBuffer);

                gl::active_texture(2);
                gl_model
                    .tint_texture
                    .as_ref()
                    .unwrap_or(&self.standard_tint)
                    .bind(gl::TextureTarget::Texture2D);

                program
                    .uniform("u_atlas")
                    .map(|v| v.set_int(gl_model.texture.0));
                let rect = gl_model.texture.1;
                program
                    .uniform("u_texture_info")
                    .map(|v| v.set_int4(rect.x, rect.y, rect.width, rect.height));

                gl::draw_elements_instanced(
                    gl::DrawType::Triangles,
                    model.info.faces.len() * 3,
                    gl_model.index_ty,
                    gl_model.dyn_info.len(),
                );
            }
        }
    }
}

use std::collections::hash_map::VacantEntry;

pub(crate) fn init_model<'a>(
    log: &Logger,
    asset_manager: &assets::AssetManager,
    val: VacantEntry<'a, ResourceKey<'static>, Model>,
) -> &'a mut Model {
    let mut file = assume!(
        log,
        asset_manager.open_from_pack(
            val.key().module_key(),
            &format!("models/{}.uamod", val.key().resource())
        )
    );
    let minfo = assume!(log, exmodel::AniModel::read_from(&mut file));

    let mut bones = FNVMap::default();

    let root_node = AniNode::convert(&mut bones, &minfo.root_node);

    let mut bone_copy_map = Vec::with_capacity(minfo.bones.len());
    for bone in &minfo.bones {
        let id = bones.get(&bone.name).cloned().unwrap_or(0);
        bone_copy_map.push(id);
    }

    val.insert(Model {
        info: minfo,
        bones,
        bone_copy_map,
        animations: Default::default(),
        loaded_sets: Default::default(),

        root_node,
    })
}

fn init_gl_model<'a>(
    log: &Logger,
    asset_manager: &assets::AssetManager,
    global_atlas: &mut super::GlobalAtlas,
    attributes: &Attributes,
    model: &Model,
    val: VacantEntry<'a, ModelKey, GLModel>,
) -> &'a mut GLModel {
    use std::mem;

    let minfo = &model.info;

    let array = gl::VertexArray::new();
    array.bind();

    let model_buffer = gl::Buffer::new();
    model_buffer.bind(gl::BufferTarget::Array);

    attributes.position.enable();
    attributes.position.vertex_pointer(
        3,
        gl::Type::Float,
        false,
        mem::size_of::<exmodel::AniVertex>() as i32,
        0,
    );
    attributes.normal.enable();
    attributes.normal.vertex_pointer(
        3,
        gl::Type::Float,
        false,
        mem::size_of::<exmodel::AniVertex>() as i32,
        12,
    );
    attributes.uv.enable();
    attributes.uv.vertex_pointer(
        2,
        gl::Type::Float,
        false,
        mem::size_of::<exmodel::AniVertex>() as i32,
        24,
    );
    attributes.bones.enable();
    attributes.bones.vertex_int_pointer(
        4,
        gl::Type::UnsignedByte,
        mem::size_of::<exmodel::AniVertex>() as i32,
        32,
    );
    attributes.bone_weights.enable();
    attributes.bone_weights.vertex_pointer(
        4,
        gl::Type::Float,
        false,
        mem::size_of::<exmodel::AniVertex>() as i32,
        36,
    );

    model_buffer.set_data(
        gl::BufferTarget::Array,
        &minfo.verts,
        gl::BufferUsage::Static,
    );

    let model_index_buffer = gl::Buffer::new();
    model_index_buffer.bind(gl::BufferTarget::ElementArray);
    let index_ty = if minfo.verts.len() <= 0xFF {
        set_indices_packed::<_, u8>(minfo.faces.iter(), &model_index_buffer);
        gl::Type::UnsignedByte
    } else if minfo.verts.len() <= 0xFFFF {
        set_indices_packed::<_, u16>(minfo.faces.iter(), &model_index_buffer);
        gl::Type::UnsignedShort
    } else {
        set_indices_packed::<_, u32>(minfo.faces.iter(), &model_index_buffer);
        gl::Type::UnsignedInt
    };

    let matrix_buffer = gl::Buffer::new();
    matrix_buffer.bind(gl::BufferTarget::Array);

    for i in 0..4 {
        let attrib = attributes.matrix.offset(i);
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
    attributes.tint.enable();
    attributes.tint.vertex_pointer(
        4,
        gl::Type::UnsignedByte,
        true,
        mem::size_of::<DynInfo>() as i32,
        4 * 4 * 4,
    );
    attributes.tint.divisor(1);
    attributes.highlight.enable();
    attributes.highlight.vertex_pointer(
        4,
        gl::Type::UnsignedByte,
        true,
        mem::size_of::<DynInfo>() as i32,
        4 * 4 * 4 + 4,
    );
    attributes.highlight.divisor(1);
    attributes.bone_offset.enable();
    attributes.bone_offset.vertex_int_pointer(
        1,
        gl::Type::Int,
        mem::size_of::<DynInfo>() as i32,
        4 * 4 * 4 + 4 + 4,
    );
    attributes.bone_offset.divisor(1);
    attributes.tint_offset.enable();
    attributes.tint_offset.vertex_int_pointer(
        1,
        gl::Type::Int,
        mem::size_of::<DynInfo>() as i32,
        4 * 4 * 4 + 4 + 4 + 4,
    );
    attributes.tint_offset.divisor(1);

    let texture = {
        let texture = val.key().1.as_ref().map(|v| v.borrow());
        let tex = texture.unwrap_or_else(|| {
            assets::LazyResourceKey::parse(&minfo.texture).or_module(val.key().0.module_key())
        });
        super::RenderState::texture_info_for(log, asset_manager, global_atlas, tex)
    };

    let tint_count;
    let tint_texture;
    {
        let key = format!("{}_tint", minfo.texture);
        let tex = assets::LazyResourceKey::parse(&key).or_module(val.key().0.module_key());
        let img = assume!(
            log,
            asset_manager.loader_open::<image::Loader>(tex.borrow())
        );
        if let Ok(tint_tex) = img.wait_take_image() {
            let mut highest = 0;
            for c in tint_tex.data.chunks_exact(4) {
                if c[0] > highest {
                    highest = c[0];
                }
            }
            tint_count = highest as usize + 1;

            let texture = gl::Texture::new();
            texture.bind(gl::TextureTarget::Texture2D);
            texture.image_2d_ex(
                gl::TextureTarget::Texture2D,
                0,
                tint_tex.width,
                tint_tex.height,
                gl::TextureFormat::R8,
                gl::TextureFormat::Red,
                gl::Type::UnsignedByte,
                Some(
                    &tint_tex
                        .data
                        .chunks_exact(4)
                        .map(|v| v[0])
                        .collect::<Vec<_>>(),
                ),
            );
            texture.set_parameter::<gl::TextureMinFilter>(
                gl::TextureTarget::Texture2D,
                gl::TextureFilter::Nearest,
            );
            texture.set_parameter::<gl::TextureMagFilter>(
                gl::TextureTarget::Texture2D,
                gl::TextureFilter::Nearest,
            );
            texture.set_parameter::<gl::TextureWrapS>(
                gl::TextureTarget::Texture2D,
                gl::TextureWrap::ClampToEdge,
            );
            texture.set_parameter::<gl::TextureWrapT>(
                gl::TextureTarget::Texture2D,
                gl::TextureWrap::ClampToEdge,
            );
            texture.set_parameter::<gl::TextureBaseLevel>(gl::TextureTarget::Texture2D, 0);
            texture.set_parameter::<gl::TextureMaxLevel>(gl::TextureTarget::Texture2D, 0);

            tint_texture = Some(texture);
        } else {
            tint_count = 1;
            tint_texture = None;
        }
    };

    let bone_matrix_buffer = gl::Buffer::new();
    bone_matrix_buffer.bind(gl::BufferTarget::Texture);
    let bone_matrix_texture = gl::Texture::new();
    bone_matrix_texture.bind(gl::TextureTarget::TextureBuffer);
    bone_matrix_texture.buffer(
        gl::TextureTarget::TextureBuffer,
        &bone_matrix_buffer,
        gl::TextureFormat::Rgba32F,
    );

    let tint_col_buffer = gl::Buffer::new();
    tint_col_buffer.bind(gl::BufferTarget::Texture);
    let tint_col_texture = gl::Texture::new();
    tint_col_texture.bind(gl::TextureTarget::TextureBuffer);
    tint_col_texture.buffer(
        gl::TextureTarget::TextureBuffer,
        &tint_col_buffer,
        gl::TextureFormat::Rgba8,
    );

    val.insert(GLModel {
        texture,
        tint_texture,
        tint_count,

        array,
        _buffer: model_buffer,
        _index_buffer: model_index_buffer,
        index_ty,
        matrix_buffer,
        max_count: 0,

        entity_map: FNVMap::default(),
        dyn_info: vec![],

        bone_matrix_buffer,
        bone_matrix_texture,
        tint_col_buffer,
        tint_col_texture,

        bone_info: vec![],
        tint_info: vec![],
        bone_node_info: vec![],
        has_highlights: false,
    })
}

pub(crate) struct Animation {
    pub(crate) duration: f64,
    pub(crate) channels: Vec<AnimationDetails>,
}

pub(crate) struct AnimationDetails {
    pub(crate) position: ChannelData<cgmath::Vector3<f32>>,
    pub(crate) rotation: ChannelData<cgmath::Quaternion<f32>>,
    pub(crate) scale: ChannelData<cgmath::Vector3<f32>>,
}

impl AnimationDetails {
    pub(crate) fn empty() -> AnimationDetails {
        use cgmath::prelude::*;
        AnimationDetails {
            position: ChannelData::empty(cgmath::Vector3::zero()),
            rotation: ChannelData::empty(cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0)),
            scale: ChannelData::empty(cgmath::Vector3::new(1.0, 1.0, 1.0)),
        }
    }
    pub(crate) fn new(
        position: Vec<(f64, cgmath::Vector3<f32>)>,
        rotation: Vec<(f64, cgmath::Quaternion<f32>)>,
        scale: Vec<(f64, cgmath::Vector3<f32>)>,
    ) -> AnimationDetails {
        use cgmath::prelude::*;
        AnimationDetails {
            position: ChannelData::new(cgmath::Vector3::zero(), position),
            rotation: ChannelData::new(cgmath::Quaternion::new(1.0, 0.0, 0.0, 0.0), rotation),
            scale: ChannelData::new(cgmath::Vector3::new(1.0, 1.0, 1.0), scale),
        }
    }
}

pub(crate) struct AniNode {
    pub(crate) nodes: Vec<AniSubNode>,
}

pub(crate) struct AniSubNode {
    parent: Option<usize>,
    id: usize,
    pub(crate) transform: cgmath::Matrix4<f32>,
}

impl AniNode {
    pub(crate) fn convert(map: &mut FNVMap<String, usize>, info: &exmodel::AniNode) -> AniNode {
        let mut nodes = vec![];
        Self::collect(&mut nodes, map, info, None);
        AniNode { nodes }
    }

    fn collect(
        output: &mut Vec<AniSubNode>,
        map: &mut FNVMap<String, usize>,
        info: &exmodel::AniNode,
        parent: Option<usize>,
    ) {
        let len = map.len();
        let id = *map.entry(info.name.clone()).or_insert(len);
        output.push(AniSubNode {
            parent,
            id,
            transform: info.transform,
        });
        for node in &info.child_nodes {
            Self::collect(output, map, node, Some(id));
        }
    }
}

fn compute_nodes(
    ani: &Animation,
    nodes: &AniNode,
    time: f32,
    matrices: &mut [cgmath::Matrix4<f32>],
    root_reverse_transform: cgmath::Matrix4<f32>,
) {
    use cgmath::prelude::*;

    for node in &nodes.nodes {
        let transform = if let Some(ani) = ani.channels.get(node.id) {
            let position = ani.position.get(time);
            let rotation: cgmath::Matrix4<f32> = ani.rotation.get(time).into();
            let scale = ani.scale.get(time);

            cgmath::Matrix4::from_translation(position)
                * rotation
                * cgmath::Matrix4::from_nonuniform_scale(scale.x, scale.y, scale.z)
        } else {
            node.transform
        };

        let parent = node
            .parent
            .and_then(|v| matrices.get(v))
            .cloned()
            .unwrap_or_else(cgmath::Matrix4::identity);
        let result_transform = parent * transform;

        if node.id < matrices.len() {
            matrices[node.id] = result_transform;
        }
    }
    for m in matrices {
        *m = *m * root_reverse_transform;
    }
}

pub(super) trait FromU32 {
    fn from(v: u32) -> Self;
}
impl FromU32 for u8 {
    fn from(v: u32) -> Self {
        v as Self
    }
}
impl FromU32 for u16 {
    fn from(v: u32) -> Self {
        v as Self
    }
}
impl FromU32 for u32 {
    fn from(v: u32) -> Self {
        v
    }
}

pub(super) fn set_indices_packed<'a, I, T>(faces: I, buffer: &gl::Buffer)
where
    T: FromU32,
    I: Iterator<Item = &'a exmodel::Face>,
{
    let indices: Vec<T> = faces
        .flat_map(|v| &v.indices)
        .map(|v| T::from(*v))
        .collect();
    buffer.set_data(
        gl::BufferTarget::ElementArray,
        &indices,
        gl::BufferUsage::Static,
    );
}

pub(crate) struct ChannelData<T> {
    pub(crate) buckets: SmallVec<[T; 4]>,
    scale: f32,
    default: T,
}

impl<T> ChannelData<T>
where
    T: Copy + Lerpable,
{
    // TODO: This is invalid currently
    fn empty(def: T) -> ChannelData<T> {
        ChannelData {
            buckets: SmallVec::new(),
            scale: 1.0,
            default: def,
        }
    }
    fn new(def: T, data: Vec<(f64, T)>) -> ChannelData<T> {
        use std::f64;
        debug_assert!(data.len() >= 2); // TODO: might not always be true?

        let mut smallest_gap = f64::INFINITY;
        let mut length = 0.0;
        for v in data.windows(2) {
            smallest_gap = smallest_gap.min(v[1].0 - v[0].0);
            length = v[1].0;
        }
        let steps = (length / smallest_gap).ceil() as usize;
        let mut buckets = SmallVec::with_capacity(steps);

        for offset in 0..=steps {
            let time = offset as f64 * smallest_gap;
            let part = if let Some(parts) = data.windows(2).find(|v| time <= v[1].0) {
                let rel_time = time - parts[0].0;
                let delta = rel_time / (parts[1].0 - parts[0].0);
                debug_assert!(delta >= 0.0 && delta <= 1.0);
                parts[0].1.lerp(parts[1].1, delta as f32)
            } else if let Some(part) = data.last() {
                part.1
            } else {
                unreachable!()
            };
            buckets.push(part);
        }

        ChannelData {
            buckets,
            scale: smallest_gap as f32,
            default: def,
        }
    }

    #[inline]
    fn get(&self, time: f32) -> T {
        let key = time / self.scale;
        let fract = key.fract();
        let key = key as usize;

        if let Some(val) = self.buckets.get(key) {
            if let Some(other) = self.buckets.get(key + 1) {
                val.lerp(*other, fract)
            } else {
                *val
            }
        } else {
            self.default
        }
    }
}

pub(crate) trait Lerpable: Sized {
    fn lerp(self, other: Self, t: f32) -> Self;
}

impl Lerpable for cgmath::Vector3<f32> {
    #[inline]
    fn lerp(self, other: Self, t: f32) -> Self {
        use cgmath::prelude::*;
        VectorSpace::lerp(self, other, t)
    }
}

impl Lerpable for cgmath::Quaternion<f32> {
    #[inline]
    fn lerp(self, other: Self, t: f32) -> Self {
        self.slerp(other, t)
    }
}

pub(crate) fn decompose(
    m: &cgmath::Matrix4<f32>,
) -> (
    cgmath::Vector3<f32>,
    cgmath::Quaternion<f32>,
    cgmath::Vector3<f32>,
) {
    use cgmath::{InnerSpace, SquareMatrix};
    let pos = cgmath::Vector3::new(m[3][0], m[3][1], m[3][2]);

    let mut cols = [m[0].truncate(), m[1].truncate(), m[2].truncate()];

    let mut scaling = cgmath::Vector3::new(
        cols[0].magnitude(),
        cols[1].magnitude(),
        cols[2].magnitude(),
    );

    if m.determinant() < 0.0 {
        scaling = -scaling;
    }

    if scaling.x != 0.0 {
        cols[0] /= scaling.x;
    }
    if scaling.y != 0.0 {
        cols[1] /= scaling.y;
    }
    if scaling.z != 0.0 {
        cols[2] /= scaling.z;
    }

    let rot = cgmath::Matrix3::from_cols(cols[0], cols[1], cols[2]);

    (scaling, rot.into(), pos)
}
