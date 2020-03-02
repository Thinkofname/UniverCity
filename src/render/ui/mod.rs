//! fungui UI renderer

use crate::prelude::*;
use crate::render::gl;
use crate::render::pipeline;
use cgmath;
use fungui::*;
use std::cell::RefCell;
use std::mem;
use std::rc::Rc;

use super::atlas;
use super::ATLAS_SIZE;
use rusttype;

pub(crate) mod border;
pub(crate) mod color;
pub(crate) mod shadow;
pub(crate) mod text_shadow;

pub struct Renderer {
    log: Logger,
    assets: AssetManager,
    width: u32,
    height: u32,
    pub(super) ui_scale: f32,

    fonts: Rc<RefCell<FNVMap<String, Font>>>,
    font_texture: gl::Texture,
    font_atlases: Vec<atlas::TextureAtlas>,

    clip_array: gl::VertexArray,
    _clip_buffer: gl::Buffer,
}

struct Font {
    font: rusttype::Font<'static>,
    chars: FNVMap<(i32, rusttype::GlyphId), (i32, atlas::Rect)>,
}

impl Renderer {
    pub fn new(log: &Logger, assets: &AssetManager, ctx: &mut pipeline::Context<'_>) -> Renderer {
        let attrib_position = {
            let program = ctx.program("ui/clip");
            program.use_program();
            (assume!(log, program.attribute("attrib_position")))
        };

        let array = gl::VertexArray::new();
        array.bind();

        let buffer = gl::Buffer::new();
        buffer.bind(gl::BufferTarget::Array);

        attrib_position.enable();
        attrib_position.vertex_pointer(
            2,
            gl::Type::Float,
            false,
            mem::size_of::<ClipVertex>() as i32,
            0,
        );

        let verts = &[
            ClipVertex { x: 0.0, y: 1.0 },
            ClipVertex { x: 0.0, y: 0.0 },
            ClipVertex { x: 1.0, y: 1.0 },
            ClipVertex { x: 0.0, y: 0.0 },
            ClipVertex { x: 1.0, y: 0.0 },
            ClipVertex { x: 1.0, y: 1.0 },
        ];

        buffer.set_data(gl::BufferTarget::Array, verts, gl::BufferUsage::Static);

        let texture = gl::Texture::new();
        texture.bind(gl::TextureTarget::Texture2DArray);
        texture.image_3d(
            gl::TextureTarget::Texture2DArray,
            0,
            ATLAS_SIZE as u32,
            ATLAS_SIZE as u32,
            1,
            gl::TextureFormat::R8,
            gl::TextureFormat::Red,
            gl::Type::UnsignedByte,
            None,
        );
        texture.set_parameter::<gl::TextureMinFilter>(
            gl::TextureTarget::Texture2DArray,
            gl::TextureFilter::Nearest,
        );
        texture.set_parameter::<gl::TextureMagFilter>(
            gl::TextureTarget::Texture2DArray,
            gl::TextureFilter::Nearest,
        );
        texture.set_parameter::<gl::TextureWrapS>(
            gl::TextureTarget::Texture2DArray,
            gl::TextureWrap::ClampToEdge,
        );
        texture.set_parameter::<gl::TextureWrapT>(
            gl::TextureTarget::Texture2DArray,
            gl::TextureWrap::ClampToEdge,
        );
        texture.set_parameter::<gl::TextureBaseLevel>(gl::TextureTarget::Texture2DArray, 0);
        texture.set_parameter::<gl::TextureMaxLevel>(gl::TextureTarget::Texture2DArray, 0);

        Renderer {
            log: log.new(o!("type" => "ui-renderer")),
            assets: assets.clone(),
            width: 800,
            height: 480,
            ui_scale: 1.0,

            clip_array: array,
            _clip_buffer: buffer,

            fonts: Rc::new(RefCell::new(FNVMap::default())),
            font_texture: texture,
            font_atlases: Vec::new(),
        }
    }

    pub fn set_scale(&mut self, scale: f32) {
        self.ui_scale = scale;
        for font in self.fonts.borrow_mut().values_mut() {
            font.chars.clear();
        }
        for atlas in &mut self.font_atlases {
            *atlas = atlas::TextureAtlas::new(ATLAS_SIZE, ATLAS_SIZE);
        }
    }

    pub fn init(&mut self, manager: &mut Manager<UniverCityUI>) {
        manager.add_func_raw("rgb", color::rgb);
        manager.add_func_raw("rgba", color::rgba);
        manager.add_func_raw("deg", deg);
        manager.add_func_raw(ui::BORDER.0, border::border);
        manager.add_func_raw("bside", border::border_side);
        manager.add_func_raw("border_width", border::border_width);
        manager.add_func_raw("border_image", border::border_image);
        manager.add_func_raw("shadow", shadow::shadow);
        manager.add_func_raw("shadows", shadow::shadows);
        manager.add_func_raw("text_shadow", text_shadow::text_shadow);

        let fonts = self.fonts.clone();
        let assets = self.assets.clone();
        manager.add_layout_engine(move || Lined::new(assets.clone(), fonts.clone()));
    }

    pub(super) fn update_image(
        &mut self,
        global_atlas: &mut super::GlobalAtlas,
        name: ResourceKey<'_>,
        width: u32,
        height: u32,
        data: Vec<u8>,
    ) {
        use super::image::{Image, ImageFuture};
        if let Some((idx, rect)) = global_atlas.textures.get(&name) {
            // Update existing
            let img = Image {
                width,
                height,
                data,
            };
            super::RenderState::upload_texture(&global_atlas.texture, Some(img), *idx, *rect);
            return;
        }

        let info = super::RenderState::place_texture_atlas(
            global_atlas,
            ImageFuture::completed(width, height, data),
        );
        global_atlas.textures.insert(name.into_owned(), info);
    }

    pub fn layout(&mut self, manager: &mut Manager<UniverCityUI>, width: u32, height: u32) {
        // TODO: return value?
        self.width = (width as f32 * self.ui_scale) as u32;
        self.height = (height as f32 * self.ui_scale) as u32;
        manager.layout(self.width as i32, self.height as i32);
    }

    pub(super) fn draw<'a>(
        &mut self,
        manager: &mut Manager<UniverCityUI>,
        ctx: &'a mut pipeline::Context<'a>,
        global_atlas: &mut super::GlobalAtlas,
    ) {
        // TODO: Don't do this all the time?

        let view_matrix =
            cgmath::ortho(0.0, self.width as f32, self.height as f32, 0.0, -1.0, 10.0);

        gl::disable(gl::Flag::DepthTest);
        gl::enable(gl::Flag::Blend);
        gl::enable(gl::Flag::StencilTest);
        // TODO: Do only for things that need it
        gl::blend_func(gl::BlendFunc::SrcAlpha, gl::BlendFunc::OneMinusSrcAlpha);

        gl::active_texture(1);
        self.font_texture.bind(gl::TextureTarget::Texture2DArray);

        do_clip(
            ctx,
            &view_matrix,
            &self.clip_array,
            0.0,
            0.0,
            self.width as f32,
            self.height as f32,
        );
        manager.render(&mut Builder {
            log: &self.log,
            ui_scale: self.ui_scale,
            view_matrix,
            ctx,
            clip_array: &self.clip_array,
            assets: &self.assets,
            global_atlas,
            offset: Vec::new(),
            clips: vec![(0.0, 0.0, self.width as f32, self.height as f32)],

            fonts: &mut *self.fonts.borrow_mut(),
            font_texture: &mut self.font_texture,
            font_atlases: &mut self.font_atlases,
        });

        gl::enable(gl::Flag::DepthTest);
        gl::disable(gl::Flag::Blend);
        gl::disable(gl::Flag::StencilTest);
    }
}

