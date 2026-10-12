//! Verb
//! Verb glue for bound Lua functions.

use std::borrow::Cow;

use crate::registry::Registry;

/// One verb descriptor.
///
/// The descriptor travels from the verb face to the
/// registry and the stub emitter.
pub struct VerbDescriptor {
    /// The verb name.
    pub name: &'static str,
    /// The verb documentation.
    pub documentation: &'static str,
}

/// One documented verb parameter.
///
/// The descriptor carries the optional marker from
/// the verb declaration to stub emission.
pub struct ParameterDescriptor {
    /// The parameter name.
    pub name: &'static str,
    /// The parameter documentation.
    pub documentation: &'static str,
    /// The parameter face expression.
    pub face: Cow<'static, str>,
    /// The optional marker.
    pub optional: bool,
}

/// One documented verb return.
///
/// The descriptor carries one return slot from the
/// verb declaration to stub emission.
pub struct ReturnDescriptor {
    /// The return face expression.
    pub face: Cow<'static, str>,
    /// The return documentation.
    pub documentation: &'static str,
}

/// Bound verb face.
///
/// - Every implementor answers one stable verb name.
/// - Every implementor answers one stub block.
/// - Collection registers the verb with every
///   referenced face exactly once.
pub trait Verb {
    /// Return the verb descriptor.
    fn descriptor() -> VerbDescriptor;
    /// Return the stub block.
    fn stub() -> String;
    /// Register the verb with every referenced face.
    ///
    /// # Errors
    ///
    /// - [`crate::error::BindingError`] for repeated registration.
    fn collect(collector: &mut Registry) -> crate::error::Result<()>;
}

