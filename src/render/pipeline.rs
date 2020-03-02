#![allow(dead_code, missing_docs)]

use super::gl;
use crate::prelude::*;
use std::any::Any;
use std::borrow::Cow;
use std::cell::Cell;
use std::rc::Rc;

pub struct Pipeline<Flag> {
    log: Logger,
    assets: AssetManager,
    global_scale: f32,

    programs: FNVMap<&'static str, Program>,
    passes: Vec<Pass<Flag>>,
    attachments: Vec<InternalAttachment>,
    final_color_a: InternalAttachment,
    final_color_b: InternalAttachment,

    quad_vao: gl::VertexArray,
    _quad_buffer: gl::Buffer,
}

pub struct PipelineBuilder<Flag> {
    log: Logger,
    assets: AssetManager,
    global_scale: f32,

    programs: FNVMap<&'static str, Program>,
    passes: Vec<(&'static str, PassBuilder<Flag>)>,

    quad_vao: gl::VertexArray,
    _quad_buffer: gl::Buffer,
}

impl<Flag> Pipeline<Flag> {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(log: &Logger, assets: AssetManager, global_scale: f32) -> PipelineBuilder<Flag> {
        let log = log.new(o!("source" => "pipeline"));
        let vao = gl::VertexArray::new();
        vao.bind();
        let buf = gl::Buffer::new();
        buf.bind(gl::BufferTarget::Array);
        buf.set_data(
            gl::BufferTarget::Array,
            &[
                -1.0f32, 1.0, 1.0, 1.0, 1.0, -1.0, -1.0, 1.0, 1.0, -1.0, -1.0, -1.0,
            ],
            gl::BufferUsage::Static,
        );

        let a = gl::Attribute::new(0);
        a.enable();
        a.vertex_pointer(2, gl::Type::Float, false, 8, 0);

        PipelineBuilder {
            log,
            assets,
            global_scale,
            programs: FNVMap::default(),
            passes: Vec::new(),

            quad_vao: vao,
            _quad_buffer: buf,
        }
    }

    pub fn context(&mut self) -> Context<'_> {
        Context {
            log: &self.log,
            programs: &mut self.programs,
            vars: FNVMap::default(),
            replacements: None,
        }
    }

    pub fn get_program(&mut self, name: &'static str) -> &mut Program {
        assume!(self.log, self.programs.get_mut(name))
    }

    pub fn begin_draw(&mut self) -> DrawSetup<'_, Flag> {
        DrawSetup {
            pipeline: self,
            vars: FNVMap::default(),
            pass_vars: FNVMap::default(),
        }
    }

    /// Clears most of the pipeline's resources
    pub fn clear(&mut self) {
        self.programs.clear();
        self.passes.clear();
    }
}