struct CharData {
    data: Vec<u8>,
    width: i32,
    height: i32,
}

fn place_font_atlas(
    font_atlases: &mut Vec<atlas::TextureAtlas>,
    font_texture: &gl::Texture,
    c: CharData,
) -> (i32, atlas::Rect) {
    let (width, height) = (c.width, c.height);

    for (idx, ref mut atlas) in font_atlases.iter_mut().enumerate() {
        if let Some(rect) = atlas.find(width as i32, height as i32) {
            upload_font_data(&font_texture, c, idx as i32, rect);
            return (idx as i32, rect);
        }
    }

    // Resize
    gl::pixel_store(gl::PixelStore::UnpackAlignment, 1);
    font_texture.bind(gl::TextureTarget::Texture2DArray);
    // Get the original image data
    let layers = font_atlases.len();
    let orig = if layers != 0 {
        let mut orig = vec![0; ATLAS_SIZE as usize * ATLAS_SIZE as usize * layers];
        font_texture.get_data(
            gl::TextureTarget::Texture2DArray,
            0,
            gl::TextureFormat::Red,
            gl::Type::UnsignedByte,
            &mut orig,
        );
        Some(orig)
    } else {
        None
    };
    // Resize the texture
    font_texture.image_3d(
        gl::TextureTarget::Texture2DArray,
        0,
        ATLAS_SIZE as u32,
        ATLAS_SIZE as u32,
        (layers + 1) as u32,
        gl::TextureFormat::R8,
        gl::TextureFormat::Red,
        gl::Type::UnsignedByte,
        None,
    );
    // Place old data back
    if let Some(orig) = orig {
        font_texture.sub_image_3d(
            gl::TextureTarget::Texture2DArray,
            0,
            0,
            0,
            0,
            ATLAS_SIZE as u32,
            ATLAS_SIZE as u32,
            layers as u32,
            gl::TextureFormat::Red,
            gl::Type::UnsignedByte,
            Some(&orig),
        );
    }
    gl::pixel_store(gl::PixelStore::UnpackAlignment, 4);
    let mut atlas = atlas::TextureAtlas::new(ATLAS_SIZE, ATLAS_SIZE);
    let rect = atlas
        .find(width as i32, height as i32)
        .expect("Out of texture space");
    let idx = layers as i32;
    upload_font_data(&font_texture, c, idx, rect);
    font_atlases.push(atlas);
    (idx, rect)
}

fn upload_font_data(texture: &gl::Texture, tex: CharData, atlas: i32, rect: atlas::Rect) {
    gl::pixel_store(gl::PixelStore::UnpackAlignment, 1);
    texture.bind(gl::TextureTarget::Texture2DArray);
    texture.sub_image_3d(
        gl::TextureTarget::Texture2DArray,
        0,
        rect.x as u32,
        rect.y as u32,
        atlas as u32,
        rect.width as u32,
        rect.height as u32,
        1,
        gl::TextureFormat::Red,
        gl::Type::UnsignedByte,
        Some(&tex.data),
    );
    gl::pixel_store(gl::PixelStore::UnpackAlignment, 4);
}

pub(crate) struct BoxRender {
    array: gl::VertexArray,
    _buffer: gl::Buffer,
    count: usize,
}

pub(crate) struct ShadowRender {
    array: gl::VertexArray,
    _buffer: gl::Buffer,
    u_buffer: (gl::UniformBlock, gl::Buffer),
    count: usize,
    inset: bool,
}

pub(crate) struct ImageRender {
    array: gl::VertexArray,
    _buffer: gl::Buffer,
    count: usize,
}

pub(crate) struct TextRender {
    array: gl::VertexArray,
    _buffer: gl::Buffer,
    count: usize,
    color: color::Color,
    shadow: Option<TextShadowRender>,
}
pub(crate) struct TextShadowRender {
    array: gl::VertexArray,
    _buffer: gl::Buffer,
    count: usize,
    color: color::Color,
    radius: f32,
}

pub(crate) struct BorderRender {
    array: gl::VertexArray,
    _buffer: gl::Buffer,
    count: usize,
    image: bool,
}

#[repr(C)]
struct ClipVertex {
    x: f32,
    y: f32,
}

#[repr(C)]
struct BoxVertex {
    x: f32,
    y: f32,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

#[repr(C)]
struct TextVertex {
    x: f32,
    y: f32,
    texture_x: u16,
    texture_y: u16,
    texture_w: u16,
    texture_h: u16,
    atlas: u16,
    _padding: u16,
    ux: f32,
    uy: f32,
}

#[repr(C)]
struct ImageVertex {
    x: f32,
    y: f32,
    texture_x: u16,
    texture_y: u16,
    texture_w: u16,
    texture_h: u16,
    atlas: u16,
    _padding: u16,
    ux: f32,
    uy: f32,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct BorderVertex {
    x: f32,
    y: f32,
    rel_x: f32,
    rel_y: f32,
    size: f32,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
    style: i8,
    dir_x: i8,
    dir_y: i8,
    padding: i8,
}

struct Builder<'a, 'b> {
    log: &'b Logger,
    ui_scale: f32,
    view_matrix: cgmath::Matrix4<f32>,
    clip_array: &'b gl::VertexArray,
    ctx: &'a mut pipeline::Context<'a>,
    assets: &'b AssetManager,
    global_atlas: &'b mut super::GlobalAtlas,
    offset: Vec<(f32, f32)>,
    clips: Vec<(f32, f32, f32, f32)>,

    fonts: &'b mut FNVMap<String, Font>,
    font_texture: &'b gl::Texture,
    font_atlases: &'b mut Vec<atlas::TextureAtlas>,
}

fn load_font<'a>(
    assets: &AssetManager,
    fonts: &'a mut FNVMap<String, Font>,
    font_key: &str,
) -> Option<&'a mut Font> {
    use std::io::Read;

    if let Some(font) = fonts.get_mut(font_key) {
        // This is safe fuck you rustc
        return Some(unsafe { std::mem::transmute(font) });
    }