/// Declare one Lua verb with its call.
///
/// - The verb name reads the function ident.
/// - Verb plus parameter plus return docs read `///` lines.
/// - Parameter plus return faces read declared bounds.
/// - The optional marker follows the parameter docs.
/// - An optional leading `lua: &Lua` param passes the Lua context
///   through and stays out of the stub.
/// - The call runs per invocation with params decoded through
///   Decode and returns encoded through Encode.
/// - The function closes over nothing.
#[macro_export]
macro_rules! verb {
    (
        $(#[doc = $verb_documentation:literal])+
        fn $verb:ident (
            $lua:ident : &Lua
            $(,
                $(#[doc = $parameter_documentation:literal])+
                $(#[face($marker:ident)])?
                $parameter:ident : $parameter_type:ty
            )*
            $(,)?
        ) -> (
            $(
                $(#[doc = $return_documentation:literal])*
                $return_type:ty
            ),* $(,)?
        ) [call = $call:path]
    ) => {
        $(#[doc = $verb_documentation])*
        pub mod $verb {
            use super::*;

            $crate::__verb_face! {
                $(#[doc = $verb_documentation])+
                fn $verb
                [$( $(#[doc = $parameter_documentation])+ $(#[face($marker)])? $parameter : $parameter_type ),*]
                [$( $(#[doc = $return_documentation])* $return_type ),*]
            }

            /// Build the verb function closing over nothing.
            ///
            /// # Errors
            ///
            /// - [`::mlua::Error`] for Lua failures.
            pub fn function(lua: &::mlua::Lua) -> ::mlua::Result<::mlua::Function> {
                lua.create_function(
                    |$lua, arguments: ::mlua::Variadic<::mlua::Value>| {
                        let mut next = 0usize;
                        $(
                            let $parameter: $parameter_type = match $crate::value::take_arg(
                                &arguments,
                                &mut next,
                            ) {
                                ::std::option::Option::Some(raw) => {
                                    <$parameter_type as $crate::value::Decode>::decode(
                                        &raw, $lua,
                                    )
                                    .map_err(|error| {
                                        ::mlua::Error::runtime(format!(
                                            "{}: {error}",
                                            stringify!($parameter),
                                        ))
                                    })?
                                }
                                ::std::option::Option::None => $crate::__verb_missing!(
                                    $parameter : $parameter_type $(, $marker)?
                                ),
                            };
                        )*
                        let _ = &mut next;
                        let outcome: ($($return_type,)*) =
                            $call($lua, $($parameter,)*)?;
                        Ok($crate::__verb_encode!($lua, outcome, $($return_type),*))
                    },
                )
            }
        }
    };
    (
        $(#[doc = $verb_documentation:literal])+
        fn $verb:ident (
            $(
                $(#[doc = $parameter_documentation:literal])+
                $(#[face($marker:ident)])?
                $parameter:ident : $parameter_type:ty
            ),* $(,)?
        ) -> (
            $(
                $(#[doc = $return_documentation:literal])*
                $return_type:ty
            ),* $(,)?
        ) [call = $call:path]
    ) => {
        $(#[doc = $verb_documentation])*
        pub mod $verb {
            use super::*;

            $crate::__verb_face! {
                $(#[doc = $verb_documentation])+
                fn $verb
                [$( $(#[doc = $parameter_documentation])+ $(#[face($marker)])? $parameter : $parameter_type ),*]
                [$( $(#[doc = $return_documentation])* $return_type ),*]
            }

            /// Build the verb function closing over nothing.
            ///
            /// # Errors
            ///
            /// - [`::mlua::Error`] for Lua failures.
            pub fn function(lua: &::mlua::Lua) -> ::mlua::Result<::mlua::Function> {
                lua.create_function(
                    |lua, arguments: ::mlua::Variadic<::mlua::Value>| {
                        let mut next = 0usize;
                        $(
                            let $parameter: $parameter_type = match $crate::value::take_arg(
                                &arguments,
                                &mut next,
                            ) {
                                ::std::option::Option::Some(raw) => {
                                    <$parameter_type as $crate::value::Decode>::decode(
                                        &raw, lua,
                                    )
                                    .map_err(|error| {
                                        ::mlua::Error::runtime(format!(
                                            "{}: {error}",
                                            stringify!($parameter),
                                        ))
                                    })?
                                }
                                ::std::option::Option::None => $crate::__verb_missing!(
                                    $parameter : $parameter_type $(, $marker)?
                                ),
                            };
                        )*
                        let _ = &mut next;
                        let outcome: ($($return_type,)*) =
                            $call($($parameter,)*)?;
                        Ok($crate::__verb_encode!(lua, outcome, $($return_type),*))
                    },
                )
            }
        }
    };
}

/// Expand one verb declaration into its face with its registration.
#[doc(hidden)]
#[macro_export]
macro_rules! __verb_face {
    (
        $(#[doc = $verb_documentation:literal])+
        fn $verb:ident
        [$( $(#[doc = $parameter_documentation:literal])+ $(#[face($marker:ident)])? $parameter:ident : $parameter_type:ty ),*]
        [$( $(#[doc = $return_documentation:literal])* $return_type:ty ),*]
    ) => {
        /// Marker carrying the verb face.
        pub struct Verb;

        impl $crate::verb::Verb for Verb {
            fn descriptor() -> $crate::verb::VerbDescriptor {
                $crate::verb::VerbDescriptor {
                    name: stringify!($verb),
                    documentation: concat!($($verb_documentation),*),
                }
            }

            fn stub() -> String {
                $crate::stub::emit_function(
                    &Self::descriptor(),
                    &[
                        $($crate::verb::ParameterDescriptor {
                            name: stringify!($parameter),
                            documentation: concat!($($parameter_documentation),*),
                            face: ::std::borrow::Cow::Owned(
                                <$parameter_type as $crate::face::Face>::face_name(),
                            ),
                            optional: $crate::__verb_optional!($($marker)?),
                        },)*
                    ],
                    &[
                        $($crate::verb::ReturnDescriptor {
                            face: ::std::borrow::Cow::Owned(
                                <$return_type as $crate::face::Face>::face_name(),
                            ),
                            documentation: concat!($($return_documentation),*),
                        },)*
                    ],
                )
            }

            fn collect(
                collector: &mut $crate::registry::Registry,
            ) -> $crate::error::Result<()> {
                if collector.register(Self::descriptor().name.to_owned(), Self::stub()) {
                    $(<$parameter_type as $crate::face::Face>::collect(collector)?;)*
                    $(<$return_type as $crate::face::Face>::collect(collector)?;)*
                }
                Ok(())
            }
        }

        /// Register the verb on one Lua table.
        ///
        /// # Errors
        ///
        /// - [`::mlua::Error`] for Lua failures.
        pub fn register(
            lua: &::mlua::Lua,
            table: &::mlua::Table,
        ) -> ::mlua::Result<()> {
            table.set(stringify!($verb), function(lua)?)
        }
    };
}

/// Encode one verb outcome into its Lua returns.
#[doc(hidden)]
#[macro_export]
macro_rules! __verb_encode {
    ($lua:ident, $outcome:ident $(,)?) => {
        ::mlua::MultiValue::new()
    };
    ($lua:ident, $outcome:ident, $first:ty $(,)?) => {{
        let mut returns = ::mlua::MultiValue::new();
        returns.push_back(<$first as $crate::value::Encode>::encode(
            &$outcome.0,
            $lua,
        )?);
        returns
    }};
    ($lua:ident, $outcome:ident, $first:ty, $second:ty $(,)?) => {{
        let mut returns = ::mlua::MultiValue::new();
        returns.push_back(<$first as $crate::value::Encode>::encode(
            &$outcome.0,
            $lua,
        )?);
        returns.push_back(<$second as $crate::value::Encode>::encode(
            &$outcome.1,
            $lua,
        )?);
        returns
    }};
    ($lua:ident, $outcome:ident, $($rest:ty),+) => {{
        compile_error!("verbs answer up to two returns");
        ::mlua::MultiValue::new()
    }};
}

/// Answer the optional flag for one verb marker.
#[doc(hidden)]
#[macro_export]
macro_rules! __verb_optional {
    () => {
        false
    };
    (optional) => {
        true
    };
    ($other:ident) => {{
        compile_error!("verb params take #[face(optional)] alone");
        false
    }};
}

/// Answer the missing-argument arm for one verb parameter.
#[doc(hidden)]
#[macro_export]
macro_rules! __verb_missing {
    ($parameter:ident : $parameter_type:ty) => {
        return ::std::result::Result::Err(::mlua::Error::runtime(format!(
            "missing param '{}'",
            stringify!($parameter),
        )))
    };
    ($parameter:ident : $parameter_type:ty, optional) => {
        <$parameter_type as ::std::default::Default>::default()
    };
    ($parameter:ident : $parameter_type:ty, $other:ident) => {{
        compile_error!("verb params take #[face(optional)] alone");
        <$parameter_type as ::std::default::Default>::default()
    }};
}
