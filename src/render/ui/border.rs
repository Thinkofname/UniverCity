use super::color::Color;
use super::*;
use crate::ui::Value;
use crate::ui::*;
use fungui;

#[derive(Clone, PartialEq)]
pub struct BorderWidthInfo {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl fungui::ConvertValue<UniverCityUI> for BorderWidthInfo {
    type RefType = BorderWidthInfo;

    fn from_value(v: Value) -> Option<Self> {
        if let fungui::Value::ExtValue(UValue::BorderWidth(v)) = v {
            Some(v)
        } else {
            None
        }
    }
    fn from_value_ref(v: &Value) -> Option<&Self::RefType> {
        if let fungui::Value::ExtValue(UValue::BorderWidth(v)) = v {
            Some(v)
        } else {
            None
        }
    }
    fn to_value(v: Self) -> Value {
        fungui::Value::ExtValue(UValue::BorderWidth(v))
    }
}

pub fn border_width<'a>(
    args: &mut (dyn Iterator<Item = fungui::FResult<'a, Value>> + 'a),
) -> fungui::FResult<'a, Value> {
    let left: f32 = args
        .next()
        .ok_or(fungui::Error::MissingParameter {
            position: 0,
            name: "left",
        })
        .and_then(|v| v)?
        .convert()
        .ok_or(fungui::Error::CustomStatic {
            reason: "Expected float",
        })?;
    let top: f32 = args
        .next()
        .and_then(|v| v.ok())
        .and_then(|v| v.convert())
        .unwrap_or(left);
    let right: f32 = args
        .next()
        .and_then(|v| v.ok())
        .and_then(|v| v.convert())
        .unwrap_or(left);
    let bottom: f32 = args
        .next()
        .and_then(|v| v.ok())
        .and_then(|v| v.convert())
        .unwrap_or(top);

    Ok(fungui::Value::ExtValue(UValue::BorderWidth(
        BorderWidthInfo {
            left,
            top,
            right,
            bottom,
        },
    )))
}

#[derive(Clone, PartialEq)]
pub enum Border {
    Normal {
        left: BorderSide,
        top: BorderSide,
        right: BorderSide,
        bottom: BorderSide,
    },
    Image {
        image: String,
        width: u32,
        height: u32,
        fill: bool,
    },
}

impl fungui::ConvertValue<UniverCityUI> for Border {
    type RefType = Border;

    fn from_value(v: Value) -> Option<Self> {
        if let fungui::Value::ExtValue(UValue::Border(v)) = v {
            Some(v)
        } else {
            None
        }
    }
    fn from_value_ref(v: &Value) -> Option<&Self::RefType> {
        if let fungui::Value::ExtValue(UValue::Border(v)) = v {
            Some(v)
        } else {
            None
        }
    }
    fn to_value(v: Self) -> Value {
        fungui::Value::ExtValue(UValue::Border(v))
    }
}

#[derive(Copy, Clone, PartialEq)]
pub struct BorderSide {
    pub color: Color,
    pub style: BorderStyle,
}

impl fungui::ConvertValue<UniverCityUI> for BorderSide {
    type RefType = BorderSide;

    fn from_value(v: Value) -> Option<Self> {
        if let fungui::Value::ExtValue(UValue::BorderSide(v)) = v {
            Some(v)
        } else {
            None
        }
    }
    fn from_value_ref(v: &Value) -> Option<&Self::RefType> {
        if let fungui::Value::ExtValue(UValue::BorderSide(v)) = v {
            Some(v)
        } else {
            None
        }
    }
    fn to_value(v: Self) -> Value {
        fungui::Value::ExtValue(UValue::BorderSide(v))
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum BorderStyle {
    None,
    Solid,
    Dotted,
    Inset,
    Outset,
}

pub fn border<'a>(
    args: &mut (dyn Iterator<Item = fungui::FResult<'a, Value>> + 'a),
) -> fungui::FResult<'a, Value> {
    let left: BorderSide = args
        .next()
        .ok_or(fungui::Error::MissingParameter {
            position: 0,
            name: "left",
        })
        .and_then(|v| v)?
        .convert()
        .ok_or(fungui::Error::CustomStatic {
            reason: "Expected float",
        })?;
    let top: BorderSide = args
        .next()
        .and_then(|v| v.ok())
        .and_then(|v| v.convert())
        .unwrap_or(left);
    let right: BorderSide = args
        .next()
        .and_then(|v| v.ok())
        .and_then(|v| v.convert())
        .unwrap_or(left);
    let bottom: BorderSide = args
        .next()
        .and_then(|v| v.ok())
        .and_then(|v| v.convert())
        .unwrap_or(top);