    // Try and load the font from the asset bundles
    let font = {
        let key = LazyResourceKey::parse(&font_key).or_module(ModuleKey::new("base"));
        assets
            .open_from_pack(key.module_key(), &format!("fonts/{}.ttf", key.resource()))
            .ok()
            .and_then(|mut v| {
                let mut data = Vec::new();
                v.read_to_end(&mut data).ok()?;
                Some(data)
            })
            .and_then(|v| rusttype::Font::from_bytes(v).ok())
    };
    if let Some(f) = font {
        Some(fonts.entry(font_key.to_owned()).or_insert_with(|| Font {
            font: f,
            chars: Default::default(),
        }))
    } else {
        None
    }
}

impl<'a, 'b> RenderVisitor<UniverCityUI> for Builder<'a, 'b> {
    fn visit(&mut self, obj: &mut NodeInner<UniverCityUI>) {
        let offset = self.offset.last().cloned().unwrap_or((0.0, 0.0));

        let position = (
            obj.draw_rect.x as f32 + offset.0,
            obj.draw_rect.y as f32 + offset.1,
        );
        let width = obj.draw_rect.width as f32;
        let height = obj.draw_rect.height as f32;

        let render_loc = (position.0, position.1, width, height);
        if obj.ext.render_loc != render_loc {
            obj.ext.image_render = None;
            obj.ext.box_render = None;
            obj.ext.text_render = None;
            obj.ext.border_render = None;
            obj.ext.shadow_render.clear();

            obj.ext.render_loc = render_loc;
        }

        if let Some(text) = obj
            .value
            .text()
            .filter(|_| obj.ext.text_render.is_none() || obj.text_changed)
            .filter(|_| obj.ext.font_size > 0.0)
        {
            obj.text_changed = false;
            // Can only render if we have a font to work with
            let fonts = &mut *self.fonts;
            let assets = &*self.assets;

            if let Some(font) = obj
                .ext
                .font
                .as_ref()
                .and_then(|font| load_font(assets, fonts, font))
            {
                if obj.ext.text_splits.is_empty() {
                    obj.ext.text_splits.push((0, text.len(), obj.draw_rect));
                }
                let size = (obj.ext.font_size * 4.0).round() as i32;
                let color = obj.ext.font_color;

                let shadow_info = obj.ext.text_shadow.as_ref();

                let mut shadow_verts = shadow_info.map(|v| (v, Vec::with_capacity(text.len() * 6)));
                let mut verts = Vec::with_capacity(text.len() * 6);

                let scale = rusttype::Scale::uniform(obj.ext.font_size);

                let voffset = {
                    let m = font.font.v_metrics(scale);
                    (m.ascent - m.descent) * 0.8
                };
                for (start, end, pos) in &obj.ext.text_splits {
                    let mut prev = None;
                    let mut line_offset = 0.0;
                    for g in font.font.glyphs_for(text[*start..*end].chars()) {
                        let g = g.scaled(scale);
                        if let Some(prev) = prev {
                            line_offset += font.font.pair_kerning(scale, prev, g.id());
                        }
                        let width = g.h_metrics().advance_width;
                        let g = g.positioned(rusttype::Point {
                            x: offset.0 + pos.x as f32 + line_offset,
                            y: offset.1 + pos.y as f32 + voffset,
                        });
                        let fa = &mut *self.font_atlases;
                        let ft = &*self.font_texture;
                        let ui_scale = self.ui_scale;
                        let (atlas, rect) = font.chars.entry((size, g.id())).or_insert_with(|| {
                            let g = g
                                .unpositioned()
                                .unscaled()
                                .standalone()
                                .scaled(rusttype::Scale::uniform(size as f32 / (4.0 * ui_scale)))
                                .positioned(rusttype::Point { x: 0.0, y: 0.0 });
                            if let Some(bound) = g.pixel_bounding_box() {
                                let mut data = vec![0; (bound.width() * bound.height()) as usize];
                                g.draw(|x, y, o| {
                                    data[(x + y * bound.width() as u32) as usize] =
                                        (o * 255.0).round().min(255.0).max(0.0) as u8;
                                });
                                place_font_atlas(
                                    fa,
                                    ft,
                                    CharData {
                                        width: bound.width(),
                                        height: bound.height(),
                                        data: data,
                                    },
                                )
                            } else {
                                (
                                    0,
                                    atlas::Rect {
                                        x: 0,
                                        y: 0,
                                        width: 0,
                                        height: 0,
                                    },
                                )
                            }
                        });
                        let atlas = *atlas as u16;
                        let texture_x = rect.x as u16;
                        let texture_y = rect.y as u16;
                        let texture_w = rect.width as u16;
                        let texture_h = rect.height as u16;

                        if let Some(bound) = g.pixel_bounding_box() {
                            verts.push(TextVertex {
                                x: bound.min.x as f32,
                                y: bound.max.y as f32,
                                atlas,
                                texture_x,
                                texture_y,
                                texture_w,
                                texture_h,
                                _padding: 0,
                                ux: 0.0,
                                uy: 1.0,
                            });
                            verts.push(TextVertex {
                                x: bound.min.x as f32,
                                y: bound.min.y as f32,
                                atlas,
                                texture_x,
                                texture_y,
                                texture_w,
                                texture_h,
                                _padding: 0,
                                ux: 0.0,
                                uy: 0.0,
                            });
                            verts.push(TextVertex {
                                x: bound.max.x as f32,
                                y: bound.max.y as f32,
                                atlas,
                                texture_x,
                                texture_y,
                                texture_w,
                                texture_h,
                                _padding: 0,
                                ux: 1.0,
                                uy: 1.0,
                            });

                            verts.push(TextVertex {
                                x: bound.min.x as f32,
                                y: bound.min.y as f32,
                                atlas,
                                texture_x,
                                texture_y,
                                texture_w,
                                texture_h,
                                _padding: 0,
                                ux: 0.0,
                                uy: 0.0,
                            });
                            verts.push(TextVertex {
                                x: bound.max.x as f32,
                                y: bound.min.y as f32,
                                atlas,
                                texture_x,
                                texture_y,
                                texture_w,
                                texture_h,
                                _padding: 0,
                                ux: 1.0,
                                uy: 0.0,
                            });
                            verts.push(TextVertex {
                                x: bound.max.x as f32,
                                y: bound.max.y as f32,
                                atlas,
                                texture_x,
                                texture_y,
                                texture_w,
                                texture_h,
                                _padding: 0,
                                ux: 1.0,
                                uy: 1.0,
                            });

                            if let Some(&mut (ref info, ref mut shadow_verts)) =
                                shadow_verts.as_mut()
                            {
                                let bw = info.blur_radius / f32::from(texture_w);
                                let bh = info.blur_radius / f32::from(texture_h);
                                shadow_verts.push(TextVertex {
                                    x: bound.min.x as f32 - info.blur_radius + info.offset.0,
                                    y: bound.max.y as f32 + info.blur_radius + info.offset.1,
                                    atlas,
                                    texture_x,
                                    texture_y,
                                    texture_w,
                                    texture_h,
                                    _padding: 0,
                                    ux: -bw,
                                    uy: 1.0 + bh,
                                });
                                shadow_verts.push(TextVertex {
                                    x: bound.min.x as f32 - info.blur_radius + info.offset.0,
                                    y: bound.min.y as f32 - info.blur_radius + info.offset.1,
                                    atlas,
                                    texture_x,
                                    texture_y,
                                    texture_w,
                                    texture_h,
                                    _padding: 0,
                                    ux: -bw,
                                    uy: -bh,
                                });
                                shadow_verts.push(TextVertex {
                                    x: bound.max.x as f32 + info.blur_radius + info.offset.0,
                                    y: bound.max.y as f32 + info.blur_radius + info.offset.1,
                                    atlas,
                                    texture_x,
                                    texture_y,
                                    texture_w,
                                    texture_h,
                                    _padding: 0,
                                    ux: 1.0 + bw,
                                    uy: 1.0 + bh,
                                });

                                shadow_verts.push(TextVertex {
                                    x: bound.min.x as f32 - info.blur_radius + info.offset.0,
                                    y: bound.min.y as f32 - info.blur_radius + info.offset.1,
                                    atlas,
                                    texture_x,
                                    texture_y,
                                    texture_w,
                                    texture_h,
                                    _padding: 0,
                                    ux: -bw,
                                    uy: -bh,
                                });
                                shadow_verts.push(TextVertex {
                                    x: bound.max.x as f32 + info.blur_radius + info.offset.0,
                                    y: bound.min.y as f32 - info.blur_radius + info.offset.1,
                                    atlas,
                                    texture_x,
                                    texture_y,
                                    texture_w,
                                    texture_h,
                                    _padding: 0,
                                    ux: 1.0 + bw,
                                    uy: -bh,
                                });
                                shadow_verts.push(TextVertex {
                                    x: bound.max.x as f32 + info.blur_radius + info.offset.0,
                                    y: bound.max.y as f32 + info.blur_radius + info.offset.1,
                                    atlas,
                                    texture_x,
                                    texture_y,
                                    texture_w,
                                    texture_h,
                                    _padding: 0,
                                    ux: 1.0 + bw,
                                    uy: 1.0 + bh,
                                });
                            }
                        }

                        prev = Some(g.id());
                        line_offset += width;
                        line_offset = line_offset.ceil();
                    }
                }
                let (attrib_position, attrib_texture_info, attrib_atlas, attrib_uv) = {
                    let program = self.ctx.program("ui/text");
                    program.use_program();
                    (
                        assume!(self.log, program.attribute("attrib_position")),
                        assume!(self.log, program.attribute("attrib_texture_info")),
                        assume!(self.log, program.attribute("attrib_atlas")),
                        assume!(self.log, program.attribute("attrib_uv")),
                    )
                };

                let array = gl::VertexArray::new();
                array.bind();

                let buffer = gl::Buffer::new();
                buffer.bind(gl::BufferTarget::Array);

                attrib_position.enable();
                attrib_position.vertex_pointer(
                    2,
                    gl::Type::Float,
                    false,
                    mem::size_of::<TextVertex>() as i32,
                    0,
                );
                attrib_texture_info.enable();
                attrib_texture_info.vertex_pointer(
                    4,
                    gl::Type::UnsignedShort,
                    false,
                    mem::size_of::<TextVertex>() as i32,
                    8,
                );
                attrib_atlas.enable();
                attrib_atlas.vertex_int_pointer(
                    1,
                    gl::Type::UnsignedShort,
                    mem::size_of::<TextVertex>() as i32,
                    16,
                );
                attrib_uv.enable();
                attrib_uv.vertex_pointer(
                    2,
                    gl::Type::Float,
                    false,
                    mem::size_of::<TextVertex>() as i32,
                    20,
                );

                buffer.set_data(gl::BufferTarget::Array, &verts, gl::BufferUsage::Static);

                obj.ext.text_render = Some(TextRender {
                    array,
                    _buffer: buffer,
                    count: verts.len(),
                    color,
                    shadow: shadow_verts.map(|v| {
                        let array = gl::VertexArray::new();
                        array.bind();

                        let buffer = gl::Buffer::new();
                        buffer.bind(gl::BufferTarget::Array);

                        attrib_position.enable();
                        attrib_position.vertex_pointer(
                            2,
                            gl::Type::Float,
                            false,
                            mem::size_of::<TextVertex>() as i32,
                            0,
                        );
                        attrib_texture_info.enable();
                        attrib_texture_info.vertex_pointer(
                            4,
                            gl::Type::UnsignedShort,
                            false,
                            mem::size_of::<TextVertex>() as i32,
                            8,
                        );
                        attrib_atlas.enable();
                        attrib_atlas.vertex_int_pointer(
                            1,
                            gl::Type::UnsignedShort,
                            mem::size_of::<TextVertex>() as i32,
                            16,
                        );
                        attrib_uv.enable();
                        attrib_uv.vertex_pointer(
                            2,
                            gl::Type::Float,
                            false,
                            mem::size_of::<TextVertex>() as i32,
                            20,
                        );

                        buffer.set_data(gl::BufferTarget::Array, &v.1, gl::BufferUsage::Static);
                        TextShadowRender {
                            array,
                            _buffer: buffer,
                            count: v.1.len(),
                            color: v.0.color,
                            radius: v.0.blur_radius,
                        }
                    }),
                });
            }
        }

        if let Some(c) = obj
            .ext
            .background_color
            .filter(|_| obj.ext.box_render.is_none())
        {
            obj.ext.box_render = Some(self.solid_box(position, width, height, c));
        }

        let tint = obj.ext.tint;
        let tint = (
            (255.0 * tint.r) as u8,
            (255.0 * tint.g) as u8,
            (255.0 * tint.b) as u8,
            (255.0 * tint.a) as u8,
        );

        if let Some(img) = obj
            .ext
            .image
            .as_ref()
            .filter(|_| obj.ext.image_render.is_none())
        {
            let img = LazyResourceKey::parse(&img).or_module(ModuleKey::new("base"));
            // If the texture is dynamic then we need precreate it ready
            // for data to be loaded into it.
            if img.module() == "dynamic" && !self.global_atlas.textures.contains_key(&img) {
                use super::image::ImageFuture;
                let (width, height) = {
                    let mut parts = img.resource().split('@');
                    let width = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                    let height = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                    (width, height)
                };

                let info = super::RenderState::place_texture_atlas(
                    self.global_atlas,
                    ImageFuture::completed(width, height, vec![0; (width * height * 4) as usize]),
                );
                self.global_atlas
                    .textures
                    .insert(img.clone().into_owned(), info);
            }
            obj.ext.image_render = Some(self.image(position, width, height, tint, img));
        }

        if let (Some(border), Some(widths)) = (
            obj.ext
                .border
                .as_ref()
                .filter(|_| obj.ext.border_render.is_none()),
            obj.ext.border_width.as_ref(),
        ) {
            obj.ext.border_render = match border {
                border::Border::Image {
                    image,
                    width: iwidth,
                    height: iheight,
                    fill,
                } => Some(self.border_image(
                    position, width, height, tint, widths, image, *iwidth, *iheight, *fill,
                )),
                border::Border::Normal {
                    left,
                    top,
                    right,
                    bottom,
                } => Some(self.border(position, width, height, widths, left, top, right, bottom)),
            };
        }

        if obj.ext.shadow_render.is_empty() && !obj.ext.shadows.is_empty() {
            self.shadows(
                position,
                width,
                height,
                &obj.ext.shadows,
                &mut obj.ext.shadow_render,
            );
        }

        let view_matrix = &self.view_matrix;
        if let Some(b) = obj.ext.image_render.as_ref() {
            let program = self.ctx.program("ui/image");
            program.use_program();
            program
                .uniform("u_view_matrix")
                .map(|v| v.set_matrix4(&view_matrix));
            program
                .uniform("u_textures")
                .map(|v| v.set_int(super::GLOBAL_TEXTURE_LOCATION as i32));
            b.array.bind();
            gl::draw_arrays(gl::DrawType::Triangles, 0, b.count as _);
        }

        if let Some(b) = obj.ext.box_render.as_ref() {
            let program = self.ctx.program("ui/box");
            program.use_program();
            program
                .uniform("u_view_matrix")
                .map(|v| v.set_matrix4(&view_matrix));
            b.array.bind();
            gl::draw_arrays(gl::DrawType::Triangles, 0, b.count as _);
        }

        // Border
        if let Some(b) = obj.ext.border_render.as_ref() {
            let program = self.ctx.program(if b.image {
                "ui/border_image"
            } else {
                "ui/border_solid"
            });
            program.use_program();
            program
                .uniform("u_view_matrix")
                .map(|v| v.set_matrix4(&view_matrix));
            program
                .uniform("u_textures")
                .map(|v| v.set_int(super::GLOBAL_TEXTURE_LOCATION as i32));
            b.array.bind();
            gl::draw_arrays(gl::DrawType::Triangles, 0, b.count as _);
        }

        for shadow in &obj.ext.shadow_render {
            let program = self.ctx.program(if shadow.inset {
                "ui/box_shadow_inner"
            } else {
                "ui/box_shadow"
            });
            program.use_program();
            program
                .uniform("u_view_matrix")
                .map(|v| v.set_matrix4(&view_matrix));
            shadow.array.bind();
            shadow.u_buffer.1.bind(gl::BufferTarget::Uniform);
            shadow.u_buffer.1.bind_uniform_block(shadow.u_buffer.0);
            gl::draw_arrays(gl::DrawType::Triangles, 0, shadow.count as _);
        }

        // Text
        if let Some(b) = obj.ext.text_render.as_ref() {
            if let Some(shadow) = b.shadow.as_ref() {
                let program = self.ctx.program("ui/text_shadow");
                program.use_program();
                program
                    .uniform("u_view_matrix")
                    .map(|v| v.set_matrix4(&view_matrix));
                program.uniform("u_textures").map(|v| v.set_int(1));
                program.uniform("u_color").map(|v| {
                    v.set_float4(
                        shadow.color.r,
                        shadow.color.g,
                        shadow.color.b,
                        shadow.color.a,
                    )
                });
                program
                    .uniform("u_blur_radius")
                    .map(|v| v.set_float(shadow.radius));
                shadow.array.bind();
                gl::draw_arrays(gl::DrawType::Triangles, 0, shadow.count as _);
            }
            let program = self.ctx.program("ui/text");
            program.use_program();
            program
                .uniform("u_view_matrix")
                .map(|v| v.set_matrix4(&view_matrix));
            program.uniform("u_textures").map(|v| v.set_int(1));
            program
                .uniform("u_color")
                .map(|v| v.set_float4(b.color.r, b.color.g, b.color.b, b.color.a));
            b.array.bind();
            gl::draw_arrays(gl::DrawType::Triangles, 0, b.count as _);
        }

        if obj.clip_overflow {
            let (mut cx, mut cy, mut cw, mut ch) = (position.0, position.1, width, height);
            if let Some(last) = self.clips.last() {
                if cx < last.0 {
                    cw = (cw - (last.0 - cx)).max(0.0);
                    cx = last.0;
                }
                if cy < last.1 {
                    ch = (ch - (last.1 - cy)).max(0.0);
                    cy = last.1;
                }
                if cx + cw > last.0 + last.2 {
                    cw = last.2 - (cx - last.0);
                }
                if cy + ch > last.1 + last.3 {
                    ch = last.3 - (cy - last.1);
                }
            }
            self.clips.push((cx, cy, cw, ch));
            do_clip(self.ctx, &view_matrix, self.clip_array, cx, cy, cw, ch);
        }

        self.offset.push((
            position.0 + obj.scroll_position.0 as f32,
            position.1 + obj.scroll_position.1 as f32,
        ));
    }
    fn visit_end(&mut self, obj: &mut NodeInner<UniverCityUI>) {
        self.offset.pop();
        if obj.clip_overflow {
            self.clips.pop();
            if let Some(last) = self.clips.last() {
                do_clip(
                    self.ctx,
                    &self.view_matrix,
                    self.clip_array,
                    last.0,
                    last.1,
                    last.2,
                    last.3,
                );
            }
        }
    }
}

