//! Stub
//! Emits LuaLS stub text for bound namespaces.

use crate::face::{FieldDescriptor, VariantDescriptor};
use crate::registry::Registry;
use crate::userdata::MethodDescriptor;
use crate::verb::{ParameterDescriptor, ReturnDescriptor, VerbDescriptor};

/// One rendered stub file.
///
/// The document pairs one namespace with its complete
/// stub text for freshness comparison.
pub struct StubDocument {
    /// The dotted namespace path.
    pub namespace: String,
    /// The complete stub text.
    pub text: String,
}

/// Stub emitter with a file header.
///
/// The emitter joins registry blocks under one header
/// and renders one document per namespace.
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
/// Unions and scalars alone render aliases. Tables render
/// classes. Variants with docs add one note line each.
pub fn emit_alias(name: &str, documentation: &str, variants: &[VariantDescriptor]) -> String {
    let mut block = String::from("--- ");
    block.push_str(documentation.trim());
    for variant in variants {
        if !variant.documentation.trim().is_empty() {
            block.push('\n');
            block.push_str("--- ");
            block.push_str(variant.name);
            block.push_str(": ");
            block.push_str(variant.documentation.trim());
        }
    }
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

/// Emit one function block for one verb.
///
/// Optional params read `name?` with the plain face, so one
/// trailing `?` falls off optional faces. Return faces carry
/// nullability through the face expression alone. Docs render
/// trimmed, so raw `///` lines and trimmed strings share one
/// output shape.
pub fn emit_function(
    descriptor: &VerbDescriptor,
    parameters: &[ParameterDescriptor],
    returns: &[ReturnDescriptor],
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

/// Emit one method line for one userdata class block.
///
/// Docs render trimmed, so raw `///` lines and trimmed
/// strings share one output shape.
pub fn emit_method_field(method: &MethodDescriptor) -> String {
    let mut line = String::from("--- @field ");
    line.push_str(method.name);
    line.push(' ');
    line.push_str(&method.signature);
    line.push(' ');
    line.push_str(method.documentation.trim());
    line
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
