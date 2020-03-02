use crate::ui::*;
use fungui;

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Default for Color {
    fn default() -> Self {
        Color {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 1.0,
        }
    }
}

impl fungui::ConvertValue<UniverCityUI> for Color {
    type RefType = Color;

    fn from_value(v: Value) -> Option<Self> {
        if let fungui::Value::ExtValue(UValue::Color(v)) = v {
            Some(v)
        } else {
            None
        }
    }
    fn from_value_ref(v: &Value) -> Option<&Self::RefType> {
        if let fungui::Value::ExtValue(UValue::Color(v)) = v {
            Some(v)
        } else {
            None
        }
    }
    fn to_value(v: Self) -> Value {
        fungui::Value::ExtValue(UValue::Color(v))
    }
}

impl Color {
    pub fn from_val(obj: Value) -> Option<Color> {
        if let Some(col) = obj.convert_ref::<String>().and_then(|v| parse_color(v)) {
            Some(col)
        } else if let Some(col) = obj.convert::<Color>() {
            Some(col)
        } else {
            None
        }
    }
}

pub fn rgb<'a>(
    args: &mut (dyn Iterator<Item = fungui::FResult<'a, Value>> + 'a),
) -> fungui::FResult<'a, Value> {
    let r = args
        .next()
        .ok_or(fungui::Error::MissingParameter {
            position: 0,
            name: "r",
        })
        .and_then(|v| v)?;
    let g = args
        .next()
        .ok_or(fungui::Error::MissingParameter {
            position: 1,
            name: "g",
        })
        .and_then(|v| v)?;
    let b = args
        .next()
        .ok_or(fungui::Error::MissingParameter {
            position: 2,
            name: "b",
        })
        .and_then(|v| v)?;

    Ok(fungui::Value::ExtValue(UValue::Color(Color {
        r: match r {
            fungui::Value::Integer(v) => v as f32 / 255.0,
            fungui::Value::Float(v) => v as f32,
            _ => {
                return Err(fungui::Error::CustomStatic {
                    reason: "Expected integer or float",
                })
            }
        },
        g: match g {
            fungui::Value::Integer(v) => v as f32 / 255.0,
            fungui::Value::Float(v) => v as f32,
            _ => {
                return Err(fungui::Error::CustomStatic {
                    reason: "Expected integer or float",
                })
            }
        },
        b: match b {
            fungui::Value::Integer(v) => v as f32 / 255.0,
            fungui::Value::Float(v) => v as f32,
            _ => {
                return Err(fungui::Error::CustomStatic {
                    reason: "Expected integer or float",
                })
            }
        },
        a: 1.0,
    })))
}

pub fn rgba<'a>(
    args: &mut (dyn Iterator<Item = fungui::FResult<'a, Value>> + 'a),
) -> fungui::FResult<'a, Value> {
    let r = args
        .next()
        .ok_or(fungui::Error::MissingParameter {
            position: 0,
            name: "r",
        })
        .and_then(|v| v)?;
    let g = args
        .next()
        .ok_or(fungui::Error::MissingParameter {
            position: 1,
            name: "g",
        })
        .and_then(|v| v)?;
    let b = args
        .next()
        .ok_or(fungui::Error::MissingParameter {
            position: 2,
            name: "b",
        })
        .and_then(|v| v)?;
    let a = args
        .next()
        .ok_or(fungui::Error::MissingParameter {
            position: 3,
            name: "a",
        })
        .and_then(|v| v)?;

    Ok(fungui::Value::ExtValue(UValue::Color(Color {
        r: match r {
            fungui::Value::Integer(v) => v as f32 / 255.0,
            fungui::Value::Float(v) => v as f32,
            _ => {
                return Err(fungui::Error::CustomStatic {
                    reason: "Expected integer or float",
                })
            }
        },
        g: match g {
            fungui::Value::Integer(v) => v as f32 / 255.0,
            fungui::Value::Float(v) => v as f32,
            _ => {
                return Err(fungui::Error::CustomStatic {
                    reason: "Expected integer or float",
                })
            }
        },
        b: match b {
            fungui::Value::Integer(v) => v as f32 / 255.0,
            fungui::Value::Float(v) => v as f32,
            _ => {
                return Err(fungui::Error::CustomStatic {
                    reason: "Expected integer or float",
                })
            }
        },
        a: match a {
            fungui::Value::Integer(v) => v as f32 / 255.0,
            fungui::Value::Float(v) => v as f32,
            _ => {
                return Err(fungui::Error::CustomStatic {
                    reason: "Expected integer or float",
                })
            }
        },
    })))
}

/// Parses hex and decimal color codes
pub fn parse_color(v: &str) -> Option<Color> {
    if v.starts_with('#') {
        let col = &v[1..];
        if col.len() == 6 || col.len() == 8 {
            Some(Color {
                r: f32::from(u8::from_str_radix(&col[..2], 16).ok()?) / 255.0,
                g: f32::from(u8::from_str_radix(&col[2..4], 16).ok()?) / 255.0,
                b: f32::from(u8::from_str_radix(&col[4..6], 16).ok()?) / 255.0,
                a: f32::from(if col.len() == 8 {
                    u8::from_str_radix(&col[6..8], 16).ok()?
                } else {
                    255
                }) / 255.0,
            })
        } else {
            None
        }
    } else if v.starts_with("rgb(") && v.ends_with(')') {
        let col = &v[4..v.len() - 1];
        let mut col = col.split(',').map(|v| v.trim());

        Some(Color {
            r: f32::from(col.next().and_then(|v| v.parse::<u8>().ok()).unwrap_or(0)) / 255.0,
            g: f32::from(col.next().and_then(|v| v.parse::<u8>().ok()).unwrap_or(0)) / 255.0,
            b: f32::from(col.next().and_then(|v| v.parse::<u8>().ok()).unwrap_or(0)) / 255.0,
            a: 1.0,
        })
    } else if v.starts_with("rgba(") && v.ends_with(')') {
        let col = &v[5..v.len() - 1];
        let mut col = col.split(',').map(|v| v.trim());

        Some(Color {
            r: f32::from(col.next().and_then(|v| v.parse::<u8>().ok()).unwrap_or(0)) / 255.0,
            g: f32::from(col.next().and_then(|v| v.parse::<u8>().ok()).unwrap_or(0)) / 255.0,
            b: f32::from(col.next().and_then(|v| v.parse::<u8>().ok()).unwrap_or(0)) / 255.0,
            a: f32::from(col.next().and_then(|v| v.parse::<u8>().ok()).unwrap_or(0)) / 255.0,
        })
    } else {
        None
    }
}