impl<'a, 'b> Builder<'a, 'b> {
    fn solid_box(
        &mut self,
        position: (f32, f32),
        width: f32,
        height: f32,
        c: color::Color,
    ) -> BoxRender {
        let (attrib_position, attrib_color) = {
            let program = self.ctx.program("ui/box");
            program.use_program();
            (
                assume!(self.log, program.attribute("attrib_position")),
                assume!(self.log, program.attribute("attrib_color")),
            )
        };

        let array = gl::VertexArray::new();
        array.bind();

        let buffer = gl::Buffer::new();
        buffer.bind(gl::BufferTarget::Array);

        attrib_position.enable();
        attrib_position.vertex_pointer(
            2,
            gl::Type::Float,
            false,
            mem::size_of::<BoxVertex>() as i32,
            0,
        );
        attrib_color.enable();
        attrib_color.vertex_pointer(
            4,
            gl::Type::UnsignedByte,
            true,
            mem::size_of::<BoxVertex>() as i32,
            8,
        );

        let r = (c.r * 255.0) as u8;
        let g = (c.g * 255.0) as u8;
        let b = (c.b * 255.0) as u8;
        let a = (c.a * 255.0) as u8;

        let verts = &[
            BoxVertex {
                x: position.0,
                y: position.1 + height,
                r,
                g,
                b,
                a,
            },
            BoxVertex {
                x: position.0,
                y: position.1,
                r,
                g,
                b,
                a,
            },
            BoxVertex {
                x: position.0 + width,
                y: position.1 + height,
                r,
                g,
                b,
                a,
            },
            BoxVertex {
                x: position.0,
                y: position.1,
                r,
                g,
                b,
                a,
            },
            BoxVertex {
                x: position.0 + width,
                y: position.1,
                r,
                g,
                b,
                a,
            },
            BoxVertex {
                x: position.0 + width,
                y: position.1 + height,
                r,
                g,
                b,
                a,
            },
        ];

        buffer.set_data(gl::BufferTarget::Array, verts, gl::BufferUsage::Static);
        BoxRender {
            array: array,
            _buffer: buffer,
            count: 6,
        }
    }

