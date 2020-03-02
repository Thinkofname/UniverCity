use super::color::*;
use crate::ui::*;
use fungui;

#[derive(Clone, Debug, PartialEq)]
pub struct TShadow {
    pub offset: (f32, f32),
    pub color: Color,
    pub blur_radius: f32,
}

impl fungui::ConvertValue<UniverCityUI> for TShadow {
    type RefType = TShadow;

    fn from_value(v: Value) -> Option<Self> {
        if let fungui::Value::ExtValue(UValue::TextShadow(v)) = v {
            Some(v)
        } else {
            None
        }
    }
    fn from_value_ref(v: &Value) -> Option<&Self::RefType> {
        if let fungui::Value::ExtValue(UValue::TextShadow(v)) = v {
            Some(v)
        } else {
            None
        }
    }
    fn to_value(v: Self) -> Value {
        fungui::Value::ExtValue(UValue::TextShadow(v))
    }
}

pub fn text_shadow<'a>(
    args: &mut (dyn Iterator<Item = fungui::FResult<'a, Value>> + 'a),
) -> fungui::FResult<'a, Value> {
    let offset_x: f32 = args
        .next()
        .ok_or(fungui::Error::MissingParameter {
            position: 0,
            name: "offset_x",
        })
        .and_then(|v| v)?
        .convert()
        .ok_or(fungui::Error::CustomStatic {
            reason: "Expected float",
        })?;
    let offset_y: f32 = args
        .next()
        .ok_or(fungui::Error::MissingParameter {
            position: 1,
            name: "offset_y",
        })
        .and_then(|v| v)?
        .convert()
        .ok_or(fungui::Error::CustomStatic {
            reason: "Expected float",
        })?;

    let color: Color = Color::from_val(
        args.next()
            .ok_or(fungui::Error::MissingParameter {
                position: 2,
                name: "color",
            })
            .and_then(|v| v)?,
    )
    .ok_or(fungui::Error::CustomStatic {
        reason: "Expected color",
    })?;

    let blur_radius: f32 = if let Some(b) = args.next() {
        b?.convert().ok_or(fungui::Error::CustomStatic {
            reason: "Expected float",
        })?
    } else {
        1.0
    };

    Ok(fungui::Value::ExtValue(UValue::TextShadow(TShadow {
        offset: (offset_x, offset_y),
        color: color,
        blur_radius: blur_radius,
    })))
}
