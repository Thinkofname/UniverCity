#![allow(missing_docs)]

use gl33 as gl;
pub use self::gl::load_with;

use std::ffi;
use std::ptr;
use std::mem;
use std::marker::PhantomData;
use crate::prelude::*;

bitflags! {
    pub struct BufferBit: u32 {
        const COLOR = gl::COLOR_BUFFER_BIT;
        const DEPTH = gl::DEPTH_BUFFER_BIT;
        const STENCIL = gl::STENCIL_BUFFER_BIT;
    }
}

#[repr(u32)]
#[derive(Clone, Copy)]
pub enum BufferTarget {
    Array = gl::ARRAY_BUFFER,
    ElementArray = gl::ELEMENT_ARRAY_BUFFER,
    Texture = gl::TEXTURE_BUFFER,
    Uniform = gl::UNIFORM_BUFFER,
}

#[repr(u32)]
pub enum BufferUsage {
    Static = gl::STATIC_DRAW,
    Dynamic = gl::DYNAMIC_DRAW,
    Stream = gl::STREAM_DRAW,
}

#[repr(u32)]
pub enum ShaderType {
    Vertex = gl::VERTEX_SHADER,
    Fragment = gl::FRAGMENT_SHADER,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug)]
pub enum Type {
    UnsignedByte = gl::UNSIGNED_BYTE,
    UnsignedShort = gl::UNSIGNED_SHORT,
    UnsignedInt = gl::UNSIGNED_INT,
    UnsignedInt248 = gl::UNSIGNED_INT_24_8,
    Byte = gl::BYTE,
    Short = gl::SHORT,
    Int = gl::INT,
    HalfFloat = gl::HALF_FLOAT,
    Float = gl::FLOAT,
}

impl Type {
    #[allow(dead_code)]
    fn size(self) -> usize {
        match self {
            Type::UnsignedByte => 1,
            Type::UnsignedShort => 2,
            Type::UnsignedInt => 4,
            Type::UnsignedInt248 => 4,
            Type::Byte => 1,
            Type::Short => 2,
            Type::Int => 4,
            Type::HalfFloat => 2,
            Type::Float => 2,
        }
    }
}

#[repr(u32)]
pub enum DrawType {
    Triangles = gl::TRIANGLES,
    LineStrip = gl::LINE_STRIP,
    Lines = gl::LINES,
    Points = gl::POINTS,
}

#[repr(u32)]
pub enum Flag {
    CullFace = gl::CULL_FACE,
    DepthTest = gl::DEPTH_TEST,
    Blend = gl::BLEND,
    StencilTest = gl::STENCIL_TEST,
    FramebufferSRGB = gl::FRAMEBUFFER_SRGB,
    PolygonOffsetFill = gl::POLYGON_OFFSET_FILL,
}

#[repr(u32)]
pub enum CullFace {
    Front = gl::FRONT,
    Back = gl::BACK,
}

#[repr(u32)]
pub enum Face {
    ClockWise = gl::CW,
    CounterClockWise = gl::CCW,
}

#[repr(u32)]
pub enum Func {
    Always = gl::ALWAYS,
    Less = gl::LESS,
    LessOrEqual = gl::LEQUAL,
    Greater = gl::GREATER,
    GreaterOrEqual = gl::GEQUAL,
    Equal = gl::EQUAL,
    NotEqual = gl::NOTEQUAL,
}

#[repr(u32)]
pub enum BlendFunc {
    Zero = gl::ZERO,
    One = gl::ONE,
    SrcColor = gl::SRC_COLOR,
    OneMinusSrcColor = gl::ONE_MINUS_SRC_COLOR,
    DstColor = gl::DST_COLOR,
    OneMinusDstColor = gl::ONE_MINUS_DST_COLOR,
    SrcAlpha = gl::SRC_ALPHA,
    OneMinusSrcAlpha = gl::ONE_MINUS_SRC_ALPHA,
    DstAlpha = gl::DST_ALPHA,
    OneMinusDstAlpha = gl::ONE_MINUS_DST_ALPHA,
    ConstantColor = gl::CONSTANT_COLOR,
    OneMinusConstantColor = gl::ONE_MINUS_CONSTANT_COLOR,
    ConstantAlpha = gl::CONSTANT_ALPHA,
    OneMinusConstantAlpha = gl::ONE_MINUS_CONSTANT_ALPHA,

}

