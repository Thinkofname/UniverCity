use super::*;

use serde::de::{self, Visitor};
use serde::ser::{self, Serialize};

#[derive(Debug)]
pub struct DError(pub Error);

impl ::std::error::Error for DError {
    fn description(&self) -> &'static str {
        "Error"
    }
}

impl Display for DError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl de::Error for DError {
    fn custom<T: Display>(msg: T) -> Self {
        DError(Error::Raw {
            msg: msg.to_string().into_boxed_str(),
        })
    }
}
struct TableMapAccess<'de> {
    state: &'de internal::LuaState,
    idx: i32,
}

impl<'de> de::MapAccess<'de> for TableMapAccess<'de> {
    type Error = DError;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: de::DeserializeSeed<'de>,
    {
        unsafe {
            if sys::lua_next(self.state.0, self.idx) != 0 {
                seed.deserialize(&mut Deserializer {
                    state: self.state,
                    idx: self.idx + 1,
                })
                .map(Some)
            } else {
                Ok(None)
            }
        }
    }
    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: de::DeserializeSeed<'de>,
    {
        unsafe {
            let v = seed.deserialize(&mut Deserializer {
                state: self.state,
                idx: self.idx + 2,
            });
            internal::lua_pop(self.state.0, 1);
            v
        }
    }
}

struct TableFieldAccess<'de> {
    state: &'de internal::LuaState,
    idx: i32,
    fields: &'static [&'static str],
}

impl<'de> de::SeqAccess<'de> for TableFieldAccess<'de> {
    type Error = DError;

    fn next_element_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: de::DeserializeSeed<'de>,
    {
        unsafe {
            if self.fields.is_empty() {
                Ok(None)
            } else {
                let field = self.fields[0];
                self.fields = &self.fields[1..];

                internal::push_string(self.state.0, field);
                sys::lua_rawget(self.state.0, self.idx);
                let v = seed
                    .deserialize(&mut Deserializer {
                        state: self.state,
                        idx: self.idx + 1,
                    })
                    .map(Some);
                internal::lua_pop(self.state.0, 1);
                v
            }
        }
    }
}

struct TableSeqAccess<'de> {
    state: &'de internal::LuaState,
    idx: i32,
    ended: bool,
}

impl<'de> de::SeqAccess<'de> for TableSeqAccess<'de> {
    type Error = DError;

    fn next_element_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: de::DeserializeSeed<'de>,
    {
        unsafe {
            if self.ended {
                Ok(None)
            } else if sys::lua_next(self.state.0, self.idx) != 0 {
                let v = seed
                    .deserialize(&mut Deserializer {
                        state: self.state,
                        idx: self.idx + 2,
                    })
                    .map(Some);
                internal::lua_pop(self.state.0, 1);
                v
            } else {
                self.ended = true;
                Ok(None)
            }
        }
    }
}

pub struct Deserializer<'de> {
    pub(crate) state: &'de internal::LuaState,
    pub(crate) idx: i32,
}

fn type_name(state: *mut sys::lua_State, ty: i32) -> String {
    unsafe {
        let ty_name = sys::lua_typename(state, ty);
        let ty_name = CStr::from_ptr(ty_name);
        ty_name.to_str().unwrap_or("").into()
    }
}