impl<Flag> PipelineBuilder<Flag>
where
    Flag: Clone,
{
    pub fn build(self) -> Pipeline<Flag> {
        let mut attachments: Vec<(usize, AInfo, InternalAttachment)> = vec![];
        let mut passes = vec![];

        #[derive(PartialEq, Clone, Debug)]
        struct AInfo {
            size: Option<(u32, u32)>,
            linear: bool,
            ty: Type,
            components: Components,
            compare_ref: bool,
            border_color: Option<[f32; 4]>,
        }

        // The base color textures
        let tex = gl::Texture::new();
        tex.bind(gl::TextureTarget::Texture2D);

        let current_size = Some((512, 512));
        let (w, h) = (512, 512);
        init_attachment(w, h, &tex, Type::U8, Components::RGB);

        tex.set_parameter::<gl::TextureMinFilter>(
            gl::TextureTarget::Texture2D,
            gl::TextureFilter::Linear,
        );
        tex.set_parameter::<gl::TextureMagFilter>(
            gl::TextureTarget::Texture2D,
            gl::TextureFilter::Linear,
        );
        tex.set_parameter::<gl::TextureWrapS>(
            gl::TextureTarget::Texture2D,
            gl::TextureWrap::ClampToBorder,
        );
        tex.set_parameter::<gl::TextureWrapT>(
            gl::TextureTarget::Texture2D,
            gl::TextureWrap::ClampToBorder,
        );
        tex.set_parameter::<gl::TextureBaseLevel>(gl::TextureTarget::Texture2D, 0);
        tex.set_parameter::<gl::TextureMaxLevel>(gl::TextureTarget::Texture2D, 0);

        let final_color_a = InternalAttachment {
            texture: tex,
            current_size: Cell::new(current_size),
            size: None,
            scale: 1,
            ty: Type::U8,
            components: Components::RGB,
        };
        let tex = gl::Texture::new();
        tex.bind(gl::TextureTarget::Texture2D);

        let current_size = Some((512, 512));
        let (w, h) = (512, 512);
        init_attachment(w, h, &tex, Type::U8, Components::RGB);

        tex.set_parameter::<gl::TextureMinFilter>(
            gl::TextureTarget::Texture2D,
            gl::TextureFilter::Linear,
        );
        tex.set_parameter::<gl::TextureMagFilter>(
            gl::TextureTarget::Texture2D,
            gl::TextureFilter::Linear,
        );
        tex.set_parameter::<gl::TextureWrapS>(
            gl::TextureTarget::Texture2D,
            gl::TextureWrap::ClampToBorder,
        );
        tex.set_parameter::<gl::TextureWrapT>(
            gl::TextureTarget::Texture2D,
            gl::TextureWrap::ClampToBorder,
        );
        tex.set_parameter::<gl::TextureBaseLevel>(gl::TextureTarget::Texture2D, 0);
        tex.set_parameter::<gl::TextureMaxLevel>(gl::TextureTarget::Texture2D, 0);

        let final_color_b = InternalAttachment {
            texture: tex,
            current_size: Cell::new(current_size),
            size: None,
            scale: 1,
            ty: Type::U8,
            components: Components::RGB,
        };

        for (idx, &(name, ref pass)) in self.passes.iter().enumerate() {
            let mut local_attachments = vec![];
            let mut local_names = FNVMap::default();

            let framebuffer = if pass.attachments.is_empty() && !pass.final_color {
                None
            } else {
                let framebuffer = gl::Framebuffer::new();
                framebuffer.bind(gl::TargetFramebuffer::Both);
                let mut color = if pass.final_color { 1 } else { 0 };

                'attachments: for &(attach_name, ref attachment) in &pass.attachments {
                    let a_target = if let AttachTarget::Depth = attachment.target {
                        if let Components::Depth24Stencil8 = attachment.components {
                            gl::Attachment::DepthStencil
                        } else {
                            gl::Attachment::Depth
                        }
                    } else {
                        let c = color;
                        color += 1;
                        match c {
                            0 => gl::Attachment::Color0,
                            1 => gl::Attachment::Color1,
                            2 => gl::Attachment::Color2,
                            3 => gl::Attachment::Color3,
                            4 => gl::Attachment::Color4,
                            _ => panic!("Out of color attachments"),
                        }
                    };
                    let info = AInfo {
                        size: attachment.size,
                        linear: attachment.linear,
                        ty: attachment.ty,
                        components: attachment.components,
                        compare_ref: attachment.compare_ref,
                        border_color: attachment.border_color,
                    };

                    let mut last_use = idx;
                    for (idx, &(_name, ref pass)) in self.passes[idx + 1..]
                        .iter()
                        .enumerate()
                        .map(|(i, p)| (i + idx + 1, p))
                    {
                        if let Some(full) = pass.fullscreen.as_ref() {
                            for input in &full.inputs {
                                if input.pass == name && input.name == attach_name {
                                    last_use = idx;
                                }
                            }
                        }
                        for bind in &pass.bind_texture {
                            if bind.0 == name && bind.1 == attach_name {
                                last_use = idx;
                            }
                        }
                    }

                    for (offset, a) in attachments.iter_mut().enumerate() {
                        if a.0 < idx && a.1 == info {
                            local_attachments.push(Attachment {
                                target: attachment.target,
                                clear_value: attachment.clear_value,
                            });
                            a.2.texture.bind(gl::TextureTarget::Texture2D);
                            framebuffer.texture_2d(
                                gl::TargetFramebuffer::Both,
                                a_target,
                                gl::TextureTarget::Texture2D,
                                &a.2.texture,
                                0,
                            );
                            local_names.insert(attach_name, offset);
                            a.0 = last_use; // Prevent others from re-using this until we are done
                            continue 'attachments;
                        }
                    }

                    let tex = gl::Texture::new();
                    tex.bind(gl::TextureTarget::Texture2D);

                    let current_size;
                    let (w, h) = if let Some(size) = attachment.size {
                        current_size = None;
                        size
                    } else {
                        current_size = Some((512, 512));
                        (512, 512)
                    };
                    init_attachment(w, h, &tex, attachment.ty, attachment.components);

                    let filter = if attachment.linear {
                        gl::TextureFilter::Linear
                    } else {
                        gl::TextureFilter::Nearest
                    };
                    tex.set_parameter::<gl::TextureMinFilter>(gl::TextureTarget::Texture2D, filter);
                    tex.set_parameter::<gl::TextureMagFilter>(gl::TextureTarget::Texture2D, filter);
                    tex.set_parameter::<gl::TextureWrapS>(
                        gl::TextureTarget::Texture2D,
                        gl::TextureWrap::ClampToBorder,
                    );
                    tex.set_parameter::<gl::TextureWrapT>(
                        gl::TextureTarget::Texture2D,
                        gl::TextureWrap::ClampToBorder,
                    );
                    tex.set_parameter::<gl::TextureBaseLevel>(gl::TextureTarget::Texture2D, 0);
                    tex.set_parameter::<gl::TextureMaxLevel>(gl::TextureTarget::Texture2D, 0);
                    if let Some(border) = attachment.border_color {
                        tex.set_parameter::<gl::TextureBorderColor>(
                            gl::TextureTarget::Texture2D,
                            border,
                        );
                    }
                    if attachment.compare_ref {
                        tex.set_parameter::<gl::TextureCompareMode>(
                            gl::TextureTarget::Texture2D,
                            gl::CompareMode::CompareRefToTexture,
                        );
                        tex.set_parameter::<gl::TextureCompareFunc>(
                            gl::TextureTarget::Texture2D,
                            gl::Func::GreaterOrEqual,
                        );
                    }

                    local_names.insert(attach_name, attachments.len());
                    let in_attachment = InternalAttachment {
                        texture: tex,
                        current_size: Cell::new(current_size),
                        size: attachment.size,
                        scale: attachment.scale,
                        ty: attachment.ty,
                        components: attachment.components,
                    };
                    local_attachments.push(Attachment {
                        target: attachment.target,
                        clear_value: attachment.clear_value,
                    });
                    framebuffer.texture_2d(
                        gl::TargetFramebuffer::Both,
                        a_target,
                        gl::TextureTarget::Texture2D,
                        &in_attachment.texture,
                        0,
                    );
                    attachments.push((last_use, info, in_attachment));
                }

                // Optimization
                if color == 0 {
                    gl::draw_buffer(gl::Mode::None);
                    gl::read_buffer(gl::Mode::None);
                }

                gl::Framebuffer::unbind(gl::TargetFramebuffer::Both);
                Some(framebuffer)
            };
            passes.push(Pass {
                name,
                runtime_enable: pass.runtime_enable,
                fullscreen: pass.fullscreen.clone(),
                flag: pass.flag.clone(),
                framebuffer,
                final_color: pass.final_color,
                attachments: local_attachments,
                attachment_names: local_names,
                replacements: pass.replacements.clone(),
                size: pass.size,
                scale: pass.scale,
                bind_texture: pass.bind_texture.clone(),
                clear_flags: pass.clear_flags,
            });
        }

        Pipeline {
            log: self.log,
            assets: self.assets,
            global_scale: self.global_scale,
            programs: self.programs,
            passes,
            attachments: attachments.into_iter().map(|v| v.2).collect(),
            final_color_a: final_color_a,
            final_color_b: final_color_b,
            quad_vao: self.quad_vao,
            _quad_buffer: self._quad_buffer,
        }
    }

    pub fn program<D>(mut self, name: &'static str, def: D) -> PipelineBuilder<Flag>
    where
        D: FnOnce(
            &mut Context<'_>,
            ProgramBuilder<Missing, Missing, Missing>,
        ) -> ProgramBuilder<&'static str, &'static str, Vec<(&'static str, u32)>>,
    {
        let builder = {
            let mut ctx = Context {
                log: &self.log,
                programs: &mut self.programs,
                vars: FNVMap::default(),
                replacements: None,
            };
            let builder = ProgramBuilder {
                enabled: true,
                vertex: Missing,
                vertex_defines: None,
                fragment: Missing,
                fragment_defines: None,
                attribute_binds: Missing,
            };
            def(&mut ctx, builder)
        };
        if !builder.enabled {
            return self;
        }
        let program = gl::Program::new();

        let vertex_shader = gl::Shader::new(gl::ShaderType::Vertex);
        // Ensure the version number goes first otherwise the
        // the shader wouldn't compile when we add defines.
        let mut vert_full = String::from("#version 330 core\n");
        if let Some(defines) = builder.vertex_defines {
            for def in defines {
                vert_full.push_str("#define ");
                vert_full.push_str(&def);
                vert_full.push_str("\n");
            }
        }
        // Load the shader from the file
        vert_full.push_str(&assume!(
            self.log,
            load_shader(&self.assets, builder.vertex)
        ));
        vertex_shader.set_source(&vert_full);
        assume!(self.log, vertex_shader.compile());

        let frag_shader = gl::Shader::new(gl::ShaderType::Fragment);
        // See above
        let mut frag_full = String::from("#version 330 core\n");
        if let Some(defines) = builder.fragment_defines {
            for def in defines {
                frag_full.push_str("#define ");
                frag_full.push_str(&def);
                frag_full.push_str("\n");
            }
        }
        // Load the shader from the file
        frag_full.push_str(&assume!(
            self.log,
            load_shader(&self.assets, builder.fragment)
        ));
        frag_shader.set_source(&frag_full);
        assume!(self.log, frag_shader.compile());

        program.attach_shader(vertex_shader);
        program.attach_shader(frag_shader);

        let mut attributes = FNVMap::default();
        for (k, v) in builder.attribute_binds {
            program.bind_attribute_location(k, v);
            attributes.insert(k, gl::Attribute::new(v));
        }

        program.link();
        program.use_program();

        self.programs.insert(
            name,
            Program {
                name,
                program,
                attributes,
                uniforms: FNVMap::default(),
                uniform_blocks: FNVMap::default(),
            },
        );

        self
    }

    pub fn pass<D>(mut self, name: &'static str, def: D) -> PipelineBuilder<Flag>
    where
        D: FnOnce(&mut Context<'_>, PassBuilder<()>) -> PassBuilder<Flag>,
    {
        let builder = {
            let mut ctx = Context {
                log: &self.log,
                programs: &mut self.programs,
                vars: FNVMap::default(),
                replacements: None,
            };
            let builder = PassBuilder::<()>::new();
            def(&mut ctx, builder)
        };
        if !builder.enabled {
            return self;
        }
        self.passes.push((name, builder));
        self
    }
}

pub struct DrawSetup<'a, Flag> {
    pipeline: &'a mut Pipeline<Flag>,
    vars: FNVMap<&'static str, Box<dyn Any>>,
    pass_vars: FNVMap<&'static str, FNVMap<&'static str, Box<dyn Any>>>,
}

impl<'a, Flag> DrawSetup<'a, Flag> {
    pub fn var<V: Any>(mut self, name: &'static str, v: V) -> Self {
        self.vars.insert(name, Box::new(v));
        self
    }

    pub fn pass_var<V: Any>(mut self, pipe: &'static str, name: &'static str, v: V) -> Self {
        {
            let vars = self.pass_vars.entry(pipe).or_insert_with(FNVMap::default);
            vars.insert(name, Box::new(v));
        }
        self
    }

    pub fn draw<F>(mut self, width: u32, height: u32, mut f: F)
    where
        F: FnMut(&mut Context<'_>, &Flag),
    {
        let mut ctx = Context {
            log: &self.pipeline.log,
            programs: &mut self.pipeline.programs,
            vars: self.vars,
            replacements: None,
        };

        self.pipeline
            .final_color_a
            .resize(width, height, self.pipeline.global_scale);
        self.pipeline
            .final_color_b
            .resize(width, height, self.pipeline.global_scale);

        for attachment in &self.pipeline.attachments {
            attachment.resize(
                width,
                height,
                if attachment.size.is_none() {
                    self.pipeline.global_scale
                } else {
                    1.0
                },
            );
        }

        let passes = self
            .pipeline
            .passes
            .iter()
            .filter(|v| {
                v.runtime_enable
                    .map_or(true, |r| *ctx.var::<bool>(r).unwrap_or(&false))
            })
            .collect::<Vec<_>>();
        let num_passes = passes.len();

        let mut current_final = 0;

        let mut buffers = vec![];
        for (idx, pass) in passes.into_iter().enumerate() {
            if let Some(p_vars) = self.pass_vars.remove(pass.name) {
                for (k, v) in p_vars {
                    ctx.vars.insert(k, v);
                }
            }
            ctx.replacements = Some(&pass.replacements);

            for &(pass, name, idx) in &pass.bind_texture {
                let attachments = &self.pipeline.attachments;
                if let Some(tex) = self
                    .pipeline
                    .passes
                    .iter()
                    .filter(|p| p.name == pass)
                    .map(|p| &attachments[p.attachment_names[name]])
                    .next()
                {
                    gl::active_texture(idx);
                    tex.texture.bind(gl::TextureTarget::Texture2D);
                }
            }
            gl::active_texture(0);

            let last_final = current_final;
            if let Some(framebuffer) = pass.framebuffer.as_ref() {
                framebuffer.bind(gl::TargetFramebuffer::Draw);

                buffers.clear();
                if pass.final_color {
                    let final_color = match current_final {
                        0 => {
                            current_final = 1;
                            &self.pipeline.final_color_a
                        }
                        _ => {
                            current_final = 0;
                            &self.pipeline.final_color_b
                        }
                    };
                    final_color.texture.bind(gl::TextureTarget::Texture2D);
                    framebuffer.texture_2d(
                        gl::TargetFramebuffer::Draw,
                        gl::Attachment::Color0,
                        gl::TextureTarget::Texture2D,
                        &final_color.texture,
                        0,
                    );
                    buffers.push(gl::Attachment::Color0);
                }

                let mut color = 0;
                for a in &pass.attachments {
                    let c = color;
                    let d = if let AttachTarget::Depth = a.target {
                        gl::TargetBuffer::Depth
                    } else {
                        color += 1;
                        let a = match c {
                            0 => gl::Attachment::Color0,
                            1 => gl::Attachment::Color1,
                            2 => gl::Attachment::Color2,
                            3 => gl::Attachment::Color3,
                            4 => gl::Attachment::Color4,
                            _ => panic!("Out of color attachments"),
                        };
                        buffers.push(a);
                        gl::TargetBuffer::Color
                    };
                    if let Some(clear) = a.clear_value {
                        gl::clear_buffer(
                            d,
                            if let gl::TargetBuffer::Depth = d {
                                0
                            } else {
                                c
                            },
                            &clear,
                        );
                    }
                }

                gl::draw_buffers(&buffers);
            } else {
                gl::Framebuffer::unbind(gl::TargetFramebuffer::Draw);
            }

            if pass.final_color && idx == num_passes - 1 {
                gl::Framebuffer::unbind(gl::TargetFramebuffer::Draw);
            }

            if let Some((width, height)) = pass.size {
                gl::view_port(
                    0,
                    0,
                    width / (pass.scale as u32),
                    height / (pass.scale as u32),
                );
            } else if !pass.attachments.is_empty() {
                gl::view_port(
                    0,
                    0,
                    ((width / (pass.scale as u32)) as f32 * self.pipeline.global_scale) as u32,
                    ((height / (pass.scale as u32)) as f32 * self.pipeline.global_scale) as u32,
                );
            } else {
                gl::view_port(
                    0,
                    0,
                    width / (pass.scale as u32),
                    height / (pass.scale as u32),
                );
            }
            if let Some(flags) = pass.clear_flags {
                gl::clear(flags);
            }
            if let Some(fullscreen) = pass.fullscreen.as_ref() {
                {
                    let prog = ctx.program(fullscreen.shader);
                    prog.use_program();

                    let mut next_tid = 10; // Play it safe and start at 10

                    if let Some(name) = fullscreen.final_color_input {
                        let final_color = match last_final {
                            1 => &self.pipeline.final_color_a,
                            _ => &self.pipeline.final_color_b,
                        };
                        gl::active_texture(next_tid);
                        final_color.texture.bind(gl::TextureTarget::Texture2D);
                        prog.uniform(name).map(|v| v.set_int(next_tid as i32));
                        next_tid += 1;
                    }

                    for input in &fullscreen.inputs {
                        let attachments = &self.pipeline.attachments;
                        if let Some(tex) = self
                            .pipeline
                            .passes
                            .iter()
                            .filter(|p| p.name == input.pass)
                            .map(|p| &attachments[p.attachment_names[input.name]])
                            .next()
                        {
                            gl::active_texture(next_tid);
                            tex.texture.bind(gl::TextureTarget::Texture2D);
                            prog.uniform(input.uniform)
                                .map(|v| v.set_int(next_tid as i32));
                            next_tid += 1;
                        }
                    }
                    gl::active_texture(0);
                }

                if let Some(pre) = fullscreen.pre_func.as_ref() {
                    pre(&mut ctx);
                }

                self.pipeline.quad_vao.bind();
                gl::draw_arrays(gl::DrawType::Triangles, 0, 6);
            } else {
                f(&mut ctx, &pass.flag);
            }
        }
        gl::Framebuffer::unbind(gl::TargetFramebuffer::Draw);
    }
}

pub struct Context<'a> {
    log: &'a Logger,
    programs: &'a mut FNVMap<&'static str, Program>,
    replacements: Option<&'a FNVMap<&'static str, &'static str>>,
    vars: FNVMap<&'static str, Box<dyn Any>>,
}

impl<'a> Context<'a> {
    pub fn program(&mut self, mut name: &'static str) -> &mut Program {
        if let Some(replace) = self.replacements.as_ref().and_then(|v| v.get(name)) {
            name = replace;
        }
        assume!(self.log, self.programs.get_mut(name))
    }

    pub fn var<V: Any>(&self, name: &'static str) -> Option<&V> {
        self.vars.get(name).and_then(|v| v.downcast_ref())
    }
}

pub struct Pass<Flag> {
    pub name: &'static str,
    pub flag: Flag,
    pub runtime_enable: Option<&'static str>,
    fullscreen: Option<FullscreenPass>,
    framebuffer: Option<gl::Framebuffer>,
    final_color: bool,
    attachments: Vec<Attachment>,
    attachment_names: FNVMap<&'static str, usize>,
    replacements: FNVMap<&'static str, &'static str>,
    size: Option<(u32, u32)>,
    scale: usize,
    bind_texture: Vec<(&'static str, &'static str, u32)>,
    clear_flags: Option<gl::BufferBit>,
}

pub struct PassBuilder<Flag> {
    enabled: bool,
    runtime_enable: Option<&'static str>,
    flag: Flag,
    fullscreen: Option<FullscreenPass>,
    final_color: bool,
    attachments: Vec<(&'static str, AttachmentBuilder)>,
    replacements: FNVMap<&'static str, &'static str>,
    size: Option<(u32, u32)>,
    scale: usize,
    bind_texture: Vec<(&'static str, &'static str, u32)>,
    clear_flags: Option<gl::BufferBit>,
}

impl PassBuilder<()> {
    fn new() -> PassBuilder<()> {
        PassBuilder {
            flag: (),
            enabled: true,
            runtime_enable: None,
            fullscreen: None,
            final_color: false,
            attachments: Vec::new(),
            replacements: FNVMap::default(),
            size: None,
            scale: 1,
            bind_texture: Vec::new(),
            clear_flags: None,
        }
    }
}

impl<Flag> PassBuilder<Flag> {
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn runtime_enable(mut self, name: &'static str) -> Self {
        self.runtime_enable = Some(name);
        self
    }

    pub fn when<F>(self, flag: bool, func: F) -> Self
    where
        F: FnOnce(Self) -> Self,
    {
        if flag {
            func(self)
        } else {
            self
        }
    }

    pub fn final_color(mut self) -> Self {
        self.final_color = true;
        self
    }

    pub fn clear_flags(mut self, flags: gl::BufferBit) -> Self {
        self.clear_flags = Some(flags);
        self
    }

    pub fn size(mut self, w: u32, h: u32) -> Self {
        self.size = Some((w, h));
        self
    }

    pub fn scale(mut self, scale: usize) -> Self {
        self.scale = scale;
        self
    }

    pub fn program_replace(mut self, name: &'static str, with: &'static str) -> PassBuilder<Flag> {
        self.replacements.insert(name, with);
        self
    }

    pub fn bind_texture(mut self, pass: &'static str, name: &'static str, idx: u32) -> Self {
        self.bind_texture.push((pass, name, idx));
        self
    }

    pub fn attachment<D>(mut self, name: &'static str, def: D) -> PassBuilder<Flag>
    where
        D: FnOnce(AttachmentBuilder) -> AttachmentBuilder,
    {
        let builder = {
            let builder = AttachmentBuilder {
                size: self.size,
                target: AttachTarget::Color,
                ty: Type::U8,
                components: Components::RGB,
                border_color: None,
                compare_ref: false,
                linear: false,
                clear_value: None,
                scale: 1,
            };
            def(builder)
        };
        self.attachments.push((name, builder));

        self
    }

    pub fn fullscreen<D>(mut self, def: D) -> PassBuilder<Flag>
    where
        D: FnOnce(FullscreenPassBuilder<Missing>) -> FullscreenPassBuilder<&'static str>,
    {
        let builder = {
            let builder = FullscreenPassBuilder {
                shader: Missing,
                inputs: Vec::new(),
                final_color_input: None,
                pre_func: None,
            };
            def(builder)
        };
        self.fullscreen = Some(FullscreenPass {
            shader: builder.shader,
            inputs: builder.inputs,
            final_color_input: builder.final_color_input,
            pre_func: builder.pre_func,
        });
        self
    }
}

impl PassBuilder<()> {
    pub fn flag<Flag>(self, flag: Flag) -> PassBuilder<Flag> {
        PassBuilder {
            enabled: self.enabled,
            runtime_enable: self.runtime_enable,
            flag,
            fullscreen: self.fullscreen,
            attachments: self.attachments,
            replacements: self.replacements,
            size: self.size,
            scale: self.scale,
            bind_texture: self.bind_texture,
            clear_flags: self.clear_flags,
            final_color: self.final_color,
        }
    }
}

#[derive(Clone)]
pub struct FullscreenPass {
    shader: &'static str,
    inputs: Vec<Input>,
    final_color_input: Option<&'static str>,
    pre_func: Option<Rc<dyn Fn(&mut Context<'_>)>>,
}

#[derive(Clone)]
struct Input {
    uniform: &'static str,
    pass: &'static str,
    name: &'static str,
}

pub struct FullscreenPassBuilder<S> {
    shader: S,
    final_color_input: Option<&'static str>,
    inputs: Vec<Input>,
    pre_func: Option<Rc<dyn Fn(&mut Context<'_>)>>,
}

impl FullscreenPassBuilder<Missing> {
    pub fn shader(self, shader: &'static str) -> FullscreenPassBuilder<&'static str> {
        FullscreenPassBuilder {
            shader,
            final_color_input: self.final_color_input,
            inputs: self.inputs,
            pre_func: self.pre_func,
        }
    }
}

impl<S> FullscreenPassBuilder<S> {
    pub fn when<Fun>(self, flag: bool, func: Fun) -> Self
    where
        Fun: FnOnce(Self) -> Self,
    {
        if flag {
            func(self)
        } else {
            self
        }
    }

    pub fn input_final(mut self, name: &'static str) -> Self {
        self.final_color_input = Some(name);
        self
    }

    pub fn input(mut self, uniform: &'static str, pass: &'static str, name: &'static str) -> Self {
        self.inputs.push(Input {
            uniform,
            pass,
            name,
        });
        self
    }

    pub fn pre<F>(mut self, f: F) -> Self
    where
        F: Fn(&mut Context<'_>) + 'static,
    {
        self.pre_func = Some(Rc::new(f));
        self
    }
}

pub struct InternalAttachment {
    texture: gl::Texture,
    current_size: Cell<Option<(u32, u32)>>,
    // Size of the attachment, None is screensize
    size: Option<(u32, u32)>,
    scale: usize,
    ty: Type,
    components: Components,
}

pub struct Attachment {
    target: AttachTarget,
    clear_value: Option<[f32; 4]>,
}

fn init_attachment(w: u32, h: u32, tex: &gl::Texture, ty: Type, components: Components) {
    let mut gl_ty = match ty {
        Type::U8 => gl::Type::UnsignedByte,
        Type::U16 => gl::Type::UnsignedShort,
        Type::I8 => gl::Type::Byte,
        Type::HalfFloat => gl::Type::HalfFloat,
        Type::Float => gl::Type::Float,
    };

    let (iformat, format) = match (components, ty) {
        (Components::Depth24, _) => {
            gl_ty = gl::Type::UnsignedInt;
            (
                gl::TextureFormat::DepthComponent24,
                gl::TextureFormat::DepthComponent,
            )
        }
        (Components::Depth24Stencil8, _) => {
            gl_ty = gl::Type::UnsignedInt248;
            (
                gl::TextureFormat::Depth24Stencil8,
                gl::TextureFormat::DepthStencil,
            )
        }
        (Components::R, Type::Float) => (gl::TextureFormat::R32F, gl::TextureFormat::Red),
        (Components::RG, Type::Float) => (gl::TextureFormat::Rg32F, gl::TextureFormat::Rg),
        (Components::RGB, Type::Float) => (gl::TextureFormat::Rgb32F, gl::TextureFormat::Rgb),
        (Components::RGBA, Type::Float) => (gl::TextureFormat::Rgba32F, gl::TextureFormat::Rgba),
        (Components::R, Type::HalfFloat) => (gl::TextureFormat::R16F, gl::TextureFormat::Red),
        (Components::RG, Type::HalfFloat) => (gl::TextureFormat::Rg16F, gl::TextureFormat::Rg),
        (Components::RGB, Type::HalfFloat) => (gl::TextureFormat::Rgb16F, gl::TextureFormat::Rgb),
        (Components::RGBA, Type::HalfFloat) => {
            (gl::TextureFormat::Rgba16F, gl::TextureFormat::Rgba)
        }

        (Components::R, Type::U8) => (gl::TextureFormat::R8, gl::TextureFormat::Red),
        (Components::RG, Type::U8) => (gl::TextureFormat::Rg8, gl::TextureFormat::Rg),
        (Components::RGB, Type::U8) => (gl::TextureFormat::Rgb8, gl::TextureFormat::Rgb),
        (Components::RGBA, Type::U8) => (gl::TextureFormat::Rgba8, gl::TextureFormat::Rgba),

        (Components::R, Type::U16) => (gl::TextureFormat::R16, gl::TextureFormat::Red),
        (Components::RG, Type::U16) => (gl::TextureFormat::Rg16, gl::TextureFormat::Rg),
        (Components::RGB, Type::U16) => (gl::TextureFormat::Rgb16, gl::TextureFormat::Rgb),
        (Components::RGBA, Type::U16) => (gl::TextureFormat::Rgba16, gl::TextureFormat::Rgba),

        (Components::R, Type::I8) => (gl::TextureFormat::R8I, gl::TextureFormat::Red),
        (Components::RG, Type::I8) => (gl::TextureFormat::Rg8I, gl::TextureFormat::Rg),
        (Components::RGB, Type::I8) => (gl::TextureFormat::Rgb8I, gl::TextureFormat::Rgb),
        (Components::RGBA, Type::I8) => (gl::TextureFormat::Rgba8I, gl::TextureFormat::Rgba),
    };

    tex.image_2d_ex(
        gl::TextureTarget::Texture2D,
        0,
        w,
        h,
        iformat,
        format,
        gl_ty,
        None,
    );
}

impl InternalAttachment {
    fn resize(&self, width: u32, height: u32, global_scale: f32) {
        let width = ((width / (self.scale as u32)) as f32 * global_scale) as u32;
        let height = ((height / (self.scale as u32)) as f32 * global_scale) as u32;
        if self.size.is_none() && self.current_size.get() != Some((width, height)) {
            self.current_size.set(Some((width, height)));
            self.texture.bind(gl::TextureTarget::Texture2D);
            init_attachment(width, height, &self.texture, self.ty, self.components)
        }
    }
}

pub struct AttachmentBuilder {
    // Size of the attachment, None is screensize
    size: Option<(u32, u32)>,
    target: AttachTarget,
    scale: usize,
    ty: Type,
    components: Components,

    border_color: Option<[f32; 4]>,
    clear_value: Option<[f32; 4]>,
    compare_ref: bool,

    linear: bool,
}

impl AttachmentBuilder {
    pub fn scale(mut self, scale: usize) -> Self {
        self.scale = scale;
        self
    }
    pub fn target(mut self, target: AttachTarget) -> Self {
        self.target = target;
        self
    }
    pub fn ty(mut self, ty: Type) -> Self {
        self.ty = ty;
        self
    }
    pub fn components(mut self, components: Components) -> Self {
        self.components = components;
        self
    }
    pub fn border_color(mut self, border_color: [f32; 4]) -> Self {
        self.border_color = Some(border_color);
        self
    }
    pub fn clear_value(mut self, clear_value: [f32; 4]) -> Self {
        self.clear_value = Some(clear_value);
        self
    }
    pub fn compare_ref(mut self) -> Self {
        self.compare_ref = true;
        self
    }
    pub fn linear(mut self) -> Self {
        self.linear = true;
        self
    }
}

#[derive(Debug, Clone, Copy, Ord, PartialOrd, Eq, PartialEq)]
pub enum Type {
    I8,
    U8,
    U16,
    HalfFloat,
    Float,
}

#[derive(Debug, Clone, Copy, Ord, PartialOrd, Eq, PartialEq)]
pub enum Components {
    R,
    RG,
    RGB,
    RGBA,
    Depth24,
    Depth24Stencil8,
}

#[derive(Clone, Copy)]
pub enum AttachTarget {
    Color,
    Depth,
}

pub struct ProgramBuilder<V, F, A> {
    enabled: bool,
    vertex: V,
    vertex_defines: Option<Vec<Cow<'static, str>>>,
    fragment: F,
    fragment_defines: Option<Vec<Cow<'static, str>>>,
    attribute_binds: A,
}

impl<V, F, A> ProgramBuilder<V, F, A> {
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn when<Fun>(self, flag: bool, func: Fun) -> Self
    where
        Fun: FnOnce(Self) -> Self,
    {
        if flag {
            func(self)
        } else {
            self
        }
    }
}

impl<F, A> ProgramBuilder<Missing, F, A> {
    pub fn vertex(self, name: &'static str) -> ProgramBuilder<&'static str, F, A> {
        ProgramBuilder {
            enabled: self.enabled,
            vertex: name,
            vertex_defines: self.vertex_defines,
            fragment: self.fragment,
            fragment_defines: self.fragment_defines,
            attribute_binds: self.attribute_binds,
        }
    }
}

impl<F, A> ProgramBuilder<&'static str, F, A> {
    pub fn vertex_defines<S>(mut self, defines: Vec<S>) -> Self
    where
        S: Into<Cow<'static, str>>,
    {
        {
            let defs = self.vertex_defines.get_or_insert_with(Vec::new);
            defs.extend(defines.into_iter().map(|v| v.into()));
        }
        self
    }
}

impl<V, A> ProgramBuilder<V, Missing, A> {
    pub fn fragment(self, name: &'static str) -> ProgramBuilder<V, &'static str, A> {
        ProgramBuilder {
            enabled: self.enabled,
            vertex: self.vertex,
            vertex_defines: self.vertex_defines,
            fragment: name,
            fragment_defines: self.fragment_defines,
            attribute_binds: self.attribute_binds,
        }
    }
}

impl<V, A> ProgramBuilder<V, &'static str, A> {
    pub fn fragment_defines<S>(mut self, defines: Vec<S>) -> Self
    where
        S: Into<Cow<'static, str>>,
    {
        {
            let defs = self.fragment_defines.get_or_insert_with(Vec::new);
            defs.extend(defines.into_iter().map(|v| v.into()));
        }
        self
    }
}
impl<V, F> ProgramBuilder<V, F, Missing> {
    pub fn attribute_binds(
        self,
        binds: &[(&'static str, u32)],
    ) -> ProgramBuilder<V, F, Vec<(&'static str, u32)>> {
        ProgramBuilder {
            enabled: self.enabled,
            vertex: self.vertex,
            vertex_defines: self.vertex_defines,
            fragment: self.fragment,
            fragment_defines: self.fragment_defines,
            attribute_binds: binds.to_owned(),
        }
    }
}

pub struct Missing;

pub struct Program {
    pub name: &'static str,
    program: gl::Program,
    attributes: FNVMap<&'static str, gl::Attribute>,
    uniforms: FNVMap<&'static str, Option<gl::Uniform>>,
    uniform_blocks: FNVMap<&'static str, gl::UniformBlock>,
}

impl Program {
    pub fn use_program(&self) {
        self.program.use_program();
    }

    pub fn attribute(&mut self, name: &'static str) -> Option<gl::Attribute> {
        self.attributes.get(name).cloned()
    }

    pub fn uniform(&mut self, name: &'static str) -> Option<gl::Uniform> {
        let program = &self.program;
        self.uniforms
            .entry(name)
            .or_insert_with(|| program.uniform_location(name))
            .as_ref()
            .cloned()
    }

    pub fn uniform_block(&mut self, name: &'static str) -> gl::UniformBlock {
        let program = &self.program;
        *self
            .uniform_blocks
            .entry(name)
            .or_insert_with(|| program.uniform_block(name))
    }
}

fn load_shader(asset_manager: &AssetManager, name: &str) -> UResult<String> {
    use std::io::Read;
    let mut file = asset_manager.open_from_pack("base", &format!("shaders/{}.glsl", name))?;
    let mut shader = String::new();
    file.read_to_string(&mut shader)?;
    Ok(shader)
}
