use super::*;

use std::mem;

pub struct Icons {
    array: gl::VertexArray,
    _buffer: gl::Buffer,
    dynamic_buffer: gl::Buffer,
}

#[repr(C)]
struct UniqueData {
    x: f32,
    y: f32,
    z: f32,
    size_x: u16,
    size_y: u16,
    texture_x: u16,
    texture_y: u16,
    texture_w: u16,
    texture_h: u16,
    atlas: u16,
    _padding: u16,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

#[repr(C)]
struct Vertex {
    x: f32,
    y: f32,
    tx: f32,
    ty: f32,
}

impl Icons {
    pub fn new(log: &Logger, ctx: &mut pipeline::Context<'_>) -> Icons {
        let log = log.new(o!("source" => "icons"));
        let (
            attrib_position, attrib_color, attrib_vert, attrib_uv, attrib_size,
            attrib_texture_info, attrib_atlas,
        ) = {
            let program = ctx.program("icon");
            program.use_program();
            (
                assume!(log, program.attribute("attrib_position")),
                assume!(log, program.attribute("attrib_color")),
                assume!(log, program.attribute("attrib_vert")),
                assume!(log, program.attribute("attrib_uv")),
                assume!(log, program.attribute("attrib_size")),
                assume!(log, program.attribute("attrib_texture_info")),
                assume!(log, program.attribute("attrib_atlas")),
            )
        };

        let array = gl::VertexArray::new();
        array.bind();

        let model_buffer = gl::Buffer::new();
        model_buffer.bind(gl::BufferTarget::Array);

        attrib_vert.enable();
        attrib_vert.vertex_pointer(2, gl::Type::Float, false, mem::size_of::<Vertex>() as i32, 0);
        attrib_uv.enable();
        attrib_uv.vertex_pointer(2, gl::Type::Float, false, mem::size_of::<Vertex>() as i32, 8);

        let verts = &[
            Vertex { x: -0.5, y: 0.5, tx: 0.0, ty: 0.0},
            Vertex { x: 0.5, y: 0.5, tx: 1.0, ty: 0.0},
            Vertex { x: -0.5, y: -0.5, tx: 0.0, ty: 1.0},

            Vertex { x: 0.5, y: -0.5, tx: 1.0, ty: 1.0},
            Vertex { x: -0.5, y: -0.5, tx: 0.0, ty: 1.0},
            Vertex { x: 0.5, y: 0.5, tx: 1.0, ty: 0.0},
        ];

        model_buffer.set_data(gl::BufferTarget::Array, verts, gl::BufferUsage::Static);

        let dynamic_buffer = gl::Buffer::new();
        dynamic_buffer.bind(gl::BufferTarget::Array);

        attrib_position.enable();
        attrib_position.vertex_pointer(3, gl::Type::Float, false, mem::size_of::<UniqueData>() as i32, 0);
        attrib_position.divisor(1);
        attrib_size.enable();
        attrib_size.vertex_pointer(2, gl::Type::UnsignedShort, false, mem::size_of::<UniqueData>() as i32, 12);
        attrib_size.divisor(1);
        attrib_texture_info.enable();
        attrib_texture_info.vertex_pointer(4, gl::Type::UnsignedShort, false, mem::size_of::<UniqueData>() as i32, 16);
        attrib_texture_info.divisor(1);
        attrib_atlas.enable();
        attrib_atlas.vertex_pointer(1, gl::Type::UnsignedShort, false, mem::size_of::<UniqueData>() as i32, 24);
        attrib_atlas.divisor(1);
        attrib_color.enable();
        attrib_color.vertex_pointer(4, gl::Type::UnsignedByte, true, mem::size_of::<UniqueData>() as i32, 28);
        attrib_color.divisor(1);

        Icons {
            array,
            _buffer: model_buffer,
            dynamic_buffer,
        }
    }
}

impl RenderState {
    pub(super) fn draw_icons(
            &mut self,
            ctx: &mut pipeline::Context<'_>,
            projection: &cgmath::Matrix4<f32>,
            view_matrix: &cgmath::Matrix4<f32>,
            entities: &mut ecs::Container,
    ) {
        use crate::entity::{Icon, Color};

        entities.with(|
            em: ecs::EntityManager<'_>,
            pos: ecs::Read<Position>,
            icon: ecs::Read<Icon>,
            color: ecs::Read<Color>,
        | {
            let data: Vec<_> = em.group((&icon, &pos))
                .map(|(e, (icon, pos))| {
                    let color = color.get_component(e)
                            .map_or((255, 255, 255, 255), |v| v.color);

                    let (atlas, rect) = super::RenderState::texture_info_for(
                        &self.log,
                        &self.asset_manager,
                        &mut self.global_atlas,
                        icon.texture.borrow(),
                    );

                    UniqueData {
                        x: pos.x,
                        y: pos.y,
                        z: pos.z,
                        size_x: (icon.size.0 * 256.0) as u16,
                        size_y: (icon.size.1 * 256.0) as u16,
                        atlas: atlas as u16,
                        texture_x: rect.x as u16,
                        texture_y: rect.y as u16,
                        texture_w: rect.width as u16,
                        texture_h: rect.height as u16,
                        r: color.0,
                        g: color.1,
                        b: color.2,
                        a: color.3,
                        _padding: 0,
                    }
                })
                .collect();

            if !data.is_empty() {
                let matrix = projection * view_matrix;
                let right: cgmath::Vector3<f32> = matrix.row(0).truncate().normalize();
                let up: cgmath::Vector3<f32> = matrix.row(1).truncate().normalize();

                let prog = ctx.program("icon");
                prog.use_program();

                prog.uniform("u_textures").map(|v| v.set_int(super::GLOBAL_TEXTURE_LOCATION as i32));
                prog.uniform("u_view_matrix").map(|v| v.set_matrix4(&matrix));
                prog.uniform("u_view_up").map(|v| v.set_float3(up.x, up.y, up.z));
                prog.uniform("u_view_right").map(|v| v.set_float3(right.x, right.y, right.z));

                gl::enable(gl::Flag::Blend);
                gl::blend_func(gl::BlendFunc::SrcAlpha, gl::BlendFunc::OneMinusSrcAlpha);

                self.icons.array.bind();
                self.icons.dynamic_buffer.bind(gl::BufferTarget::Array);
                self.icons.dynamic_buffer.set_data(gl::BufferTarget::Array, &data, gl::BufferUsage::Stream);

                gl::draw_arrays_instanced(gl::DrawType::Triangles, 0, 6, data.len());
                gl::disable(gl::Flag::Blend);
            }
        })
    }
}