    fn shadows(
        &mut self,
        position: (f32, f32),
        width: f32,
        height: f32,
        shadows: &[shadow::Shadow],
        output: &mut Vec<ShadowRender>,
    ) {
        for shadow in shadows {
            let (attrib_position, attrib_color, shadow_block) = {
                let program = self.ctx.program(if shadow.inset {
                    "ui/box_shadow_inner"
                } else {
                    "ui/box_shadow"
                });
                program.use_program();
                (
                    assume!(self.log, program.attribute("attrib_position")),
                    assume!(self.log, program.attribute("attrib_color")),
                    program.uniform_block("ShadowInfo"),
                )
            };

            let array = gl::VertexArray::new();
            array.bind();

            let buffer = gl::Buffer::new();
            buffer.bind(gl::BufferTarget::Array);

            attrib_position.enable();
            attrib_position.vertex_pointer(
                2,
                gl::Type::Float,
                false,
                mem::size_of::<BoxVertex>() as i32,
                0,
            );
            attrib_color.enable();
            attrib_color.vertex_pointer(
                4,
                gl::Type::UnsignedByte,
                true,
                mem::size_of::<BoxVertex>() as i32,
                8,
            );

            let r = (shadow.color.r * 255.0) as u8;
            let g = (shadow.color.g * 255.0) as u8;
            let b = (shadow.color.b * 255.0) as u8;
            let a = (shadow.color.a * 255.0) as u8;

            let shadow_pos = (
                position.0 + shadow.offset.0 - shadow.spread_radius - shadow.blur_radius,
                position.1 + shadow.offset.1 - shadow.spread_radius - shadow.blur_radius,
                width + shadow.spread_radius * 2.0 + shadow.blur_radius * 2.0,
                height + shadow.spread_radius * 2.0 + shadow.blur_radius * 2.0,
            );

            let verts = &[
                BoxVertex {
                    x: shadow_pos.0,
                    y: shadow_pos.1 + shadow_pos.3,
                    r,
                    g,
                    b,
                    a,
                },
                BoxVertex {
                    x: shadow_pos.0,
                    y: shadow_pos.1,
                    r,
                    g,
                    b,
                    a,
                },
                BoxVertex {
                    x: shadow_pos.0 + shadow_pos.2,
                    y: shadow_pos.1 + shadow_pos.3,
                    r,
                    g,
                    b,
                    a,
                },
                BoxVertex {
                    x: shadow_pos.0,
                    y: shadow_pos.1,
                    r,
                    g,
                    b,
                    a,
                },
                BoxVertex {
                    x: shadow_pos.0 + shadow_pos.2,
                    y: shadow_pos.1,
                    r,
                    g,
                    b,
                    a,
                },
                BoxVertex {
                    x: shadow_pos.0 + shadow_pos.2,
                    y: shadow_pos.1 + shadow_pos.3,
                    r,
                    g,
                    b,
                    a,
                },
            ];
            buffer.set_data(gl::BufferTarget::Array, verts, gl::BufferUsage::Static);

            #[repr(C, packed)]
            struct ShadowInfo {
                box_position: (f32, f32, f32, f32),
                shift_position: (f32, f32, f32, f32),
                shadow_params: (f32, f32, f32, f32),
            }

            let u_buffer = gl::Buffer::new();
            u_buffer.bind(gl::BufferTarget::Uniform);
            let info = ShadowInfo {
                box_position: (position.0, position.1, width, height),
                shift_position: (
                    position.0 + shadow.offset.0,
                    position.1 + shadow.offset.1,
                    width,
                    height,
                ),
                shadow_params: (shadow.spread_radius, shadow.blur_radius, 0.0, 0.0),
            };
            u_buffer.set_data(gl::BufferTarget::Uniform, &[info], gl::BufferUsage::Static);
            u_buffer.bind_uniform_block(shadow_block);

            output.push(ShadowRender {
                array: array,
                _buffer: buffer,
                u_buffer: (shadow_block, u_buffer),
                count: 6,
                inset: shadow.inset,
            });
        }
    }

