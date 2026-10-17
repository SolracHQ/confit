//! Stub
//! Emit LuaLS stub text for bound namespaces.

use crate::face::{
    FieldDescriptor, FunctionDescriptor, FunctionParameter, FunctionReturn, VariantDescriptor,
};
use crate::registry::Registry;
use crate::userdata::MethodDescriptor;

/// One rendered stub file.
///
/// One namespace with its complete
/// stub text for freshness comparison.
pub struct StubDocument {
    /// The dotted namespace path.
    pub namespace: String,
    /// The complete stub text.
    pub text: String,
}

/// Stub emitter with a file header.
///
/// Registry blocks under one header
/// and one document per namespace.
pub struct StubEmitter {
    /// The file header lines.
    header: String,
}

impl StubEmitter {
    /// Build one emitter with its file header.
    pub fn new(header: impl Into<String>) -> Self {
        Self {
            header: header.into(),
        }
    }

    /// Emit one stub document for one namespace.
    pub fn emit(&self, namespace: &str, registry: &Registry) -> StubDocument {
        let mut text = self.header.clone();
        text.push('\n');
        text.push_str(&registry.stub_text());
        StubDocument {
            namespace: namespace.to_owned(),
            text,
        }
    }
}

/// Emit one class block for one table shape.
///
/// Docs render trimmed, so raw `///` lines and trimmed
/// strings share one output shape.
pub fn emit_class(name: &str, documentation: &str, fields: &[FieldDescriptor]) -> String {
    let mut block = String::from("--- ");
    block.push_str(documentation.trim());
    block.push_str("\n--- @class ");
    block.push_str(name);
    for field in fields {
        block.push('\n');
        block.push_str(&emit_field(field));
    }
    block
}

/// Emit one field line for one class block.
///
/// Faces carry nullability, so optional fields render the
/// `?` suffix through the face expression alone.
pub fn emit_field(field: &FieldDescriptor) -> String {
    let mut line = String::from("--- @field ");
    line.push_str(field.name);
    line.push(' ');
    line.push_str(&field.face);
    line.push(' ');
    line.push_str(field.documentation.trim());
    line
}

/// Emit one alias block for one union face.
///
/// Unions and scalars alone render aliases.
pub fn emit_alias(name: &str, documentation: &str, variants: &[VariantDescriptor]) -> String {
    let mut block = String::from("--- ");
    block.push_str(documentation.trim());
    block.push_str("\n--- @alias ");
    block.push_str(name);
    block.push(' ');
    let faces: Vec<String> = variants
        .iter()
        .map(|variant| variant.face.clone().into_owned())
        .collect();
    block.push_str(&faces.join(" | "));
    block
}

/// Emit one function block for one static.
///
/// Optional params read `name?` with the plain face, so one
/// trailing `?` falls off optional faces. Return faces carry
/// nullability through the face expression alone. Docs render
/// trimmed, so raw `///` lines and trimmed strings share one
/// output shape.
pub fn emit_function(
    descriptor: &FunctionDescriptor,
    parameters: &[FunctionParameter],
    returns: &[FunctionReturn],
) -> String {
    let mut block = String::from("--- ");
    block.push_str(descriptor.documentation.trim());
    for parameter in parameters {
        block.push_str("\n--- @param ");
        block.push_str(parameter.name);
        if parameter.optional {
            block.push('?');
        }
        block.push(' ');
        if parameter.optional {
            block.push_str(parameter.face.strip_suffix('?').unwrap_or(&parameter.face));
        } else {
            block.push_str(&parameter.face);
        }
        block.push(' ');
        block.push_str(parameter.documentation.trim());
    }
    for returned in returns {
        block.push_str("\n--- @return ");
        block.push_str(&returned.face);
        block.push(' ');
        block.push_str(returned.documentation.trim());
    }
    block.push_str("\nfunction ");
    block.push_str(descriptor.name);
    block.push('(');
    let labels: Vec<String> = parameters
        .iter()
        .map(|parameter| parameter.name.to_owned())
        .collect();
    block.push_str(&labels.join(", "));
    block.push_str(") end");
    block
}

/// Emit one method block for one userdata method.
///
/// Docs render trimmed, so raw `///` lines and trimmed
/// strings share one output shape. The receiver stays out
/// of the argument list since the colon passes it.
pub fn emit_method(class: &str, method: &MethodDescriptor) -> String {
    let mut block = String::from("---");
    let documentation = method.documentation.trim();
    if !documentation.is_empty() {
        block.push(' ');
        block.push_str(documentation);
    }
    for parameter in &method.parameters {
        block.push_str("\n--- @param ");
        block.push_str(parameter.name);
        block.push(' ');
        block.push_str(parameter.face.trim());
        block.push(' ');
        block.push_str(parameter.documentation.trim());
    }
    for returned in &method.returns {
        block.push_str("\n--- @return ");
        block.push_str(returned.face.trim());
        block.push(' ');
        block.push_str(returned.documentation.trim());
    }
    block.push_str("\nfunction ");
    block.push_str(class);
    block.push(':');
    block.push_str(method.name);
    block.push('(');
    let labels: Vec<&str> = method
        .parameters
        .iter()
        .map(|parameter| parameter.name)
        .collect();
    block.push_str(&labels.join(", "));
    block.push_str(") end");
    block
}

/// Emit one literal union block for one string enum.
pub fn emit_literal(name: &str, documentation: &str, literals: &[String]) -> String {
    let variants: Vec<VariantDescriptor> = literals
        .iter()
        .map(|literal| VariantDescriptor {
            name: "",
            documentation: "",
            face: std::borrow::Cow::Owned(format!("\"{literal}\"")),
        })
        .collect();
    emit_alias(name, documentation, &variants)
}

/// Emit one optional type expression.
pub fn emit_optional(face: &str) -> String {
    if face.ends_with('?') {
        face.to_owned()
    } else {
        format!("{face}?")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::userdata::{MethodParameter, MethodReturn};

    #[test]
    fn method_block_emits_colon_call_without_receiver() {
        let method = MethodDescriptor {
            name: "members",
            documentation: "List the archive member names, else nil with reason.",
            parameters: vec![],
            returns: vec![
                MethodReturn {
                    face: std::borrow::Cow::Borrowed("string[]?"),
                    documentation: "Member names, else nil.",
                },
                MethodReturn {
                    face: std::borrow::Cow::Borrowed("string?"),
                    documentation: "Failure reason, else nil.",
                },
            ],
        };
        assert_eq!(
            emit_method("ArchiveHandle", &method),
            "--- List the archive member names, else nil with reason.\n--- @return string[]? Member names, else nil.\n--- @return string? Failure reason, else nil.\nfunction ArchiveHandle:members() end"
        );
        let named = MethodDescriptor {
            name: "member",
            documentation: "Open one archive member as its handle, else nil with reason.",
            parameters: vec![MethodParameter {
                name: "name",
                face: std::borrow::Cow::Borrowed("string"),
                documentation: "Archive-relative member name.",
            }],
            returns: vec![MethodReturn {
                face: std::borrow::Cow::Borrowed("ResourceHandle?"),
                documentation: "Member handle, else nil.",
            }],
        };
        let block = emit_method("ArchiveHandle", &named);
        assert!(block.contains("--- @param name string Archive-relative member name."));
        assert!(block.contains("function ArchiveHandle:member(name) end"));
        assert!(!block.contains("self"));
    }
}