// No bounds checks here, can't be safe unless every ty+format combo is handled manually
pub unsafe fn read_pixels(x: i32, y: i32, width: i32, height: i32, format: TextureFormat, ty: Type, data: &mut [u8]) {
    gl::ReadPixels(x as _, y as _, width as _, height as _, format as _, ty as _, data.as_mut_ptr() as *mut _);
}

pub fn polygon_offset(factor: f32, units: f32) {
    unsafe {
        gl::PolygonOffset(factor, units);
    }
}

pub fn blend_func(sfactor: BlendFunc, dfactor: BlendFunc) {
    unsafe {
        gl::BlendFunc(sfactor as u32, dfactor as u32);
    }
}

pub fn enable(flag: Flag) {
    unsafe {
        gl::Enable(flag as u32);
    }
}

pub fn disable(flag: Flag) {
    unsafe {
        gl::Disable(flag as u32);
    }
}

pub fn enable_i(flag: Flag, i: u32) {
    unsafe {
        gl::Enablei(flag as u32, i);
    }
}

pub fn disable_i(flag: Flag, i: u32) {
    unsafe {
        gl::Disablei(flag as u32, i);
    }
}

pub fn depth_func(func: Func) {
    unsafe {
        gl::DepthFunc(func as u32);
    }
}

pub fn depth_mask(val: bool) {
    unsafe {
        gl::DepthMask(val as u8);
    }
}

pub fn clear_depth(depth: f64) {
    unsafe {
        gl::ClearDepth(depth);
    }
}

pub fn front_face(face: Face) {
    unsafe {
        gl::FrontFace(face as u32);
    }
}

pub fn cull_face(cull_face: CullFace) {
    unsafe {
        gl::CullFace(cull_face as u32);
    }
}

pub fn view_port(x: u32, y: u32, width: u32, height: u32) {
    unsafe {
        gl::Viewport(x as i32, y as i32, width as i32, height as i32);
    }
}

pub fn color_mask(red: bool, green: bool, blue: bool, alpha: bool) {
    unsafe {
        gl::ColorMask(red as u8, green as u8, blue as u8, alpha as u8);
    }
}

pub fn clear_color(r: f32, g: f32, b: f32, a: f32) {
    unsafe {
        gl::ClearColor(r, g, b, a);
    }
}

pub fn clear_stencil(val: i32) {
    unsafe {
        gl::ClearStencil(val);
    }
}

pub fn clear(buffer_bits: BufferBit) {
    unsafe {
        gl::Clear(buffer_bits.bits());
    }
}

pub fn draw_arrays(ty: DrawType, offset: usize, count: usize) {
    unsafe {
        gl::DrawArrays(ty as u32, offset as i32, count as i32);
    }
}

pub fn draw_arrays_instanced(ty: DrawType, offset: usize, count: usize, prim_count: usize) {
    unsafe {
        gl::DrawArraysInstanced(ty as u32, offset as i32, count as i32, prim_count as i32);
    }
}

pub fn draw_elements_instanced(ty: DrawType, count: usize, ety: Type, prim_count: usize) {
    unsafe {
        gl::DrawElementsInstanced(ty as u32, count as i32, ety as u32, ptr::null(), prim_count as i32);
    }
}

pub fn stencil_func(func: Func, r: i32, mask: u32) {
    unsafe {
        gl::StencilFunc(func as u32, r, mask);
    }
}

pub fn stencil_op(sfail: StencilOp, dpfail: StencilOp, dppass: StencilOp) {
    unsafe {
        gl::StencilOp(sfail as u32, dpfail as u32, dppass as u32);
    }
}

pub fn stencil_op_separate(face: CullFace, sfail: StencilOp, dpfail: StencilOp, dppass: StencilOp) {
    unsafe {
        gl::StencilOpSeparate(face as u32, sfail as u32, dpfail as u32, dppass as u32);
    }
}

pub fn stencil_mask(mask: u32) {
    unsafe {
        gl::StencilMask(mask);
    }
}

#[repr(u32)]
pub enum StencilOp {
    Keep = gl::KEEP,
    Zero = gl::ZERO,
    Replace = gl::REPLACE,
    Incr = gl::INCR,
    IncrWrap = gl::INCR_WRAP,
    Decr = gl::DECR,
    DecrWrap = gl::DECR_WRAP,
    Invert = gl::INVERT,
}

