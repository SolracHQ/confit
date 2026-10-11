//! Userdata
//! Method glue for bound userdata handles.

use crate::registry::Registry;

/// One documented userdata method.
///
/// The descriptor carries one method from the impl
/// block macro to stub emission.
pub struct MethodDescriptor {
    /// The method name.
    pub name: &'static str,
    /// The method documentation.
    pub documentation: &'static str,
    /// The method stub signature.
    pub signature: String,
}

/// Bound userdata class.
///
/// - Every implementor answers one stable class name.
/// - Every implementor answers one stub block.
/// - Collection registers the class with every
///   referenced face exactly once.
pub trait UserdataClass: mlua::UserData {
    /// Return the class name.
    fn class_name() -> &'static str;
    /// Return the stub block.
    fn class_stub() -> String;
    /// Return the collected method descriptors.
    fn methods() -> Vec<MethodDescriptor>;
    /// Register the class with every referenced face.
    ///
    /// # Errors
    ///
    /// - [`crate::error::BindingError`] for repeated registration.
    fn collect(collector: &mut Registry) -> crate::error::Result<()>;
}
