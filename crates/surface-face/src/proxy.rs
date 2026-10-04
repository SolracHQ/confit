//! Proxy
//! Lua presentation shapes converting into Rust values. The face with the Lua
//! read comes from the presentation shape.

use std::fmt::Display;
use std::marker::PhantomData;

use mlua::{FromLua, Lua, Value};
use mlua_extras::typed::{Type, Typed};
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::Deserialize;
use std::borrow::Cow;

/// One Lua string holding text.
///
/// Strict strings fail them naming the received type.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
pub struct StrictString(pub String);

impl Typed for StrictString {
    fn ty() -> Type {
        Type::string()
    }
}

impl FromLua for StrictString {
    fn from_lua(value: Value, _lua: &Lua) -> mlua::Result<Self> {
        match value {
            Value::String(found) => Ok(Self(found.to_string_lossy())),
            _ => Err(mlua::Error::FromLuaConversionError {
                from: value.type_name(),
                to: "string".to_owned(),
                message: None,
            }),
        }
    }
}

/// One presentation conversion into its Rust value.
pub trait Convert<P, T> {
    /// Conversion failure rendering both channels.
    type Error: Display + Into<mlua::Error>;

    /// Converts one presentation shape into its Rust value.
    fn convert(presentation: P) -> Result<T, Self::Error>;
}

/// One Lua presentation shape converting into its Rust value.
///
/// P returns the face with the Lua read, the converter returns the Rust value
/// with the failure.
pub struct ProxyType<P, T, C>(pub T, PhantomData<(P, C)>);

impl<P, T, C> std::fmt::Debug for ProxyType<P, T, C>
where
    T: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ProxyType").field(&self.0).finish()
    }
}

impl<P, T, C> Clone for ProxyType<P, T, C>
where
    T: Clone,
{
    fn clone(&self) -> Self {
        Self(self.0.clone(), PhantomData)
    }
}

impl<P, T, C> Default for ProxyType<P, T, C>
where
    T: Default,
{
    fn default() -> Self {
        Self(T::default(), PhantomData)
    }
}

impl<P, T, C> ProxyType<P, T, C> {
    /// Builds one presentation shape holding its converted value.
    pub fn new(value: T) -> Self {
        Self(value, PhantomData)
    }

    /// Reads the converted Rust value.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<P, T, C> PartialEq for ProxyType<P, T, C>
where
    T: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<P, T, C> Eq for ProxyType<P, T, C> where T: Eq {}

impl<P, T, C> Typed for ProxyType<P, T, C>
where
    P: Typed,
    C: Convert<P, T>,
{
    fn ty() -> Type {
        P::ty()
    }
}

impl<P, T, C> JsonSchema for ProxyType<P, T, C>
where
    P: JsonSchema,
    C: Convert<P, T>,
{
    fn schema_name() -> Cow<'static, str> {
        P::schema_name()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        P::json_schema(generator)
    }

    fn inline_schema() -> bool {
        P::inline_schema()
    }
}

impl<'de, P, T, C> Deserialize<'de> for ProxyType<P, T, C>
where
    P: Deserialize<'de>,
    C: Convert<P, T>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let presentation = P::deserialize(deserializer)?;
        C::convert(presentation)
            .map(|value| Self(value, PhantomData))
            .map_err(serde::de::Error::custom)
    }
}

impl<P, T, C> FromLua for ProxyType<P, T, C>
where
    P: FromLua,
    C: Convert<P, T>,
{
    fn from_lua(value: Value, lua: &Lua) -> mlua::Result<Self> {
        let presentation = P::from_lua(value, lua)?;
        C::convert(presentation)
            .map(|value| Self(value, PhantomData))
            .map_err(C::Error::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct ModeError(String);

    impl Display for ModeError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.0)
        }
    }

    impl From<ModeError> for mlua::Error {
        fn from(error: ModeError) -> Self {
            mlua::Error::RuntimeError(error.0)
        }
    }

    struct ParseBits;

    impl Convert<Option<String>, Option<u32>> for ParseBits {
        type Error = ModeError;

        fn convert(found: Option<String>) -> Result<Option<u32>, ModeError> {
            match found {
                None => Ok(None),
                Some(raw) => match u32::from_str_radix(&raw, 8) {
                    Ok(bits) => Ok(Some(bits)),
                    Err(_) => Err(ModeError(format!("bad mode '{raw}'"))),
                },
            }
        }
    }

    type Mode = ProxyType<Option<String>, Option<u32>, ParseBits>;

    #[test]
    fn strict_strings_refuse_numbers() {
        let lua = Lua::new();
        let found =
            StrictString::from_lua(Value::String(lua.create_string("hi").unwrap()), &lua).unwrap();
        assert_eq!(found.0, "hi");
        assert_eq!(StrictString::ty(), Type::string());
        match StrictString::from_lua(Value::Integer(1), &lua) {
            Ok(_) => panic!("coerced number passes"),
            Err(error) => assert!(error.to_string().contains("integer")),
        }
    }

    #[test]
    fn face_returns_presentation_shape() {
        assert_eq!(Mode::ty(), Type::union(vec![Type::string(), Type::nil()]));
    }

    #[test]
    fn lua_read_converts_presentation() {
        let lua = Lua::new();
        let found = Mode::from_lua(Value::String(lua.create_string("755").unwrap()), &lua).unwrap();
        assert_eq!(found.into_inner(), Some(0o755));
        match Mode::from_lua(Value::String(lua.create_string("nope").unwrap()), &lua) {
            Ok(_) => panic!("bad mode passes"),
            Err(error) => assert!(error.to_string().contains("bad mode")),
        }
    }

    #[test]
    fn serde_read_converts_presentation() {
        let found: Mode = serde_json::from_value(serde_json::json!("755")).unwrap();
        assert_eq!(found.into_inner(), Some(0o755));
        match serde_json::from_value::<Mode>(serde_json::json!("nope")) {
            Ok(_) => panic!("bad mode passes"),
            Err(error) => assert!(error.to_string().contains("bad mode")),
        }
    }
}