pub struct Buffer {
    internal: u32,
    _not_send_sync: PhantomData<*mut ()>,
}

impl Buffer {
    #[inline]
    pub fn new() -> Buffer {
        unsafe {
            let mut buffer = 0;
            gl::GenBuffers(1, &mut buffer);
            Buffer {
                internal: buffer,
                _not_send_sync: PhantomData,
            }
        }
    }

    #[inline]
    pub fn bind(&self, target: BufferTarget) {
        unsafe {
            gl::BindBuffer(target as u32, self.internal);
        }
    }

    #[inline]
    pub fn bind_uniform_block(&self, block: UniformBlock) {
        unsafe {
            gl::BindBufferBase(BufferTarget::Uniform as u32, block.internal, self.internal);
        }
    }

    #[inline]
    pub fn alloc_size<T>(&self, target: BufferTarget, len: usize, usage: BufferUsage) {
        unsafe {
            gl::BufferData(target as u32, (len * mem::size_of::<T>()) as isize, ptr::null(), usage as u32);
        }
    }

    #[inline]
    pub fn set_data<T>(&self, target: BufferTarget, data: &[T], usage: BufferUsage) {
        unsafe {
            gl::BufferData(target as u32, (data.len() * mem::size_of::<T>()) as isize, data.as_ptr() as *const _, usage as u32);
        }
    }

    #[inline]
    pub fn set_data_range<T>(&self, target: BufferTarget, data: &[T], start: i32) {
        unsafe {
            gl::BufferSubData(target as u32, start as isize * mem::size_of::<T>() as isize, (data.len() * mem::size_of::<T>()) as isize, data.as_ptr() as *const _);
        }
    }

    pub unsafe fn write_unsync<T>(&self, target: BufferTarget, length: usize) -> MappedWriteBuffer<'_, T> {
        let data = gl::MapBufferRange(
            target as u32, 0,
            (length * mem::size_of::<T>()) as isize,
            gl::MAP_WRITE_BIT | gl::MAP_UNSYNCHRONIZED_BIT | gl::MAP_INVALIDATE_BUFFER_BIT
        );
        MappedWriteBuffer {
            target,
            data: data as *mut _,
            len: length,
            _lifetime: PhantomData,
            _not_send_sync: PhantomData,
        }
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteBuffers(1, &self.internal);
        }
    }
}

pub struct MappedWriteBuffer<'a, T> {
    target: BufferTarget,
    data: *mut T,
    len: usize,
    _lifetime: PhantomData<&'a ()>,
    _not_send_sync: PhantomData<*mut ()>,
}

impl <'a, T> ::std::ops::Deref for MappedWriteBuffer<'a, T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        unsafe { ::std::slice::from_raw_parts(self.data, self.len) }
    }
}

impl <'a, T> ::std::ops::DerefMut for MappedWriteBuffer<'a, T> {

    fn deref_mut(&mut self) -> &mut [T] {
        unsafe { ::std::slice::from_raw_parts_mut(self.data, self.len) }
    }
}

impl <'a, T> Drop for MappedWriteBuffer<'a, T> {
    fn drop(&mut self) {
        unsafe {
            gl::UnmapBuffer(self.target as u32);
        }
    }
}

pub struct VertexArray {
    internal: u32,
    _not_send_sync: PhantomData<*mut ()>,
}

impl VertexArray {
    pub fn new() -> VertexArray {
        unsafe {
            let mut array = 0;
            gl::GenVertexArrays(1, &mut array);
            VertexArray {
                internal: array,
                _not_send_sync: PhantomData,
            }
        }
    }

    pub fn bind(&self) {
        unsafe {
            gl::BindVertexArray(self.internal);
        }
    }
}

impl Drop for VertexArray {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteVertexArrays(1, &self.internal);
        }
    }
}

pub struct Program {
    internal: u32,
    _not_send_sync: PhantomData<*mut ()>,
}

impl Program {
    pub fn new() -> Program {
        unsafe {
            Program {
                internal: gl::CreateProgram(),
                _not_send_sync: PhantomData,
            }
        }
    }

    pub fn link(&self) {
        unsafe {
            gl::LinkProgram(self.internal);
        }
    }

    pub fn use_program(&self) {
        unsafe {
            gl::UseProgram(self.internal);
        }
    }

    pub fn unbind() {
        unsafe {
            gl::UseProgram(0);
        }
    }