    fn image(
        &mut self,
        position: (f32, f32),
        width: f32,
        height: f32,
        tint: (u8, u8, u8, u8),
        img: ResourceKey<'_>,
    ) -> ImageRender {
        let (attrib_position, attrib_texture_info, attrib_atlas, attrib_uv, attrib_color) = {
            let program = self.ctx.program("ui/image");
            program.use_program();
            (
                assume!(self.log, program.attribute("attrib_position")),
                assume!(self.log, program.attribute("attrib_texture_info")),
                assume!(self.log, program.attribute("attrib_atlas")),
                assume!(self.log, program.attribute("attrib_uv")),
                assume!(self.log, program.attribute("attrib_color")),
            )
        };

        let array = gl::VertexArray::new();
        array.bind();

        let buffer = gl::Buffer::new();
        buffer.bind(gl::BufferTarget::Array);

        attrib_position.enable();
        attrib_position.vertex_pointer(
            2,
            gl::Type::Float,
            false,
            mem::size_of::<ImageVertex>() as i32,
            0,
        );
        attrib_texture_info.enable();
        attrib_texture_info.vertex_pointer(
            4,
            gl::Type::UnsignedShort,
            false,
            mem::size_of::<ImageVertex>() as i32,
            8,
        );
        attrib_atlas.enable();
        attrib_atlas.vertex_int_pointer(
            1,
            gl::Type::UnsignedShort,
            mem::size_of::<ImageVertex>() as i32,
            16,
        );
        attrib_uv.enable();
        attrib_uv.vertex_pointer(
            2,
            gl::Type::Float,
            false,
            mem::size_of::<ImageVertex>() as i32,
            20,
        );
        attrib_color.enable();
        attrib_color.vertex_pointer(
            4,
            gl::Type::UnsignedByte,
            true,
            mem::size_of::<ImageVertex>() as i32,
            28,
        );

        let (atlas, rect) =
            super::RenderState::texture_info_for(self.log, self.assets, self.global_atlas, img);

        let atlas = atlas as u16;
        let texture_x = rect.x as u16;
        let texture_y = rect.y as u16;
        let texture_w = rect.width as u16;
        let texture_h = rect.height as u16;

        let verts = &[
            ImageVertex {
                x: position.0,
                y: position.1 + height,
                atlas,
                texture_x,
                texture_y,
                texture_w,
                texture_h,
                _padding: 0,
                ux: 0.0,
                uy: 1.0,
                r: tint.0,
                g: tint.1,
                b: tint.2,
                a: tint.3,
            },
            ImageVertex {
                x: position.0,
                y: position.1,
                atlas,
                texture_x,
                texture_y,
                texture_w,
                texture_h,
                _padding: 0,
                ux: 0.0,
                uy: 0.0,
                r: tint.0,
                g: tint.1,
                b: tint.2,
                a: tint.3,
            },
            ImageVertex {
                x: position.0 + width,
                y: position.1 + height,
                atlas,
                texture_x,
                texture_y,
                texture_w,
                texture_h,
                _padding: 0,
                ux: 1.0,
                uy: 1.0,
                r: tint.0,
                g: tint.1,
                b: tint.2,
                a: tint.3,
            },
            ImageVertex {
                x: position.0,
                y: position.1,
                atlas,
                texture_x,
                texture_y,
                texture_w,
                texture_h,
                _padding: 0,
                ux: 0.0,
                uy: 0.0,
                r: tint.0,
                g: tint.1,
                b: tint.2,
                a: tint.3,
            },
            ImageVertex {
                x: position.0 + width,
                y: position.1,
                atlas,
                texture_x,
                texture_y,
                texture_w,
                texture_h,
                _padding: 0,
                ux: 1.0,
                uy: 0.0,
                r: tint.0,
                g: tint.1,
                b: tint.2,
                a: tint.3,
            },
            ImageVertex {
                x: position.0 + width,
                y: position.1 + height,
                atlas,
                texture_x,
                texture_y,
                texture_w,
                texture_h,
                _padding: 0,
                ux: 1.0,
                uy: 1.0,
                r: tint.0,
                g: tint.1,
                b: tint.2,
                a: tint.3,
            },
        ];

        buffer.set_data(gl::BufferTarget::Array, verts, gl::BufferUsage::Static);
        ImageRender {
            array: array,
            _buffer: buffer,
            count: 6,
        }
    }
}

