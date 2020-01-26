

use crate::util::FNVMap;
use crate::prelude::*;
use cgmath::Matrix4;
use super::gl;
use super::pipeline;
use std::mem;

pub struct Model<V> {
    verts: Vec<V>,
    pub shader: &'static str,
    gl: GLModel,
    pub model_matrix: Matrix4<f32>,
    pub uniforms: FNVMap<&'static str, UniformValue>,
    dirty: bool,
}

#[derive(Debug)]
pub enum UniformValue {
    Matrix4(Matrix4<f32>),
    Float(f32),
    Int(i32),
}

pub struct Attribute {
    pub name: &'static str,
    pub count: u32,
    pub ty: Type,
    pub offset: u32,
    pub int: bool,
}

pub type Type = gl::Type;

struct GLModel {
    array: gl::VertexArray,
    buffer: gl::Buffer,
    alloc_size: usize,
}

impl <V> Model<V> {
    pub fn new(ctx: &mut pipeline::Context<'_>, shader: &'static str, attribs: Vec<Attribute>, verts: Vec<V>) -> Model<V> {
        use cgmath::SquareMatrix;
        let program = ctx.program(shader);
        program.use_program();

        let array = gl::VertexArray::new();
        array.bind();
        let buffer = gl::Buffer::new();
        buffer.bind(gl::BufferTarget::Array);
        for attrib in &attribs {
            if let Some(att) = program.attribute(attrib.name) {
                att.enable();
                if attrib.int {
                    att.vertex_int_pointer(attrib.count as i32, attrib.ty, mem::size_of::<V>() as i32, attrib.offset as i32);
                } else {
                    att.vertex_pointer(attrib.count as i32, attrib.ty, false, mem::size_of::<V>() as i32, attrib.offset as i32);
                }
            }
        }
        buffer.set_data(gl::BufferTarget::Array, &verts, gl::BufferUsage::Static);
        let gl = GLModel {
            array,
            buffer,
            alloc_size: verts.len(),
        };
        Model {
            shader,
            verts,
            gl,
            model_matrix: Matrix4::identity(),
            uniforms: Default::default(),
            dirty: false,
        }
    }

    pub fn set_verts(&mut self, verts: Vec<V>) {
        self.verts = verts;
        self.dirty = true;
    }

    pub fn draw(
            &mut self, ctx: &mut pipeline::Context<'_>,
            projection: &Matrix4<f32>,
            view_matrix: &Matrix4<f32>,
    ) {
        let program = ctx.program(self.shader);
        program.use_program();
        let mdl = &mut self.gl;
        if self.dirty {
            mdl.buffer.bind(gl::BufferTarget::Array);
            if self.verts.len() <= mdl.alloc_size {
                mdl.buffer.set_data_range(gl::BufferTarget::Array, &self.verts, 0);
            } else {
                mdl.buffer.set_data(gl::BufferTarget::Array, &self.verts, gl::BufferUsage::Dynamic);
                mdl.alloc_size = self.verts.len();
            }
            self.dirty = false;
        }

        program.uniform("view_matrix").map(|v| v.set_matrix4(view_matrix));
        program.uniform("projection_matrix").map(|v| v.set_matrix4(projection));
        let model_mat = &self.model_matrix;
        program.uniform("model_matrix").map(|v| v.set_matrix4(model_mat));
        for (name, val) in &self.uniforms {
            if let Some(uni) = program.uniform(name) {
                match *val {
                    UniformValue::Float(val) => uni.set_float(val),
                    UniformValue::Int(val) => uni.set_int(val),
                    UniformValue::Matrix4(ref mat) => uni.set_matrix4(mat),
                }
            }
        }
        mdl.array.bind();
        gl::draw_arrays(gl::DrawType::Triangles, 0, self.verts.len());
    }
}