    pub fn attach_shader(&self, shader: Shader) {
        unsafe {
            gl::AttachShader(self.internal, shader.internal);
        }
    }

    pub fn uniform_block(&self, name: &str) -> UniformBlock {
        unsafe {
            let name_c = ffi::CString::new(name).expect("Failed to create CString");
            let uniform = gl::GetUniformBlockIndex(self.internal, name_c.as_ptr());
            UniformBlock {
                internal: uniform as u32,
                _not_send_sync: PhantomData,
            }
        }
    }

    pub fn uniform_location(&self, name: &str) -> Option<Uniform> {
        unsafe {
            let name_c = ffi::CString::new(name).expect("Failed to create CString");
            let uniform = gl::GetUniformLocation(self.internal, name_c.as_ptr());
            if uniform != -1 {
                Some(Uniform {
                    internal: uniform as u32,
                    _not_send_sync: PhantomData,
                })
            } else {
                None
            }
        }
    }

    pub fn attribute_location(&self, name: &str) -> Option<Attribute> {
        unsafe {
            let name_c = ffi::CString::new(name).expect("Failed to create CString");
            let attrib = gl::GetAttribLocation(self.internal, name_c.as_ptr());
            if attrib != -1 {
                Some(Attribute {
                    internal: attrib as u32,
                    _not_send_sync: PhantomData,
                })
            } else {
                None
            }
        }
    }

    pub fn bind_attribute_location(&self, name: &str, index: u32) {
        unsafe {
            let name_c = ffi::CString::new(name).expect("Failed to create CString");
            gl::BindAttribLocation(self.internal, index, name_c.as_ptr());
        }

    }
}

impl Drop for Program {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteProgram(self.internal);
        }
    }
}

pub struct Shader {
    internal: u32,
    _not_send_sync: PhantomData<*mut ()>,
}

impl Shader {
    pub fn new(ty: ShaderType) -> Shader {
        unsafe {
            Shader {
                internal: gl::CreateShader(ty as u32),
                _not_send_sync: PhantomData,
            }
        }
    }

    pub fn set_source(&self, src: &str) {
        unsafe {
            let src_c = ffi::CString::new(src).expect("Failed to create CString");
            gl::ShaderSource(self.internal, 1, &src_c.as_ptr(), ptr::null());
        }
    }

    pub fn compile(&self) -> Result<(), String> {
        unsafe {
            gl::CompileShader(self.internal);
            let mut status = 0;
            gl::GetShaderiv(self.internal, gl::COMPILE_STATUS, &mut status);
            if status == 0 {
                let mut len = 0;
                gl::GetShaderiv(self.internal, gl::INFO_LOG_LENGTH, &mut len);
                let mut buffer = Vec::with_capacity(len as usize);
                buffer.set_len(len as usize);
                gl::GetShaderInfoLog(self.internal, len, ptr::null_mut(), buffer.as_mut_ptr() as *mut _);
                let msg = ffi::CStr::from_ptr(buffer.as_ptr());
                Err(msg.to_string_lossy().into_owned())
            } else {
                Ok(())
            }
        }
    }
}

impl Drop for Shader {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteShader(self.internal);
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct UniformBlock {
    internal: u32,
    _not_send_sync: PhantomData<*mut ()>,
}

#[derive(Clone, Copy)]
pub struct Uniform {
    internal: u32,
    _not_send_sync: PhantomData<*mut ()>,
}

impl Uniform {
    pub fn set_int(self, val: i32) {
        unsafe {
            gl::Uniform1i(self.internal as i32, val);
        }
    }

    pub fn set_int2(self, x: i32, y: i32) {
        unsafe {
            gl::Uniform2i(self.internal as i32, x, y);
        }
    }

    pub fn set_int3(self, x: i32, y: i32, z: i32) {
        unsafe {
            gl::Uniform3i(self.internal as i32, x, y, z);
        }
    }

    pub fn set_int4(self, x: i32, y: i32, z: i32, w: i32) {
        unsafe {
            gl::Uniform4i(self.internal as i32, x, y, z, w);
        }
    }

    pub fn set_float(self, val: f32) {
        unsafe {
            gl::Uniform1f(self.internal as i32, val);
        }
    }

    pub fn set_float2(self, x: f32, y: f32) {
        unsafe {
            gl::Uniform2f(self.internal as i32, x, y);
        }
    }

    pub fn set_float3(self, x: f32, y: f32, z: f32) {
        unsafe {
            gl::Uniform3f(self.internal as i32, x, y, z);
        }
    }