    Ok(fungui::Value::ExtValue(UValue::Border(Border::Normal {
        left,
        top,
        right,
        bottom,
    })))
}

pub fn border_image<'a>(
    args: &mut (dyn Iterator<Item = fungui::FResult<'a, Value>> + 'a),
) -> fungui::FResult<'a, Value> {
    let image: String = args
        .next()
        .ok_or(fungui::Error::MissingParameter {
            position: 0,
            name: "image",
        })
        .and_then(|v| v)?
        .convert()
        .ok_or(fungui::Error::CustomStatic {
            reason: "Expected string",
        })?;

    let pwidth: i32 = args
        .next()
        .ok_or(fungui::Error::MissingParameter {
            position: 1,
            name: "width",
        })
        .and_then(|v| v)?
        .convert()
        .ok_or(fungui::Error::CustomStatic {
            reason: "Expected integer",
        })?;
    let pheight: i32 = args
        .next()
        .unwrap_or_else(|| Ok(fungui::Value::Integer(pwidth)))?
        .convert()
        .ok_or(fungui::Error::CustomStatic {
            reason: "Expected integer",
        })?;

    let fill: bool = args
        .next()
        .unwrap_or(Ok(fungui::Value::Boolean(false)))?
        .convert()
        .ok_or(fungui::Error::CustomStatic {
            reason: "Expected boolean",
        })?;

    Ok(fungui::Value::ExtValue(UValue::Border(Border::Image {
        image: image,
        width: pwidth as u32 * 3,
        height: pheight as u32 * 3,
        fill: fill,
    })))
}

pub fn border_side<'a>(
    args: &mut (dyn Iterator<Item = fungui::FResult<'a, Value>> + 'a),
) -> fungui::FResult<'a, Value> {
    let color: Color = Color::from_val(
        args.next()
            .ok_or(fungui::Error::MissingParameter {
                position: 0,
                name: "color",
            })
            .and_then(|v| v)?,
    )
    .ok_or(fungui::Error::CustomStatic {
        reason: "Expected color",
    })?;

    let style = if let Some(v) = args.next() {
        match v?
            .convert::<String>()
            .ok_or(fungui::Error::CustomStatic {
                reason: "Expected style",
            })?
            .as_str()
        {
            "solid" => BorderStyle::Solid,
            "dotted" => BorderStyle::Dotted,
            "inset" => BorderStyle::Inset,
            "outset" => BorderStyle::Outset,
            _ => BorderStyle::None,
        }
    } else {
        BorderStyle::Solid
    };

    Ok(fungui::Value::ExtValue(UValue::BorderSide(BorderSide {
        color,
        style,
    })))
}

