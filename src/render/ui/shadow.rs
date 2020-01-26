
use fungui;
use super::color::*;
use crate::ui::*;

#[derive(Clone, Debug, PartialEq)]
pub struct Shadow {
    pub offset: (f32, f32),
    pub color: Color,
    pub blur_radius: f32,
    pub spread_radius: f32,
    pub inset: bool,
}

impl fungui::ConvertValue<UniverCityUI> for Vec<Shadow> {
    type RefType = [Shadow];

    fn from_value(v: Value) -> Option<Self> {
        if let fungui::Value::ExtValue(UValue::Shadow(v)) = v {
            Some(v)
        } else {
            None
        }
    }
    fn from_value_ref(v: &Value) -> Option<&Self::RefType> {
        if let fungui::Value::ExtValue(UValue::Shadow(v)) = v {
            Some(v)
        } else {
            None
        }
    }
    fn to_value(v: Self) -> Value {
        fungui::Value::ExtValue(UValue::Shadow(v))
    }
}


pub fn shadows<'a>(args: &mut (dyn Iterator<Item = fungui::FResult<'a, Value>> + 'a)) -> fungui::FResult<'a, Value> {
    let shadows = args
        .map(|v| v.and_then(|v| v.convert::<Vec<Shadow>>()
            .filter(|v| v.len() == 1)
            .and_then(|v| v.into_iter().next())
            .ok_or(fungui::Error::CustomStatic {
                reason: "Expected shadow"
            }))
        )
        .collect::<fungui::FResult<'_, Vec<Shadow>>>()?;

    Ok(fungui::Value::ExtValue(UValue::Shadow(shadows)))
}

pub fn shadow<'a>(args: &mut (dyn Iterator<Item = fungui::FResult<'a, Value>> + 'a)) -> fungui::FResult<'a, Value> {
    let offset_x: f32 = args.next()
        .ok_or(fungui::Error::MissingParameter {
            position: 0,
            name: "offset_x"
        })
        .and_then(|v| v)?
        .convert()
        .ok_or(fungui::Error::CustomStatic {
            reason: "Expected float"
        })?;
    let offset_y: f32 = args.next()
        .ok_or(fungui::Error::MissingParameter {
            position: 1,
            name: "offset_y"
        })
        .and_then(|v| v)?
        .convert()
        .ok_or(fungui::Error::CustomStatic {
            reason: "Expected float"
        })?;

    let color: Color = Color::from_val(args.next()
        .ok_or(fungui::Error::MissingParameter {
            position: 2,
            name: "color"
        })
        .and_then(|v| v)?)
        .ok_or(fungui::Error::CustomStatic {
            reason: "Expected color"
        })?;

    let blur_radius: f32 = if let Some(b) = args.next() {
        b?
            .convert()
            .ok_or(fungui::Error::CustomStatic {
                reason: "Expected float"
            })?
    } else {
        1.0
    };

    let spread_radius: f32 = if let Some(b) = args.next() {
        b?
            .convert()
            .ok_or(fungui::Error::CustomStatic {
                reason: "Expected float"
            })?
    } else {
        1.0
    };

    let clip_mode = if let Some(b) = args.next() {
        b?
            .convert::<String>()
            .ok_or(fungui::Error::CustomStatic {
                reason: "Expected string"
            })
            .and_then(|v| match v.as_str() {
                "outset" => Ok(false),
                "inset" => Ok(true),
                _ => Err(fungui::Error::CustomStatic {
                    reason: "Expected either outset or inset"
                })
            })?
    } else {
        false
    };


    Ok(fungui::Value::ExtValue(UValue::Shadow(vec![Shadow {
        offset: (offset_x, offset_y),
        color: color,
        blur_radius: blur_radius,
        spread_radius: spread_radius,
        inset: clip_mode,
    }])))
}