impl<'de, 'a> de::Deserializer<'de> for &'a mut Deserializer<'de> {
    type Error = DError;
    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        const TABLE: i32 = sys::LUA_TTABLE as i32;
        const NUMBER: i32 = sys::LUA_TNUMBER as i32;
        const STRING: i32 = sys::LUA_TSTRING as i32;
        const BOOLEAN: i32 = sys::LUA_TBOOLEAN as i32;
        unsafe {
            match sys::lua_type(self.state.0, self.idx) {
                BOOLEAN => self.deserialize_bool(visitor),
                NUMBER => self.deserialize_f64(visitor),
                STRING => self.deserialize_str(visitor),
                TABLE => self.deserialize_map(visitor),
                ty => Err(DError(Error::UnsupportedDynamicType {
                    ty: format!("lua {}", type_name(self.state.0, ty)),
                })),
            }
        }
    }
    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        unsafe { visitor.visit_bool(sys::lua_toboolean(self.state.0, self.idx) != 0) }
    }
    fn deserialize_i8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_i64(visitor)
    }
    fn deserialize_i16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_i64(visitor)
    }
    fn deserialize_i32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_i64(visitor)
    }
    fn deserialize_i64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        unsafe {
            if sys::lua_isnumber(self.state.0, self.idx) != 0 {
                visitor.visit_i64(sys::lua_tonumber(self.state.0, self.idx) as i64)
            } else {
                Err(DError(Error::TypeMismatch { wanted: "Number" }))
            }
        }
    }
    fn deserialize_u8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_u64(visitor)
    }
    fn deserialize_u16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_u64(visitor)
    }
    fn deserialize_u32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_u64(visitor)
    }
    fn deserialize_u64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        unsafe {
            if sys::lua_isnumber(self.state.0, self.idx) != 0 {
                visitor.visit_u64(sys::lua_tonumber(self.state.0, self.idx) as u64)
            } else {
                Err(DError(Error::TypeMismatch { wanted: "Number" }))
            }
        }
    }
    fn deserialize_f32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        unsafe {
            if sys::lua_isnumber(self.state.0, self.idx) != 0 {
                visitor.visit_f32(sys::lua_tonumber(self.state.0, self.idx) as f32)
            } else {
                Err(DError(Error::TypeMismatch { wanted: "Number" }))
            }
        }
    }
    fn deserialize_f64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        unsafe {
            if sys::lua_isnumber(self.state.0, self.idx) != 0 {
                visitor.visit_f64(sys::lua_tonumber(self.state.0, self.idx) as f64)
            } else {
                Err(DError(Error::TypeMismatch { wanted: "Number" }))
            }
        }
    }
    fn deserialize_char<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(DError(Error::UnsupportedType { ty: "char" }))
    }
    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        unsafe {
            if sys::lua_isstring(self.state.0, self.idx) != 0 {
                let cstr =
                    CStr::from_ptr(sys::lua_tolstring(self.state.0, self.idx, ptr::null_mut()));
                visitor.visit_str(cstr.to_str().unwrap_or(""))
            } else {
                Err(DError(Error::TypeMismatch { wanted: "String" }))
            }
        }
    }
    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }
    fn deserialize_bytes<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(DError(Error::UnsupportedType { ty: "bytes" }))
    }
    fn deserialize_byte_buf<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(DError(Error::UnsupportedType { ty: "byte buf" }))
    }
    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        unsafe {
            if sys::lua_type(self.state.0, self.idx) == i32::from(sys::LUA_TNIL) {
                visitor.visit_none()
            } else {
                visitor.visit_some(self)
            }
        }
    }
    fn deserialize_unit<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(DError(Error::UnsupportedType { ty: "unit" }))
    }
    fn deserialize_unit_struct<V>(
        self,
        _name: &'static str,
        _visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(DError(Error::UnsupportedType { ty: "unit struct" }))
    }
    fn deserialize_newtype_struct<V>(
        self,
        _name: &'static str,
        _visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(DError(Error::UnsupportedType {
            ty: "newtype struct",
        }))
    }
    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        unsafe {
            let access = TableSeqAccess {
                state: self.state,
                idx: self.idx,
                ended: false,
            };
            sys::lua_pushnil(self.state.0);
            visitor.visit_seq(access)
        }
    }
    fn deserialize_tuple<V>(self, _len: usize, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(DError(Error::UnsupportedType { ty: "tuple" }))
    }
    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        _len: usize,
        _visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(DError(Error::UnsupportedType { ty: "tuple struct" }))
    }
    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        unsafe {
            if sys::lua_type(self.state.0, self.idx) == i32::from(sys::LUA_TTABLE) {
                let access = TableMapAccess {
                    state: self.state,
                    idx: self.idx,
                };
                sys::lua_pushnil(self.state.0);
                visitor.visit_map(access)
            } else {
                Err(DError(Error::TypeMismatch { wanted: "Table" }))
            }
        }
    }
    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        unsafe {
            if sys::lua_type(self.state.0, self.idx) == i32::from(sys::LUA_TTABLE) {
                let access = TableFieldAccess {
                    state: self.state,
                    idx: self.idx,
                    fields,
                };
                visitor.visit_seq(access)
            } else {
                Err(DError(Error::TypeMismatch { wanted: "Table" }))
            }
        }
    }
    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        _visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(DError(Error::UnsupportedType { ty: "enum" }))
    }
    fn deserialize_identifier<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(DError(Error::UnsupportedType { ty: "identifier" }))
    }
    fn deserialize_ignored_any<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(DError(Error::UnsupportedType { ty: "ignored any" }))
    }
}