impl<'a, 'b> Builder<'a, 'b> {
    pub fn border_image(
        &mut self,
        position: (f32, f32),
        width: f32,
        height: f32,
        tint: (u8, u8, u8, u8),
        widths: &BorderWidthInfo,
        image: &str,
        iwidth: u32,
        iheight: u32,
        fill: bool,
    ) -> BorderRender {
        let (attrib_position, attrib_texture_info, attrib_atlas, attrib_uv, attrib_color) = {
            let program = self.ctx.program("ui/border_image");
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

        let (atlas, rect) = crate::render::RenderState::texture_info_for(
            self.log,
            self.assets,
            self.global_atlas,
            LazyResourceKey::parse(&image).or_module(ModuleKey::new("base")),
        );

        let atlas = atlas as u16;
        let texture_x = rect.x as u16;
        let texture_y = rect.y as u16;
        let texture_w = iwidth as u16 / 3;
        let texture_h = iheight as u16 / 3;

        let width_uv = (width - widths.left - widths.right) / f32::from(texture_w);
        let height_uv = (height - widths.top - widths.bottom) / f32::from(texture_h);

        let mut widths = Clone::clone(widths);
        widths.left = widths.left.min(width);
        widths.right = widths.right.min(width);
        widths.top = widths.top.min(height);
        widths.bottom = widths.bottom.min(height);

        macro_rules! gen_image_verts {
            [$($elm:expr),*] => {[
                // Top left corner
                ImageVertex { x: position.0, y: position.1 + widths.top, atlas, texture_x, texture_y, texture_w, texture_h, _padding: 0, ux: 0.0, uy: 1.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},
                ImageVertex { x: position.0, y: position.1, atlas, texture_x, texture_y, texture_w, texture_h, _padding: 0, ux: 0.0, uy: 0.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},
                ImageVertex { x: position.0 + widths.left, y: position.1 + widths.top, atlas, texture_x, texture_y, texture_w, texture_h, _padding: 0, ux: 1.0, uy: 1.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},

                ImageVertex { x: position.0, y: position.1, atlas, texture_x, texture_y, texture_w, texture_h, _padding: 0, ux: 0.0, uy: 0.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},
                ImageVertex { x: position.0 + widths.left, y: position.1, atlas, texture_x, texture_y, texture_w, texture_h, _padding: 0, ux: 1.0, uy: 0.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},
                ImageVertex { x: position.0 + widths.left, y: position.1 + widths.top, atlas, texture_x, texture_y, texture_w, texture_h, _padding: 0, ux: 1.0, uy: 1.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},

                // Top right corner
                ImageVertex { x: position.0 + width - widths.right, y: position.1 + widths.top, atlas, texture_x: texture_x + texture_w * 2, texture_y, texture_w, texture_h, _padding: 0, ux: 0.0, uy: 1.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},
                ImageVertex { x: position.0 + width - widths.right, y: position.1, atlas, texture_x: texture_x + texture_w * 2, texture_y, texture_w, texture_h, _padding: 0, ux: 0.0, uy: 0.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},
                ImageVertex { x: position.0 + width, y: position.1 + widths.top, atlas, texture_x: texture_x + texture_w * 2, texture_y, texture_w, texture_h, _padding: 0, ux: 1.0, uy: 1.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},

                ImageVertex { x: position.0 + width - widths.right, y: position.1, atlas, texture_x: texture_x + texture_w * 2, texture_y, texture_w, texture_h, _padding: 0, ux: 0.0, uy: 0.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},
                ImageVertex { x: position.0 + width, y: position.1, atlas, texture_x: texture_x + texture_w * 2, texture_y, texture_w, texture_h, _padding: 0, ux: 1.0, uy: 0.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},
                ImageVertex { x: position.0 + width, y: position.1 + widths.top, atlas, texture_x: texture_x + texture_w * 2, texture_y, texture_w, texture_h, _padding: 0, ux: 1.0, uy: 1.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},

                // Bottom left corner
                ImageVertex { x: position.0, y: position.1 + height, atlas, texture_x, texture_y: texture_y + texture_h * 2, texture_w, texture_h, _padding: 0, ux: 0.0, uy: 1.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},
                ImageVertex { x: position.0, y: position.1 + height - widths.bottom, atlas, texture_x, texture_y: texture_y + texture_h * 2, texture_w, texture_h, _padding: 0, ux: 0.0, uy: 0.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},
                ImageVertex { x: position.0 + widths.left, y: position.1 + height, atlas, texture_x, texture_y: texture_y + texture_h * 2, texture_w, texture_h, _padding: 0, ux: 1.0, uy: 1.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},

                ImageVertex { x: position.0, y: position.1 + height - widths.bottom, atlas, texture_x, texture_y: texture_y + texture_h * 2, texture_w, texture_h, _padding: 0, ux: 0.0, uy: 0.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},
                ImageVertex { x: position.0 + widths.left, y: position.1 + height - widths.bottom, atlas, texture_x, texture_y: texture_y + texture_h * 2, texture_w, texture_h, _padding: 0, ux: 1.0, uy: 0.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},
                ImageVertex { x: position.0 + widths.left, y: position.1 + height, atlas, texture_x, texture_y: texture_y + texture_h * 2, texture_w, texture_h, _padding: 0, ux: 1.0, uy: 1.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},

                // Bottom right corner
                ImageVertex { x: position.0 + width - widths.right, y: position.1 + height, atlas, texture_x: texture_x + texture_w * 2, texture_y: texture_y + texture_h * 2, texture_w, texture_h, _padding: 0, ux: 0.0, uy: 1.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},
                ImageVertex { x: position.0 + width - widths.right, y: position.1 + height - widths.bottom, atlas, texture_x: texture_x + texture_w * 2, texture_y: texture_y + texture_h * 2, texture_w, texture_h, _padding: 0, ux: 0.0, uy: 0.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},
                ImageVertex { x: position.0 + width, y: position.1 + height, atlas, texture_x: texture_x + texture_w * 2, texture_y: texture_y + texture_h * 2, texture_w, texture_h, _padding: 0, ux: 1.0, uy: 1.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},

                ImageVertex { x: position.0 + width - widths.right, y: position.1 + height - widths.bottom, atlas, texture_x: texture_x + texture_w * 2, texture_y: texture_y + texture_h * 2, texture_w, texture_h, _padding: 0, ux: 0.0, uy: 0.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},
                ImageVertex { x: position.0 + width, y: position.1 + height - widths.bottom, atlas, texture_x: texture_x + texture_w * 2, texture_y: texture_y + texture_h * 2, texture_w, texture_h, _padding: 0, ux: 1.0, uy: 0.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},
                ImageVertex { x: position.0 + width, y: position.1 + height, atlas, texture_x: texture_x + texture_w * 2, texture_y: texture_y + texture_h * 2, texture_w, texture_h, _padding: 0, ux: 1.0, uy: 1.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},

                // Top edge
                ImageVertex { x: position.0 + widths.left, y: position.1 + widths.top, atlas, texture_x: texture_x + texture_w, texture_y, texture_w, texture_h, _padding: 0, ux: 0.0, uy: 1.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},
                ImageVertex { x: position.0 + widths.left, y: position.1, atlas, texture_x: texture_x + texture_w, texture_y, texture_w, texture_h, _padding: 0, ux: 0.0, uy: 0.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},
                ImageVertex { x: position.0 + width - widths.right, y: position.1 + widths.top, atlas, texture_x: texture_x + texture_w, texture_y, texture_w, texture_h, _padding: 0, ux: width_uv, uy: 1.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},

                ImageVertex { x: position.0 + widths.left, y: position.1, atlas, texture_x: texture_x + texture_w, texture_y, texture_w, texture_h, _padding: 0, ux: 0.0, uy: 0.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},
                ImageVertex { x: position.0 + width - widths.right, y: position.1, atlas, texture_x: texture_x + texture_w, texture_y, texture_w, texture_h, _padding: 0, ux: width_uv, uy: 0.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},
                ImageVertex { x: position.0 + width - widths.right, y: position.1 + widths.top, atlas, texture_x: texture_x + texture_w, texture_y, texture_w, texture_h, _padding: 0, ux: width_uv, uy: 1.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},

                // Bottom edge
                ImageVertex { x: position.0 + widths.left, y: position.1 + height, atlas, texture_x: texture_x + texture_w, texture_y: texture_y + texture_h * 2, texture_w, texture_h, _padding: 0, ux: 0.0, uy: 1.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},
                ImageVertex { x: position.0 + widths.left, y: position.1 + height - widths.bottom, atlas, texture_x: texture_x + texture_w, texture_y: texture_y + texture_h * 2, texture_w, texture_h, _padding: 0, ux: 0.0, uy: 0.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},
                ImageVertex { x: position.0 + width - widths.right, y: position.1 + height, atlas, texture_x: texture_x + texture_w, texture_y: texture_y + texture_h * 2, texture_w, texture_h, _padding: 0, ux: width_uv, uy: 1.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},

                ImageVertex { x: position.0 + widths.left, y: position.1 + height - widths.bottom, atlas, texture_x: texture_x + texture_w, texture_y: texture_y + texture_h * 2, texture_w, texture_h, _padding: 0, ux: 0.0, uy: 0.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},
                ImageVertex { x: position.0 + width - widths.right, y: position.1 + height - widths.bottom, atlas, texture_x: texture_x + texture_w, texture_y: texture_y + texture_h * 2, texture_w, texture_h, _padding: 0, ux: width_uv, uy: 0.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},
                ImageVertex { x: position.0 + width - widths.right, y: position.1 + height, atlas, texture_x: texture_x + texture_w, texture_y: texture_y + texture_h * 2, texture_w, texture_h, _padding: 0, ux: width_uv, uy: 1.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},

                // Left edge
                ImageVertex { x: position.0, y: position.1 + height - widths.bottom, atlas, texture_x: texture_x, texture_y: texture_y + texture_h, texture_w, texture_h, _padding: 0, ux: 0.0, uy: height_uv, r: tint.0, g: tint.1, b: tint.2, a: tint.3},
                ImageVertex { x: position.0, y: position.1 + widths.top, atlas, texture_x: texture_x, texture_y: texture_y + texture_h, texture_w, texture_h, _padding: 0, ux: 0.0, uy: 0.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},
                ImageVertex { x: position.0 + widths.left, y: position.1 + height - widths.bottom, atlas, texture_x: texture_x, texture_y: texture_y + texture_h, texture_w, texture_h, _padding: 0, ux: 1.0, uy: height_uv, r: tint.0, g: tint.1, b: tint.2, a: tint.3},

                ImageVertex { x: position.0, y: position.1 + widths.top, atlas, texture_x: texture_x, texture_y: texture_y + texture_h, texture_w, texture_h, _padding: 0, ux: 0.0, uy: 0.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},
                ImageVertex { x: position.0 + widths.left, y: position.1 + widths.top, atlas, texture_x: texture_x, texture_y: texture_y + texture_h, texture_w, texture_h, _padding: 0, ux: 1.0, uy: 0.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},
                ImageVertex { x: position.0 + widths.left, y: position.1 + height - widths.bottom, atlas, texture_x: texture_x, texture_y: texture_y + texture_h, texture_w, texture_h, _padding: 0, ux: 1.0, uy: height_uv, r: tint.0, g: tint.1, b: tint.2, a: tint.3},

                // Right edge
                ImageVertex { x: position.0 + width - widths.right, y: position.1 + height - widths.bottom, atlas, texture_x: texture_x + texture_w * 2, texture_y: texture_y + texture_h, texture_w, texture_h, _padding: 0, ux: 0.0, uy: height_uv, r: tint.0, g: tint.1, b: tint.2, a: tint.3},
                ImageVertex { x: position.0 + width - widths.right, y: position.1 + widths.top, atlas, texture_x: texture_x + texture_w * 2, texture_y: texture_y + texture_h, texture_w, texture_h, _padding: 0, ux: 0.0, uy: 0.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},
                ImageVertex { x: position.0 + width, y: position.1 + height - widths.bottom, atlas, texture_x: texture_x + texture_w * 2, texture_y: texture_y + texture_h, texture_w, texture_h, _padding: 0, ux: 1.0, uy: height_uv, r: tint.0, g: tint.1, b: tint.2, a: tint.3},

                ImageVertex { x: position.0 + width - widths.right, y: position.1 + widths.top, atlas, texture_x: texture_x + texture_w * 2, texture_y: texture_y + texture_h, texture_w, texture_h, _padding: 0, ux: 0.0, uy: 0.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},
                ImageVertex { x: position.0 + width, y: position.1 + widths.top, atlas, texture_x: texture_x + texture_w * 2, texture_y: texture_y + texture_h, texture_w, texture_h, _padding: 0, ux: 1.0, uy: 0.0, r: tint.0, g: tint.1, b: tint.2, a: tint.3},
                ImageVertex { x: position.0 + width, y: position.1 + height - widths.bottom, atlas, texture_x: texture_x + texture_w * 2, texture_y: texture_y + texture_h, texture_w, texture_h, _padding: 0, ux: 1.0, uy: height_uv, r: tint.0, g: tint.1, b: tint.2, a: tint.3},

                $($elm,)*
            ]};
        }

        let a;
        let b;
        let verts: &[ImageVertex] = if fill {
            a = gen_image_verts![
                ImageVertex {
                    x: position.0 + widths.left,
                    y: position.1 + height - widths.bottom,
                    atlas,
                    texture_x: texture_x + texture_w,
                    texture_y: texture_y + texture_h,
                    texture_w,
                    texture_h,
                    _padding: 0,
                    ux: 0.0,
                    uy: height_uv,
                    r: tint.0,
                    g: tint.1,
                    b: tint.2,
                    a: tint.3
                },
                ImageVertex {
                    x: position.0 + widths.left,
                    y: position.1 + widths.top,
                    atlas,
                    texture_x: texture_x + texture_w,
                    texture_y: texture_y + texture_h,
                    texture_w,
                    texture_h,
                    _padding: 0,
                    ux: 0.0,
                    uy: 0.0,
                    r: tint.0,
                    g: tint.1,
                    b: tint.2,
                    a: tint.3
                },
                ImageVertex {
                    x: position.0 + width - widths.right,
                    y: position.1 + height - widths.bottom,
                    atlas,
                    texture_x: texture_x + texture_w,
                    texture_y: texture_y + texture_h,
                    texture_w,
                    texture_h,
                    _padding: 0,
                    ux: width_uv,
                    uy: height_uv,
                    r: tint.0,
                    g: tint.1,
                    b: tint.2,
                    a: tint.3
                },
                ImageVertex {
                    x: position.0 + widths.left,
                    y: position.1 + widths.top,
                    atlas,
                    texture_x: texture_x + texture_w,
                    texture_y: texture_y + texture_h,
                    texture_w,
                    texture_h,
                    _padding: 0,
                    ux: 0.0,
                    uy: 0.0,
                    r: tint.0,
                    g: tint.1,
                    b: tint.2,
                    a: tint.3
                },
                ImageVertex {
                    x: position.0 + width - widths.right,
                    y: position.1 + widths.top,
                    atlas,
                    texture_x: texture_x + texture_w,
                    texture_y: texture_y + texture_h,
                    texture_w,
                    texture_h,
                    _padding: 0,
                    ux: width_uv,
                    uy: 0.0,
                    r: tint.0,
                    g: tint.1,
                    b: tint.2,
                    a: tint.3
                },
                ImageVertex {
                    x: position.0 + width - widths.right,
                    y: position.1 + height - widths.bottom,
                    atlas,
                    texture_x: texture_x + texture_w,
                    texture_y: texture_y + texture_h,
                    texture_w,
                    texture_h,
                    _padding: 0,
                    ux: width_uv,
                    uy: height_uv,
                    r: tint.0,
                    g: tint.1,
                    b: tint.2,
                    a: tint.3
                }
            ];
            &a
        } else {
            b = gen_image_verts![];
            &b
        };

        buffer.set_data(gl::BufferTarget::Array, verts, gl::BufferUsage::Static);
        BorderRender {
            array: array,
            _buffer: buffer,
            count: verts.len(),
            image: true,
        }
    }

    pub fn border(
        &mut self,
        position: (f32, f32),
        width: f32,
        height: f32,
        widths: &BorderWidthInfo,
        left: &BorderSide,
        top: &BorderSide,
        right: &BorderSide,
        bottom: &BorderSide,
    ) -> BorderRender {
        let (attrib_position, attrib_rel_position, attrib_color, attrib_style) = {
            let program = self.ctx.program("ui/border_solid");
            program.use_program();
            (
                assume!(self.log, program.attribute("attrib_position")),
                assume!(self.log, program.attribute("attrib_rel_position")),
                assume!(self.log, program.attribute("attrib_color")),
                assume!(self.log, program.attribute("attrib_style")),
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
            mem::size_of::<BorderVertex>() as i32,
            0,
        );
        attrib_rel_position.enable();
        attrib_rel_position.vertex_pointer(
            3,
            gl::Type::Float,
            false,
            mem::size_of::<BorderVertex>() as i32,
            8,
        );
        attrib_color.enable();
        attrib_color.vertex_pointer(
            4,
            gl::Type::UnsignedByte,
            true,
            mem::size_of::<BorderVertex>() as i32,
            20,
        );
        attrib_style.enable();
        attrib_style.vertex_int_pointer(
            4,
            gl::Type::Byte,
            mem::size_of::<BorderVertex>() as i32,
            24,
        );

        let mut widths = Clone::clone(widths);
        widths.left = widths.left.min(width);
        widths.right = widths.right.min(width);
        widths.top = widths.top.min(height);
        widths.bottom = widths.bottom.min(height);

        let mut verts = Vec::with_capacity(
            if top.style != BorderStyle::None {
                12
            } else {
                0
            } + if bottom.style != BorderStyle::None {
                12
            } else {
                0
            } + if left.style != BorderStyle::None {
                12
            } else {
                0
            } + if right.style != BorderStyle::None {
                12
            } else {
                0
            },
        );

        #[inline(always)]
        fn do_border(
            verts: &mut Vec<BorderVertex>,
            position: (f32, f32),
            width: f32,
            height: f32,
            widths: &BorderWidthInfo,
            side: &BorderSide,
            dx: f32,
            dy: f32,
            ex: f32,
            ey: f32,
        ) {
            let style = match side.style {
                BorderStyle::None => return,
                BorderStyle::Solid => 0,
                BorderStyle::Dotted => 1,
                BorderStyle::Inset => 2,
                BorderStyle::Outset => 3,
            };

            let bx =
                // Shift to the left or right(minus the border) based on the direction, 0 if top/bottom
                (width * 0.5) * ex.abs() + (width * 0.5) * ex - widths.right * ex.max(0.0)
                // If top or bottom shift by the left edge
                + widths.left * ey.abs()
                ;
            let by =
                // Shift to the top or bottom(minus the border) based on the direction, 0 if left/right
                (height * 0.5) * ey.abs() + (height * 0.5) * ey - widths.bottom * ey.max(0.0)
                // If left or right shift by the top edge
                + widths.top * ex.abs()
                ;
            let bw =
                // If left then use the left edge
                widths.left * ex.min(0.0).abs()
                // If right then use the right edge
                + widths.right * ex.max(0.0).abs()
                // If top/bottom use the width minus the edges
                + (width - (widths.left + widths.right)) * ey.abs()
                ;
            let bh =
                // If top then use the top edge
                widths.top * ey.min(0.0).abs()
                // If bottom then use the bottom edge
                + widths.bottom * ey.max(0.0).abs()
                // If right/right use the height minus the edges
                + (height - (widths.top + widths.bottom)) * ex.abs()
                ;

            let params = BorderVertex {
                x: 0.0,
                y: 0.0,
                rel_x: 0.0,
                rel_y: 0.0,
                size: if ex != 0.0 {
                    if ex < 0.0 {
                        widths.left
                    } else {
                        widths.right
                    }
                } else if ey < 0.0 {
                    widths.top
                } else {
                    widths.bottom
                },
                r: (side.color.r * 255.0) as u8,
                g: (side.color.g * 255.0) as u8,
                b: (side.color.b * 255.0) as u8,
                a: (side.color.a * 255.0) as u8,

                style,
                dir_x: dx as i8,
                dir_y: dy as i8,
                padding: 0,
            };

            verts.push(BorderVertex {
                x: position.0 + bx,
                y: position.1 + by + bh,
                rel_x: 0.0,
                rel_y: 0.0 + bh,
                ..params
            });
            verts.push(BorderVertex {
                x: position.0 + bx,
                y: position.1 + by,
                rel_x: 0.0,
                rel_y: 0.0,
                ..params
            });
            verts.push(BorderVertex {
                x: position.0 + bx + bw,
                y: position.1 + by + bh,
                rel_x: 0.0 + bw,
                rel_y: 0.0 + bh,
                ..params
            });
            verts.push(BorderVertex {
                x: position.0 + bx,
                y: position.1 + by,
                rel_x: 0.0,
                rel_y: 0.0,
                ..params
            });
            verts.push(BorderVertex {
                x: position.0 + bx + bw,
                y: position.1 + by,
                rel_x: 0.0 + bw,
                rel_y: 0.0,
                ..params
            });
            verts.push(BorderVertex {
                x: position.0 + bx + bw,
                y: position.1 + by + bh,
                rel_x: 0.0 + bw,
                rel_y: 0.0 + bh,
                ..params
            });

            if ey.abs() > 0.5 {
                let top = ey.max(0.0) * widths.bottom;
                let bottom = ey.min(0.0).abs() * widths.top;
                let tmp = &mut [
                    BorderVertex {
                        x: position.0 + bx,
                        y: position.1 + by + top,
                        rel_x: 0.0,
                        rel_y: 0.0 + top,
                        ..params
                    },
                    BorderVertex {
                        x: position.0 + bx - widths.left,
                        y: position.1 + by + top,
                        rel_x: 0.0 - widths.left,
                        rel_y: 0.0 + top,
                        ..params
                    },
                    BorderVertex {
                        x: position.0 + bx,
                        y: position.1 + by + bottom,
                        rel_x: 0.0,
                        rel_y: 0.0 + bottom,
                        ..params
                    },
                    BorderVertex {
                        x: position.0 + bx + bw,
                        y: position.1 + by + top,
                        rel_x: 0.0 + bw,
                        rel_y: 0.0 + top,
                        ..params
                    },
                    BorderVertex {
                        x: position.0 + bx + bw,
                        y: position.1 + by + bottom,
                        rel_x: 0.0 + bw,
                        rel_y: 0.0 + bottom,
                        ..params
                    },
                    BorderVertex {
                        x: position.0 + bx + bw + widths.right,
                        y: position.1 + by + top,
                        rel_x: 0.0 + bw + widths.right,
                        rel_y: 0.0 + top,
                        ..params
                    },
                ];
                if ey < 0.0 {
                    tmp.reverse();
                }

                verts.extend_from_slice(tmp);
            }

            if ex.abs() > 0.5 {
                let top = ex.max(0.0) * widths.right;
                let bottom = ex.min(0.0).abs() * widths.left;
                let tmp = &mut [
                    BorderVertex {
                        x: position.0 + bx + top,
                        y: position.1 + by,
                        rel_x: 0.0 + top,
                        rel_y: 0.0,
                        ..params
                    },
                    BorderVertex {
                        x: position.0 + bx + bottom,
                        y: position.1 + by,
                        rel_x: 0.0 + bottom,
                        rel_y: 0.0,
                        ..params
                    },
                    BorderVertex {
                        x: position.0 + bx + top,
                        y: position.1 + by - widths.top,
                        rel_x: 0.0 + top,
                        rel_y: 0.0 - widths.top,
                        ..params
                    },
                    BorderVertex {
                        x: position.0 + bx + top,
                        y: position.1 + by + bh,
                        rel_x: 0.0 + top,
                        rel_y: 0.0 + bh,
                        ..params
                    },
                    BorderVertex {
                        x: position.0 + bx + top,
                        y: position.1 + by + bh + widths.bottom,
                        rel_x: 0.0 + top,
                        rel_y: 0.0 + bh + widths.bottom,
                        ..params
                    },
                    BorderVertex {
                        x: position.0 + bx + bottom,
                        y: position.1 + by + bh,
                        rel_x: 0.0 + bottom,
                        rel_y: 0.0 + bh,
                        ..params
                    },
                ];
                if ex < 0.0 {
                    tmp.reverse();
                }

                verts.extend_from_slice(tmp);
            }
        }

        do_border(
            &mut verts, position, width, height, &widths, top, 1.0, 0.0, 0.0, -1.0,
        );
        do_border(
            &mut verts, position, width, height, &widths, bottom, -1.0, 0.0, 0.0, 1.0,
        );

        do_border(
            &mut verts, position, width, height, &widths, left, 0.0, 1.0, -1.0, 0.0,
        );
        do_border(
            &mut verts, position, width, height, &widths, right, 0.0, -1.0, 1.0, 0.0,
        );

        buffer.set_data(gl::BufferTarget::Array, &verts, gl::BufferUsage::Static);
        BorderRender {
            array: array,
            _buffer: buffer,
            count: verts.len(),
            image: false,
        }
    }
}