fn do_clip(
    ctx: &mut pipeline::Context<'_>,
    view_matrix: &cgmath::Matrix4<f32>,
    vao: &gl::VertexArray,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    // Mark everywhere as invalid
    gl::stencil_mask(0xFF);
    gl::clear_stencil(0);
    gl::clear(gl::BufferBit::STENCIL);
    // Draw the clip without color
    gl::color_mask(false, false, false, false);

    gl::stencil_func(gl::Func::Always, 1, 0xFF);
    gl::stencil_op(
        gl::StencilOp::Replace,
        gl::StencilOp::Replace,
        gl::StencilOp::Replace,
    );

    let prog = ctx.program("ui/clip");
    prog.use_program();
    prog.uniform("u_view_matrix")
        .map(|v| v.set_matrix4(view_matrix));
    prog.uniform("u_clip_position")
        .map(|v| v.set_float4(x, y, w, h));
    vao.bind();
    gl::draw_arrays(gl::DrawType::Triangles, 0, 6);

    // Limit drawing to masked region
    gl::stencil_mask(0x00);
    gl::stencil_func(gl::Func::Equal, 1, 0xFF);
    gl::stencil_op(
        gl::StencilOp::Keep,
        gl::StencilOp::Keep,
        gl::StencilOp::Keep,
    );
    gl::color_mask(true, true, true, true);
}

pub fn deg<'a>(
    params: &mut (dyn Iterator<Item = FResult<'a, ui::Value>> + 'a),
) -> FResult<'a, ui::Value> {
    let val = params
        .next()
        .ok_or(Error::MissingParameter {
            position: 0,
            name: "degrees",
        })
        .and_then(|v| v)?;

    if let Some(d) = val.convert_ref::<i32>() {
        Ok(Value::Float((f64::from(*d)).to_radians()))
    } else if let Some(d) = val.convert_ref::<f64>() {
        Ok(Value::Float((*d).to_radians()))
    } else {
        Err(Error::CustomStatic {
            reason: "Expected integer or float",
        })
    }
}

struct Lined {
    assets: AssetManager,
    fonts: Rc<RefCell<FNVMap<String, Font>>>,

    line_height: i32,
    recompute: bool,

    line: i32,
    remaining: i32,
    width: i32,

    bounded_size: Rect,
    word_wrap: bool,
}

impl Lined {
    fn new(assets: AssetManager, fonts: Rc<RefCell<FNVMap<String, Font>>>) -> Lined {
        Lined {
            assets,
            fonts,

            line_height: 16,
            recompute: true,

            line: 0,
            remaining: 0,
            width: 0,

            bounded_size: Default::default(),
            word_wrap: true,
        }
    }
}

struct LinedChild {
    planned: Rect,
    width: Option<i32>,
    height: Option<i32>,
}

static LINE_HEIGHT: StaticKey = StaticKey("line_height");
static WORD_WRAP: StaticKey = StaticKey("word_wrap");