#[derive(Debug)]
pub struct SError(pub Error);

impl ::std::error::Error for SError {
    fn description(&self) -> &'static str {
        "Error"
    }
}

impl Display for SError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl ser::Error for SError {
    fn custom<T: Display>(msg: T) -> Self {
        SError(Error::Raw {
            msg: msg.to_string().into_boxed_str(),
        })
    }
}

pub struct Serializer<'se> {
    pub(crate) state: &'se internal::LuaState,
}

impl<'a, 'se> ser::Serializer for &'a mut Serializer<'se> {
    type Ok = ();
    type Error = SError;

    type SerializeSeq = SeqSerializer<'se>;
    type SerializeTuple = ser::Impossible<(), SError>;
    type SerializeTupleStruct = ser::Impossible<(), SError>;
    type SerializeTupleVariant = ser::Impossible<(), SError>;
    type SerializeMap = MapSerializer<'se>;
    type SerializeStruct = StructSerializer<'se>;
    type SerializeStructVariant = ser::Impossible<(), SError>;

    fn serialize_bool(self, v: bool) -> Result<Self::Ok, Self::Error> {
        unsafe {
            sys::lua_pushboolean(self.state.0, if v { 1 } else { 0 });
            Ok(())
        }
    }

    fn serialize_i8(self, v: i8) -> Result<Self::Ok, Self::Error> {
        self.serialize_f64(f64::from(v))
    }

    fn serialize_i16(self, v: i16) -> Result<Self::Ok, Self::Error> {
        self.serialize_f64(f64::from(v))
    }

    fn serialize_i32(self, v: i32) -> Result<Self::Ok, Self::Error> {
        self.serialize_f64(f64::from(v))
    }

    fn serialize_i64(self, v: i64) -> Result<Self::Ok, Self::Error> {
        self.serialize_f64(v as f64)
    }

    fn serialize_u8(self, v: u8) -> Result<Self::Ok, Self::Error> {
        self.serialize_f64(f64::from(v))
    }

    fn serialize_u16(self, v: u16) -> Result<Self::Ok, Self::Error> {
        self.serialize_f64(f64::from(v))
    }

    fn serialize_u32(self, v: u32) -> Result<Self::Ok, Self::Error> {
        self.serialize_f64(f64::from(v))
    }

    fn serialize_u64(self, v: u64) -> Result<Self::Ok, Self::Error> {
        self.serialize_f64(v as f64)
    }

    fn serialize_f32(self, v: f32) -> Result<Self::Ok, Self::Error> {
        self.serialize_f64(f64::from(v))
    }

    fn serialize_f64(self, v: f64) -> Result<Self::Ok, Self::Error> {
        unsafe {
            sys::lua_pushnumber(self.state.0, v);
            Ok(())
        }
    }

    fn serialize_char(self, _v: char) -> Result<Self::Ok, Self::Error> {
        Err(SError(Error::UnsupportedType { ty: "char" }))
    }

    fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
        unsafe {
            internal::push_string(self.state.0, v);
            Ok(())
        }
    }

    fn serialize_bytes(self, _v: &[u8]) -> Result<Self::Ok, Self::Error> {
        Err(SError(Error::UnsupportedType { ty: "bytes" }))
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        unsafe {
            sys::lua_pushnil(self.state.0);
            Ok(())
        }
    }

    fn serialize_some<T: ?Sized>(self, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: Serialize,
    {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Err(SError(Error::UnsupportedType { ty: "unit" }))
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, Self::Error> {
        Err(SError(Error::UnsupportedType { ty: "unit struct" }))
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        Err(SError(Error::UnsupportedType {
            ty: "unit struct variant",
        }))
    }

    fn serialize_newtype_struct<T: ?Sized>(
        self,
        _name: &'static str,
        _value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: Serialize,
    {
        Err(SError(Error::UnsupportedType {
            ty: "newtype struct",
        }))
    }

    fn serialize_newtype_variant<T: ?Sized>(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: Serialize,
    {
        Err(SError(Error::UnsupportedType {
            ty: "newtype variant",
        }))
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        unsafe {
            sys::lua_createtable(self.state.0, 0, len.unwrap_or(0) as i32);
            Ok(SeqSerializer {
                state: self.state,
                idx: 1,
            })
        }
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Err(SError(Error::UnsupportedType { ty: "tuple" }))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Err(SError(Error::UnsupportedType { ty: "tuple struct" }))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Err(SError(Error::UnsupportedType {
            ty: "tuple variant",
        }))
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        unsafe {
            sys::lua_createtable(self.state.0, 0, len.unwrap_or(0) as _);
            Ok(MapSerializer { state: self.state })
        }
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        unsafe {
            sys::lua_createtable(self.state.0, 0, len as _);
            Ok(StructSerializer { state: self.state })
        }
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Err(SError(Error::UnsupportedType {
            ty: "struct variant",
        }))
    }
}

pub struct StructSerializer<'se> {
    state: &'se internal::LuaState,
}

impl<'se> ser::SerializeStruct for StructSerializer<'se> {
    type Ok = ();
    type Error = SError;
    fn serialize_field<T: ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error>
    where
        T: Serialize,
    {
        unsafe {
            internal::push_string(self.state.0, key);
            value.serialize(&mut Serializer { state: self.state })?;
            sys::lua_rawset(self.state.0, -3);
            Ok(())
        }
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

pub struct MapSerializer<'se> {
    state: &'se internal::LuaState,
}
impl<'se> ser::SerializeMap for MapSerializer<'se> {
    type Ok = ();
    type Error = SError;

    fn serialize_key<T: ?Sized>(&mut self, key: &T) -> Result<(), Self::Error>
    where
        T: Serialize,
    {
        key.serialize(&mut Serializer { state: self.state })?;
        Ok(())
    }
    fn serialize_value<T: ?Sized>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: Serialize,
    {
        unsafe {
            value.serialize(&mut Serializer { state: self.state })?;
            sys::lua_rawset(self.state.0, -3);
            Ok(())
        }
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

pub struct SeqSerializer<'se> {
    state: &'se internal::LuaState,
    idx: i32,
}

impl<'se> ser::SerializeSeq for SeqSerializer<'se> {
    type Ok = ();
    type Error = SError;
    fn serialize_element<T: ?Sized>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: Serialize,
    {
        unsafe {
            value.serialize(&mut Serializer { state: self.state })?;
            sys::lua_rawseti(self.state.0, -2, self.idx);
            self.idx += 1;
            Ok(())
        }
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_de() {
        let lua = Lua::new();
        let test: Ref<Table> = lua
            .execute_string(
                r#"
return {
    hello = 5,
    world = 3,
    bool = true,
    opt2 = 33,
    testing = {
        a = 1,
        b = 2,
        c = 3,
        message = "Hello world",
    },
    list = {3, 4, 5, 6, 7}
}
        "#,
            )
            .unwrap();
        #[derive(Debug, Deserialize)]
        struct Test {
            hello: i32,
            world: u8,
            #[serde(rename = "bool")]
            bool_test: bool,
            testing: Inner,
            opt: Option<i32>,
            opt2: Option<i32>,
            list: Vec<u8>,
        }
        #[derive(Debug, Deserialize)]
        struct Inner {
            a: i32,
            b: i32,
            c: i32,
            message: String,
        }
        let val: Test = from_table(&test).unwrap();
        println!("{:#?}", val);
    }

    #[test]
    fn test_se() {
        let lua = Lua::new();
        lua.execute_string::<()>(
            r#"
function print_table(tbl)
    for k, v in pairs(tbl) do
        print("Key: " .. tostring(k))
        print("Value: ")
        print_val(v)
    end
end
function print_val(val)
    local ty = type(val)
    if ty == "table" then
        print_table(val)
    else
        print(val)
    end
end
        "#,
        )
        .unwrap();
        #[derive(Debug, Serialize)]
        struct Test<'a> {
            hello: i32,
            world: u8,
            message: &'a str,
            bool_test: bool,
            opt: Option<i32>,
            opt2: Option<i32>,
            inner: Inner<'a>,
            list: &'a [i32],
        }
        #[derive(Debug, Serialize)]
        struct Inner<'a> {
            a: i32,
            b: i32,
            c: i32,
            message: &'a str,
        }
        let inner = Inner {
            a: 3,
            b: 4,
            c: 5,
            message: "banana",
        };
        let tbl = to_table(
            &lua,
            &Test {
                hello: 5,
                world: 44,
                message: "Hello world!!!",
                bool_test: true,
                opt: None,
                opt2: Some(88),
                inner: inner,
                list: &[3, 2, 7, 8],
            },
        );
        lua.invoke_function::<_, ()>("print_table", tbl).unwrap();
    }
}