    pub fn set_float4(self, x: f32, y: f32, z: f32, w: f32) {
        unsafe {
            gl::Uniform4f(self.internal as i32, x, y, z, w);
        }
    }

    pub fn set_vec3_array(self, v: &[cgmath::Vector3<f32>]) {
        unsafe {
            gl::Uniform3fv(self.internal as i32, v.len() as i32, v.as_ptr() as *const _);
        }
    }

    pub fn set_matrix4(self, m: &cgmath::Matrix4<f32>) {
        use cgmath::Matrix;
        unsafe {
            gl::UniformMatrix4fv(self.internal as i32, 1, false as u8, m.as_ptr());
        }
    }
}

#[derive(Clone, Copy)]
pub struct Attribute {
    internal: u32,
    _not_send_sync: PhantomData<*mut ()>,
}

impl Attribute {
    pub fn new(i: u32) -> Attribute {
        Attribute {
            internal: i,
            _not_send_sync: PhantomData,
        }
    }

    pub fn offset(self, off: u32) -> Attribute {
        Attribute {
            internal: self.internal + off,
            _not_send_sync: PhantomData,
        }
    }

    pub fn enable(self) {
        unsafe {
            gl::EnableVertexAttribArray(self.internal);
        }
    }

    pub fn disable(self) {
        unsafe {
            gl::DisableVertexAttribArray(self.internal);
        }
    }

    pub fn vertex_pointer(self, size: i32, ty: Type, normalized: bool, stride: i32, offset: i32) {
        unsafe {
            gl::VertexAttribPointer(self.internal, size, ty as u32, normalized as u8, stride, offset as *const _);
        }
    }

    pub fn vertex_int_pointer(self, size: i32, ty: Type, stride: i32, offset: i32) {
        unsafe {
            gl::VertexAttribIPointer(self.internal, size, ty as u32,stride, offset as *const _);
        }
    }

    pub fn divisor(self, divisor: u32) {
        unsafe {
            gl::VertexAttribDivisor(self.internal, divisor);
        }
    }

    pub fn index(self) -> u32 {
        self.internal
    }
}

#[repr(u32)]
pub enum TextureTarget {
    Texture2D = gl::TEXTURE_2D,
    Texture2DArray = gl::TEXTURE_2D_ARRAY,
    Texture3D = gl::TEXTURE_3D,
    TextureBuffer = gl::TEXTURE_BUFFER,
}

#[repr(u32)]
#[derive(Debug)]
pub enum TextureFormat {
    Red = gl::RED,
    Rg = gl::RG,
    Rgb = gl::RGB,
    Rgba = gl::RGBA,
    R8 = gl::R8,
    R8I = gl::R8I,
    R16 = gl::R16,
    R16F = gl::R16F,
    R32F = gl::R32F,
    Rg8 = gl::RG8,
    Rg8I = gl::RG8I,
    Rg16 = gl::RG16,
    Rg16F = gl::RG16F,
    Rg32F = gl::RG32F,
    Rgb8 = gl::RGB8,
    Rgb8I = gl::RGB8I,
    Rgb16 = gl::RGB16,
    Rgb16F = gl::RGB16F,
    Rgb32F = gl::RGB32F,
    Rgba8 = gl::RGBA8,
    Rgba8I = gl::RGBA8I,
    Rgba16 = gl::RGBA16,
    Rgba16F = gl::RGBA16F,
    Rgba32F = gl::RGBA32F,
    Srgb = gl::SRGB,
    Srgb8 = gl::SRGB8,
    Srgba = gl::SRGB_ALPHA,
    Srgba8 = gl::SRGB8_ALPHA8,
    DepthComponent24 = gl::DEPTH_COMPONENT24,
    DepthComponent = gl::DEPTH_COMPONENT,
    Depth24Stencil8 = gl::DEPTH24_STENCIL8,
    DepthStencil = gl::DEPTH_STENCIL,
}

impl TextureFormat {
    #[allow(dead_code)]
    fn size_per_element(&self, ty: Type) -> usize {
        match self {
            TextureFormat::Red => ty.size(),
            TextureFormat::Rg => ty.size() * 2,
            TextureFormat::Rgb => ty.size() * 3,
            TextureFormat::Rgba => ty.size() * 4,
            TextureFormat::R8 => 1,
            TextureFormat::R8I => 1,
            TextureFormat::R16 => 2,
            TextureFormat::R16F => 2,
            TextureFormat::R32F => 4,
            TextureFormat::Rg8 => 2,
            TextureFormat::Rg8I => 2,
            TextureFormat::Rg16 => 4,
            TextureFormat::Rg16F => 4,
            TextureFormat::Rg32F => 8,
            TextureFormat::Rgb8 => 3,
            TextureFormat::Rgb8I => 3,
            TextureFormat::Rgb16 => 6,
            TextureFormat::Rgb16F => 6,
            TextureFormat::Rgb32F => 12,
            TextureFormat::Rgba8 => 4,
            TextureFormat::Rgba8I => 4,
            TextureFormat::Rgba16 => 8,
            TextureFormat::Rgba16F => 8,
            TextureFormat::Rgba32F => 16,
            TextureFormat::Srgb => ty.size() * 3,
            TextureFormat::Srgb8 => 3,
            TextureFormat::Srgba => ty.size() * 4,
            TextureFormat::Srgba8 => 4,
            TextureFormat::DepthComponent24 => 3,
            TextureFormat::DepthComponent => ty.size(),
            TextureFormat::Depth24Stencil8 => 4,
            TextureFormat::DepthStencil => ty.size(),
        }
    }
}

pub fn active_texture(id: u32) {
    unsafe {
        gl::ActiveTexture(gl::TEXTURE0 + id);
    }
}

#[repr(u32)]
pub enum PixelStore {
    UnpackAlignment = gl::UNPACK_ALIGNMENT,
}

pub fn pixel_store(pname: PixelStore, val: i32) {
    unsafe {
        gl::PixelStorei(pname as u32, val);
    }
}

pub struct Texture {
    internal: u32,
    _not_send_sync: PhantomData<*mut ()>,
}

impl Texture {
    pub fn new() -> Texture {
        unsafe {
            let mut t = 0;
            gl::GenTextures(1, &mut t);
            Texture{
                internal: t,
                _not_send_sync: PhantomData,
            }
        }
    }