impl LayoutEngine<UniverCityUI> for Lined {
    type ChildData = LinedChild;
    fn name() -> &'static str {
        "lined"
    }

    fn style_properties<'a, F>(mut prop: F)
    where
        F: FnMut(StaticKey) + 'a,
    {
        prop(LINE_HEIGHT);
        prop(WORD_WRAP);
    }

    fn new_child_data() -> Self::ChildData {
        LinedChild {
            planned: Default::default(),
            width: None,
            height: None,
        }
    }
    fn update_data(
        &mut self,
        styles: &Styles<UniverCityUI>,
        nc: &NodeChain<'_, UniverCityUI>,
        rule: &Rule<UniverCityUI>,
    ) -> DirtyFlags {
        let mut flags = DirtyFlags::empty();
        eval!(styles, nc, rule.LINE_HEIGHT => val => {
            let new = val.convert().unwrap_or(16);
            if self.line_height != new {
                self.line_height = new;
                flags |= DirtyFlags::SIZE;
                self.recompute = true;
            }
        });
        eval!(styles, nc, rule.WORD_WRAP => val => {
            let new = val.convert().unwrap_or(true);
            if self.word_wrap != new {
                self.word_wrap = new;
                flags |= DirtyFlags::SIZE;
                self.recompute = true;
            }
        });
        flags
    }
    fn update_child_data(
        &mut self,
        styles: &Styles<UniverCityUI>,
        nc: &NodeChain<'_, UniverCityUI>,
        rule: &Rule<UniverCityUI>,
        data: &mut Self::ChildData,
    ) -> DirtyFlags {
        let mut flags = DirtyFlags::empty();
        eval!(styles, nc, rule.WIDTH => val => {
            let new = val.convert();
            if data.width != new {
                data.width = new;
                flags |= DirtyFlags::SIZE;
                self.recompute = true;
            }
        });
        eval!(styles, nc, rule.HEIGHT => val => {
            let new = val.convert();
            if data.height != new {
                data.height = new;
                flags |= DirtyFlags::SIZE;
                self.recompute = true;
            }
        });
        flags
    }

    fn reset_unset_data(&mut self, used_keys: &FnvHashSet<StaticKey>) -> DirtyFlags {
        let mut flags = DirtyFlags::empty();
        if !used_keys.contains(&LINE_HEIGHT) && self.line_height != 16 {
            self.line_height = 16;
            self.recompute = true;
            flags |= DirtyFlags::SIZE;
        }
        if !used_keys.contains(&WORD_WRAP) && !self.word_wrap {
            self.word_wrap = true;
            self.recompute = true;
            flags |= DirtyFlags::SIZE;
        }
        flags
    }

    fn reset_unset_child_data(
        &mut self,
        _used_keys: &FnvHashSet<StaticKey>,
        _data: &mut Self::ChildData,
    ) -> DirtyFlags {
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
    fn check_child_flags(&mut self, flags: DirtyFlags) -> DirtyFlags {
        if flags.contains(DirtyFlags::SIZE)
            || flags.contains(ui::FONT_FLAG)
            || flags.contains(DirtyFlags::TEXT)
        {
            DirtyFlags::LAYOUT_1
        } else {
            DirtyFlags::empty()
        }
    }

    fn start_layout(
        &mut self,
        _ext: &mut ui::NodeData,
        current: Rect,
        flags: DirtyFlags,
        children: ChildAccess<'_, Self, UniverCityUI>,
    ) -> Rect {
        if self.recompute
            || flags.contains(DirtyFlags::SIZE)
            || flags.contains(DirtyFlags::LAYOUT)
            || flags.contains(DirtyFlags::LAYOUT_1)
            || flags.contains(DirtyFlags::CHILDREN)
        {
            self.recompute = true;

            self.line = 0;
            self.remaining = current.width;
            self.width = current.width;

            for i in 0..children.len() {
                let (_, _, mut c) = children.get(i).expect("Missing child");
                let (value, c) = c.split();
                if value.text().is_some() {
                    continue;
                }

                c.planned = Rect {
                    x: 0,
                    y: 0,
                    width: current.width,
                    height: self.line_height,
                };
            }
        }

        current
    }
    fn finish_layout(
        &mut self,
        _ext: &mut ui::NodeData,
        current: Rect,
        _flags: DirtyFlags,
        children: ChildAccess<'_, Self, UniverCityUI>,
    ) -> Rect {
        use std::cmp::max;
        if self.recompute {
            self.bounded_size = current;
            self.bounded_size.width = 0;
            self.bounded_size.height = 0;

            for i in 0..children.len() {
                let (r, _, _) = children.get(i).expect("Missing child");
                self.bounded_size.width = max(self.bounded_size.width, r.x + r.width);
                self.bounded_size.height = (self.line + 1) * self.line_height;
            }
        }
        self.recompute = false;
        self.bounded_size.x = current.x;
        self.bounded_size.y = current.y;
        self.bounded_size
    }
    fn do_layout(
        &mut self,
        _value: &NodeValue<UniverCityUI>,
        _ext: &mut ui::NodeData,
        data: &mut Self::ChildData,
        _current: Rect,
        _flags: DirtyFlags,
    ) -> Rect {
        let mut r = data.planned;
        if self.recompute {
            data.width.map(|v| r.width = v);
            data.height.map(|v| r.height = v);
        }
        r
    }
    fn do_layout_end(
        &mut self,
        value: &NodeValue<UniverCityUI>,
        ext: &mut ui::NodeData,
        data: &mut Self::ChildData,
        mut current: Rect,
        _flags: DirtyFlags,
    ) -> Rect {
        use std::cmp;

        if self.recompute {
            if let Some(txt) = value.text() {
                let fonts = &mut *self.fonts.borrow_mut();
                if let Some(font) = ext
                    .font
                    .as_ref()
                    .and_then(|font| load_font(&self.assets, fonts, font))
                {
                    let size = ext.font_size;
                    ext.text_splits.clear();
                    if size <= 0.0 {
                        return current;
                    }
                    let scale = rusttype::Scale::uniform(size);

                    let mut word = (0, 0);
                    let mut word_size = 0.0;
                    let mut current = (0, 0);
                    let mut current_size = 0.0;
                    let mut prev = None;
                    for (idx, c) in txt.char_indices() {
                        if c.is_whitespace() {
                            current_size += word_size;
                            word_size = 0.0;
                            current.1 = idx;
                            word.0 = idx;
                        }
                        word.1 = idx;
                        let g = font.font.glyph(c).scaled(scale);

                        let offset = if let Some(last) = prev {
                            font.font.pair_kerning(scale, last, g.id())
                        } else {
                            0.0
                        };

                        let size = offset + g.h_metrics().advance_width;
                        prev = Some(g.id());

                        if ((current_size + word_size + size).ceil() as i32 > self.remaining
                            && current.0 != current.1
                            && self.word_wrap)
                            || c == '\n'
                        {
                            prev = None;
                            // Split at word
                            ext.text_splits.push((
                                current.0,
                                current.1,
                                Rect {
                                    x: self.width - self.remaining,
                                    y: self.line * self.line_height,
                                    width: current_size.ceil() as i32,
                                    height: self.line_height,
                                },
                            ));
                            current.0 = word.0;
                            current.1 = word.0;
                            current_size = 0.0;
                            self.remaining = self.width;
                            self.line += 1;

                            if !c.is_whitespace() {
                                word_size += g.h_metrics().advance_width;
                            } else {
                                current.0 += c.len_utf8();
                                current.1 += c.len_utf8();
                            }
                        } else {
                            word_size += size;
                            word_size = word_size.ceil();
                        }
                    }
                    // Add the remaining
                    current.1 = txt.len();
                    current_size += word_size;
                    let width = current_size.ceil() as i32;
                    ext.text_splits.push((
                        current.0,
                        current.1,
                        Rect {
                            x: self.width - self.remaining,
                            y: self.line * self.line_height,
                            width: width,
                            height: self.line_height,
                        },
                    ));
                    self.remaining -= width;

                    let mut min = (i32::max_value(), i32::max_value());
                    let mut max = (0, 0);
                    for split in &ext.text_splits {
                        min.0 = cmp::min(min.0, split.2.x);
                        min.1 = cmp::min(min.1, split.2.y);
                        max.0 = cmp::max(max.0, split.2.x + split.2.width);
                        max.1 = cmp::max(max.1, split.2.y + split.2.height);
                    }
                    data.planned = Rect {
                        x: min.0,
                        y: min.1,
                        width: max.0 - min.0,
                        height: max.1 - min.1,
                    };
                    return data.planned;
                }
            } else {
                if self.remaining < current.width {
                    self.line += 1;
                    self.remaining = self.width;
                }
                current.x = self.width - self.remaining;
                current.y = self.line * self.line_height + (self.line_height - current.height) / 2;

                self.remaining -= current.width;
                data.planned = current;
            }
        }
        current
    }
}
