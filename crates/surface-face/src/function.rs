//! Function face
//! Marker faces for Lua functions.

/// Declare one marker with its function face over param faces.
///
/// Param names come from idents with docs from doc lines.
#[macro_export]
macro_rules! function_face {
    (
        $(#[$marker_meta:meta])*
        $visibility:vis struct $marker:ident {
            $(
                $(#[doc = $doc:literal])*
                $param:ident : $param_ty:ty
            ),* $(,)?
        }
    ) => {
        $(#[$marker_meta])*
        #[allow(dead_code)]
        $visibility struct $marker(::std::marker::PhantomData<()>);

        impl ::mlua_extras::typed::Typed for $marker {
            fn ty() -> ::mlua_extras::typed::Type {
                ::mlua_extras::typed::Type::Function {
                    params: ::std::vec![
                        $(
                            ::mlua_extras::typed::Param {
                                name: ::std::option::Option::Some(
                                    ::std::convert::Into::into(stringify!($param)),
                                ),
                                doc: {
                                    let docs: &[&str] = &[$($doc),*];
                                    if docs.is_empty() {
                                        ::std::option::Option::None
                                    } else {
                                        ::std::option::Option::Some(
                                            ::std::convert::Into::into(
                                                docs.iter()
                                                    .map(|line| line.trim())
                                                    .collect::<::std::vec::Vec<_>>()
                                                    .join("\n"),
                                            ),
                                        )
                                    }
                                },
                                ty: <$param_ty as ::mlua_extras::typed::Typed>::ty(),
                            }
                        ),*
                    ],
                    returns: ::std::vec![],
                }
            }
        }
    };
    (
        $(#[$marker_meta:meta])*
        $visibility:vis struct $marker:ident < $generic:ident > {
            $(
                $(#[doc = $doc:literal])*
                $param:ident : $param_ty:ty
            ),* $(,)?
        }
    ) => {
        $(#[$marker_meta])*
        #[allow(dead_code)]
        $visibility struct $marker<$generic>(::std::marker::PhantomData<$generic>);

        impl<$generic: ::mlua_extras::typed::Typed> ::mlua_extras::typed::Typed
            for $marker<$generic>
        {
            fn ty() -> ::mlua_extras::typed::Type {
                ::mlua_extras::typed::Type::Function {
                    params: ::std::vec![
                        $(
                            ::mlua_extras::typed::Param {
                                name: ::std::option::Option::Some(
                                    ::std::convert::Into::into(stringify!($param)),
                                ),
                                doc: {
                                    let docs: &[&str] = &[$($doc),*];
                                    if docs.is_empty() {
                                        ::std::option::Option::None
                                    } else {
                                        ::std::option::Option::Some(
                                            ::std::convert::Into::into(
                                                docs.iter()
                                                    .map(|line| line.trim())
                                                    .collect::<::std::vec::Vec<_>>()
                                                    .join("\n"),
                                            ),
                                        )
                                    }
                                },
                                ty: <$param_ty as ::mlua_extras::typed::Typed>::ty(),
                            }
                        ),*
                    ],
                    returns: ::std::vec![],
                }
            }
        }
    };
}