    pub fn bind(&self, target: TextureTarget) {
        unsafe {
            gl::BindTexture(target as u32, self.internal);
        }
    }

    pub fn buffer(&self, target: TextureTarget, buffer: &Buffer, format: TextureFormat) {
        unsafe {
            gl::TexBuffer(target as u32, format as u32, buffer.internal);
        }
    }

    pub fn image_2d_ex(&self, target: TextureTarget,
        level: i32, width: u32, height: u32,
        internal_format: TextureFormat, format: TextureFormat,
        ty: Type, pix: Option<&[u8]>)
    {
        unsafe {
            let ptr = match pix {
                Some(val) => val.as_ptr() as *const _,
                None => ptr::null(),
            };
            gl::TexImage2D(
                target as u32, level, internal_format as i32,
                width as i32, height as i32, 0, format as u32,
                ty as u32, ptr
            );
        }
    }

    pub fn image_2d_any<T>(&self, target: TextureTarget,
        level: i32, width: u32, height: u32,
        internal_format: TextureFormat, format: TextureFormat,
        ty: Type, pix: Option<&[T]>)
    {
        unsafe {
            let ptr = match pix {
                Some(val) => val.as_ptr() as *const _,
                None => ptr::null(),
            };
            gl::TexImage2D(
                target as u32, level, internal_format as i32,
                width as i32, height as i32, 0, format as u32,
                ty as u32, ptr
            );
        }
    }

    pub fn image_3d(&self, target: TextureTarget,
        level: i32,
        width: u32, height: u32, depth: u32,
        internal_format: TextureFormat, format: TextureFormat,
        ty: Type, pix: Option<&[u8]>)
    {
        unsafe {
            let ptr = match pix {
                Some(val) => val.as_ptr() as *const _,
                None => ptr::null(),
            };
            gl::TexImage3D(
                target as u32, level, internal_format as i32,
                width as i32, height as i32, depth as i32, 0, format as u32,
                ty as u32, ptr
            );
        }
    }

    pub fn sub_image_3d(&self, target: TextureTarget,
        level: i32,
        x: u32, y: u32, z: u32,
        width: u32, height: u32, depth: u32,
        format: TextureFormat,
        ty: Type, pix: Option<&[u8]>)
    {
        unsafe {
            let ptr = match pix {
                Some(val) => val.as_ptr() as *const _,
                None => ptr::null(),
            };
            gl::TexSubImage3D(
                target as u32, level,
                x as i32, y as i32, z as i32,
                width as i32, height as i32, depth as i32, format as u32,
                ty as u32, ptr
            );
        }
    }

