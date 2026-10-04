//! Version
//! One integer naming the Lua surface level.

use mlua::Value;

/// Api level of the Lua surface.
pub const API_VERSION: i64 = 1;

/// Returns the version integer.
pub fn value() -> Value {
    Value::Integer(API_VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_one_integer() {
        assert_eq!(API_VERSION, 1);
        assert_eq!(value(), Value::Integer(1));
    }
}
