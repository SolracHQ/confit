//! Face
//! Declaration traits for Lua table and userdata faces.

use std::borrow::Cow;

use crate::registry::Registry;

/// One documented table field.
///
/// The descriptor carries the optional marker from
/// the shape declaration to stub emission.
pub struct FieldDescriptor {
    /// The field name.
    pub name: &'static str,
    /// The field documentation.
    pub documentation: &'static str,
    /// The field face expression.
    pub face: Cow<'static, str>,
    /// The optional marker.
    pub optional: bool,
}

/// One documented union variant.
///
/// The descriptor carries one member from the union
/// declaration to stub emission.
pub struct VariantDescriptor {
    /// The variant name.
    pub name: &'static str,
    /// The variant documentation.
    pub documentation: &'static str,
    /// The variant face expression.
    pub face: Cow<'static, str>,
}

/// Face kind marker.
///
/// Every declaration carries one marker from
/// derive to stub emission.
pub enum FaceKind {
    /// Table shape.
    Table,
    /// Userdata handle.
    Userdata,
}

/// Bound type face.
///
/// - Every implementor answers one stable Lua name.
/// - Every implementor answers one stub block.
/// - Every implementor declares one kind marker.
/// - Collection registers the face with every
///   referenced face exactly once.
pub trait Face {
    /// Return the Lua name.
    fn face_name() -> String;
    /// Return the stub block.
    fn face_stub() -> String;
    /// Return the kind marker.
    fn kind() -> FaceKind;
    /// Register the face with every referenced face.
    ///
    /// # Errors
    ///
    /// - [`crate::error::BindingError`] for repeated registration.
    fn collect(collector: &mut Registry) -> crate::error::Result<()>;
}

/// Bound namespace face.
///
/// - Every implementor answers one dotted Lua path.
/// - Every implementor answers one documentation string.
/// - Collection registers the namespace with every
///   member face exactly once.
pub trait Namespace {
    /// Return the dotted Lua path.
    fn namespace() -> &'static str;
    /// Return the documentation text.
    fn documentation() -> &'static str;
    /// Register the namespace with every member face.
    ///
    /// # Errors
    ///
    /// - [`crate::error::BindingError`] for repeated registration.
    fn collect(collector: &mut Registry) -> crate::error::Result<()>;
}

macro_rules! primitive_face {
    ($type:ty, $face:literal) => {
        impl Face for $type {
            fn face_name() -> String {
                $face.to_owned()
            }
            fn face_stub() -> String {
                String::new()
            }
            fn kind() -> FaceKind {
                FaceKind::Table
            }
            fn collect(_collector: &mut Registry) -> crate::error::Result<()> {
                Ok(())
            }
        }
    };
}

primitive_face!(String, "string");
primitive_face!(bool, "boolean");
primitive_face!(i64, "integer");
primitive_face!(u64, "integer");
primitive_face!(f64, "number");
primitive_face!(mlua::Function, "fun()");
primitive_face!(mlua::Value, "any");

impl Face for () {
    fn face_name() -> String {
        "nil".to_owned()
    }
    fn face_stub() -> String {
        String::new()
    }
    fn kind() -> FaceKind {
        FaceKind::Table
    }
    fn collect(_collector: &mut Registry) -> crate::error::Result<()> {
        Ok(())
    }
}

impl<First: Face, Second: Face> Face for (First, Second) {
    fn face_name() -> String {
        format!("{}, {}", First::face_name(), Second::face_name())
    }
    fn face_stub() -> String {
        String::new()
    }
    fn kind() -> FaceKind {
        First::kind()
    }
    fn collect(collector: &mut Registry) -> crate::error::Result<()> {
        First::collect(collector)?;
        Second::collect(collector)
    }
}

impl<T: Face> Face for Option<T> {
    fn face_name() -> String {
        format!("{}?", parenthesized(&T::face_name()))
    }
    fn face_stub() -> String {
        String::new()
    }
    fn kind() -> FaceKind {
        T::kind()
    }
    fn collect(collector: &mut Registry) -> crate::error::Result<()> {
        T::collect(collector)
    }
}

impl<T: Face> Face for Vec<T> {
    fn face_name() -> String {
        format!("{}[]", parenthesized(&T::face_name()))
    }
    fn face_stub() -> String {
        String::new()
    }
    fn kind() -> FaceKind {
        T::kind()
    }
    fn collect(collector: &mut Registry) -> crate::error::Result<()> {
        T::collect(collector)
    }
}

impl<T: Face> Face for Box<T> {
    fn face_name() -> String {
        T::face_name()
    }
    fn face_stub() -> String {
        String::new()
    }
    fn kind() -> FaceKind {
        T::kind()
    }
    fn collect(collector: &mut Registry) -> crate::error::Result<()> {
        T::collect(collector)
    }
}

fn parenthesized(face: &str) -> String {
    if face.contains(" | ") {
        format!("({face})")
    } else {
        face.to_owned()
    }
}