    pub fn get_data(&self, target: TextureTarget, level: i32, format: TextureFormat, ty: Type, pix: &mut [u8]) {
        unsafe {
            gl::GetTexImage(target as u32, level, format as u32, ty as u32, pix.as_mut_ptr() as *mut _);
        }
    }

    pub fn set_parameter<P: TextureParameter>(&self, target: TextureTarget, value: P::Value) {
        unsafe {
            P::set_parameter(target, value);
        }
    }
}

impl Drop for Texture {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteTextures(1, &self.internal);
        }
    }
}

pub trait TextureParameter {
    type Value;

    unsafe fn set_parameter(target: TextureTarget, value: Self::Value);
}

pub struct TextureMinFilter;
impl TextureParameter for TextureMinFilter {
    type Value = TextureFilter;

    unsafe fn set_parameter(target: TextureTarget, value: Self::Value) {
        gl::TexParameteri(target as u32, gl::TEXTURE_MIN_FILTER, value.into());
    }
}

pub struct TextureMagFilter;
impl TextureParameter for TextureMagFilter {
    type Value = TextureFilter;

    unsafe fn set_parameter(target: TextureTarget, value: Self::Value) {
        gl::TexParameteri(target as u32, gl::TEXTURE_MAG_FILTER, value.into());
    }
}

#[repr(i32)]
#[derive(Clone, Copy)]
pub enum TextureFilter {
    Nearest = gl::NEAREST as i32,
    Linear = gl::LINEAR as i32,
    LinearMipmapLinear = gl::LINEAR_MIPMAP_LINEAR as i32,
    LinearMipmapNearest = gl::LINEAR_MIPMAP_NEAREST as i32,
    NearestMipmapLinear = gl::NEAREST_MIPMAP_LINEAR as i32,
    NearestMipmapNearest = gl::NEAREST_MIPMAP_NEAREST as i32,
}
impl From<TextureFilter> for i32 {
    fn from(val: TextureFilter) -> i32  { val as i32 }
}

pub struct TextureWrapS;
impl TextureParameter for TextureWrapS {
    type Value = TextureWrap;

    unsafe fn set_parameter(target: TextureTarget, value: Self::Value) {
        gl::TexParameteri(target as u32, gl::TEXTURE_WRAP_S, value.into());
    }
}
pub struct TextureWrapT;
impl TextureParameter for TextureWrapT {
    type Value = TextureWrap;

    unsafe fn set_parameter(target: TextureTarget, value: Self::Value) {
        gl::TexParameteri(target as u32, gl::TEXTURE_WRAP_T, value.into());
    }
}

#[repr(i32)]
pub enum TextureWrap {
    ClampToEdge = gl::CLAMP_TO_EDGE as i32,
    ClampToBorder= gl::CLAMP_TO_BORDER as i32,
    MirroredRepeat = gl::MIRRORED_REPEAT as i32,
    Repeat = gl::REPEAT as i32,
}
impl From<TextureWrap> for i32 {
    fn from(val: TextureWrap) -> i32 { val as i32 }
}

pub struct TextureBorderColor;
impl TextureParameter for TextureBorderColor {
    type Value = [f32; 4];

    unsafe fn set_parameter(target: TextureTarget, value: Self::Value) {
        gl::TexParameterfv(target as u32, gl::TEXTURE_BORDER_COLOR, value.as_ptr());
    }
}

pub struct TextureBaseLevel;
impl TextureParameter for TextureBaseLevel {
    type Value = i32;

    unsafe fn set_parameter(target: TextureTarget, value: Self::Value) {
        gl::TexParameteri(target as u32, gl::TEXTURE_BASE_LEVEL, value);
    }
}

pub struct TextureMaxLevel;
impl TextureParameter for TextureMaxLevel {
    type Value = i32;

    unsafe fn set_parameter(target: TextureTarget, value: Self::Value) {
        gl::TexParameteri(target as u32, gl::TEXTURE_MAX_LEVEL, value);
    }
}

#[repr(i32)]
pub enum CompareMode {
    CompareRefToTexture = gl::COMPARE_REF_TO_TEXTURE as i32,
    None = gl::NONE as i32,
}

pub struct TextureCompareMode;
impl TextureParameter for TextureCompareMode {
    type Value = CompareMode;

    unsafe fn set_parameter(target: TextureTarget, value: Self::Value) {
        gl::TexParameteri(target as u32, gl::TEXTURE_COMPARE_MODE, value as i32);
    }
}

pub struct TextureCompareFunc;
impl TextureParameter for TextureCompareFunc {
    type Value = Func;

    unsafe fn set_parameter(target: TextureTarget, value: Self::Value) {
        gl::TexParameteri(target as u32, gl::TEXTURE_COMPARE_FUNC, value as i32);
    }
}

#[repr(u32)]
pub enum TargetFramebuffer {
    Both = gl::FRAMEBUFFER,
    Read = gl::READ_FRAMEBUFFER,
    Draw = gl::DRAW_FRAMEBUFFER,
}

#[repr(u32)]
pub enum Attachment {
    Color0 = gl::COLOR_ATTACHMENT0,
    Color1 = gl::COLOR_ATTACHMENT1,
    Color2 = gl::COLOR_ATTACHMENT2,
    Color3 = gl::COLOR_ATTACHMENT3,
    Color4 = gl::COLOR_ATTACHMENT4,
    Depth = gl::DEPTH_ATTACHMENT,
    DepthStencil = gl::DEPTH_STENCIL_ATTACHMENT,
}

pub fn bind_frag_data_location(p: &Program, cn: u32, name: &str) {
    unsafe {
        let name_c = ffi::CString::new(name).expect("Failed to create CString");
        gl::BindFragDataLocation(p.internal, cn, name_c.as_ptr());
    }
}

pub struct Framebuffer {
    internal: u32,
    _not_send_sync: PhantomData<*mut ()>,
}

impl Framebuffer {
    pub fn new() -> Framebuffer {
        unsafe {
            let mut fb = 0;
            gl::GenFramebuffers(1, &mut fb);
            Framebuffer {
                internal: fb,
                _not_send_sync: PhantomData,
            }
        }
    }

    pub fn bind(&self, target: TargetFramebuffer) {
        unsafe {
            gl::BindFramebuffer(target as u32, self.internal);
        }
    }

    pub fn unbind(target: TargetFramebuffer) {
        unsafe {
            gl::BindFramebuffer(target as u32, 0);
        }
    }

    pub fn texture_2d(&self, fbtarget: TargetFramebuffer, attachment: Attachment, target: TextureTarget, tex: &Texture, level: i32) {
        unsafe {
            gl::FramebufferTexture2D(fbtarget as u32, attachment as u32, target as u32, tex.internal, level);
        }
    }
}

impl Drop for Framebuffer {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteFramebuffers(1, &self.internal);
        }
    }
}

pub fn draw_buffers(bufs: &[Attachment]) {
    unsafe {
        gl::DrawBuffers(
            bufs.len() as i32,
            bufs.as_ptr() as *const _,
        );
    }
}

#[repr(u32)]
pub enum Mode {
    None = gl::NONE,
    FrontLeft = gl::FRONT_LEFT,
    FrontRight = gl::FRONT_RIGHT,
    BackLeft = gl::BACK_LEFT,
    BackRight = gl::BACK_RIGHT,
    Front = gl::FRONT,
    Back = gl::BACK,
    Left = gl::LEFT,
    Right = gl::RIGHT,
    FrontAndBack = gl::FRONT_AND_BACK,
}

pub fn draw_buffer(mode: Mode) {
    unsafe {
        gl::DrawBuffer(mode as u32);
    }
}

pub fn read_buffer(mode: Mode) {
    unsafe {
        gl::ReadBuffer(mode as u32);
    }
}

#[repr(u32)]
#[derive(Clone, Copy)]
pub enum TargetBuffer {
    Color = gl::COLOR,
    Depth = gl::DEPTH,
    Stencil = gl::STENCIL,
}

pub fn clear_buffer(buffer: TargetBuffer, draw_buffer: i32, values: &[f32]) {
    unsafe {
        gl::ClearBufferfv(buffer as u32, draw_buffer, values.as_ptr());
    }
}
pub fn clear_bufferi(buffer: TargetBuffer, draw_buffer: i32, values: &[i32]) {
    unsafe {
        gl::ClearBufferiv(buffer as u32, draw_buffer, values.as_ptr());
    }
}
