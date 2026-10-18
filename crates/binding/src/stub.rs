//! Stub
//! Emit LuaLS stub text for bound namespaces.

use std::borrow::Cow;

use crate::error::Result;
use crate::face::{FaceData, FieldDescriptor, FunctionParameter, FunctionReturn, UnionArm};
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

/// Emit one callback method block for one table field.
pub fn emit_callback_method(
    holder: &str,
    field: &FieldDescriptor,
    parameters: &[FunctionParameter],
    returns: &[FunctionReturn],
) -> String {
    let mut block = String::from("--- ");
    block.push_str(field.documentation.trim());
    for parameter in parameters {
        block.push_str("\n--- @param ");
        block.push_str(parameter.name);
        block.push(' ');
        block.push_str(&parameter.face);
        block.push(' ');
        block.push_str(parameter.documentation.trim());
    }
    for returned in returns {
        block.push_str("\n--- @return ");
        block.push_str(returned.face.trim());
        let documentation = returned.documentation.trim();
        if !documentation.is_empty() {
            block.push(' ');
            block.push_str(documentation);
        }
    }
    block.push_str("\nfunction ");
    block.push_str(holder);
    block.push('.');
    block.push_str(field.name);
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

/// Emit one stub block.
pub fn emit(name: &str, data: &FaceData) -> Option<String> {
    match data {
        FaceData::Scalar { .. } => None,
        FaceData::Class {
            documentation,
            fields,
        } => {
            let mut block = String::from("--- ");
            block.push_str(documentation.trim());
            block.push_str("\n--- @class ");
            block.push_str(name);
            for field in fields {
                block.push('\n');
                block.push_str(&emit_field(field));
            }
            Some(block)
        }
        FaceData::Union {
            documentation,
            arms,
        } => {
            let mut block = String::from("--- ");
            block.push_str(documentation.trim());
            block.push_str("\n--- @alias ");
            block.push_str(name);
            block.push(' ');
            let faces: Vec<String> = arms
                .iter()
                .filter(|arm| !arm.skipped)
                .map(|arm| arm.face.clone().into_owned())
                .collect();
            block.push_str(&faces.join(" | "));
            Some(block)
        }
        FaceData::Function {
            documentation,
            parameters,
            returns,
        } => Some(emit_function(name, documentation, parameters, returns)),
        FaceData::Userdata { documentation } => {
            let mut block = String::from("--- ");
            block.push_str(documentation.trim());
            block.push_str("\n--- @class ");
            block.push_str(name);
            Some(block)
        }
    }
}

/// Collect every inlined field face into its registry.
///
/// # Errors
///
/// - [`crate::error::BindingError`] for repeated registration.
fn collect_inlined(registry: &mut Registry, fields: &[FieldDescriptor]) -> Result<()> {
    for field in fields {
        (field.collect)(registry)?;
    }
    Ok(())
}

/// Collect one crossed holder with its inlined faces.
///
/// - Repeated holders keep the first entry.
/// - Crossed classes hold holder fields beside arm fields.
/// - Every inlined field face collects through its descriptor.
///
/// # Errors
///
/// - [`crate::error::BindingError`] for repeated registration.
pub fn register_crossed(
    registry: &mut Registry,
    name: &str,
    holder: &FaceData,
    inner: &FaceData,
) -> Result<bool> {
    let FaceData::Class {
        documentation,
        fields: holder_fields,
    } = holder
    else {
        return Ok(false);
    };
    match inner {
        FaceData::Class {
            fields: inner_fields,
            ..
        } => {
            if registry.contains(name) {
                return Ok(false);
            }
            let mut fields = holder_fields.clone();
            fields.extend(inner_fields.iter().cloned());
            let inserted = registry.register_class(name.to_owned(), documentation, fields.clone());
            if inserted {
                collect_inlined(registry, &fields)?;
            }
            Ok(inserted)
        }
        FaceData::Union { arms, .. } => {
            if registry.contains(name) {
                return Ok(false);
            }
            let mut members = Vec::new();
            let mut backed = 0;
            for arm in arms {
                if arm.fields.is_empty() {
                    continue;
                }
                let mut crossed = holder_fields.clone();
                crossed.extend(arm.fields.iter().cloned());
                registry.register_class(
                    crossed_name(name, arm.name),
                    arm.documentation,
                    crossed.clone(),
                );
                collect_inlined(registry, &crossed)?;
                members.push(UnionArm {
                    name: arm.name,
                    documentation: arm.documentation,
                    face: Cow::Owned(crossed_name(name, arm.name)),
                    fields: Vec::new(),
                    collect: arm.collect,
                    skipped: arm.skipped,
                });
                backed += 1;
            }
            if backed == 0 {
                let inserted =
                    registry.register_class(name.to_owned(), documentation, holder_fields.clone());
                if inserted {
                    collect_inlined(registry, holder_fields)?;
                }
                return Ok(inserted);
            }
            Ok(registry.register_alias(name.to_owned(), documentation, members))
        }
        _ => {
            if registry.contains(name) {
                return Ok(false);
            }
            let inserted =
                registry.register_class(name.to_owned(), documentation, holder_fields.clone());
            if inserted {
                collect_inlined(registry, holder_fields)?;
            }
            Ok(inserted)
        }
    }
}

/// Emit one function block from its parts.
fn emit_function(
    name: &str,
    documentation: &str,
    parameters: &[FunctionParameter],
    returns: &[FunctionReturn],
) -> String {
    let mut block = String::from("--- ");
    block.push_str(documentation.trim());
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
    block.push_str(name);
    block.push('(');
    let labels: Vec<String> = parameters
        .iter()
        .map(|parameter| parameter.name.to_owned())
        .collect();
    block.push_str(&labels.join(", "));
    block.push_str(") end");
    block
}

/// Emit one optional type expression.
///
/// - Function faces wrap in parens so the marker covers
///   the whole signature instead of the return alone.
pub fn emit_optional(face: &str) -> String {
    if face.ends_with('?') {
        face.to_owned()
    } else if face.starts_with("fun(") {
        format!("({face})?")
    } else {
        format!("{face}?")
    }
}

/// Build one crossed class name from its holder with its variant.
fn crossed_name(holder: &str, variant: &str) -> String {
    format!("{holder}_{variant}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::face::no_collect;
    use crate::userdata::{MethodParameter, MethodReturn};

    #[test]
    fn union_arms_render_alias_in_arm_order() {
        let data = FaceData::Union {
            documentation: "One document payload.",
            arms: vec![
                UnionArm {
                    name: "Text",
                    documentation: "Plain text file.",
                    face: Cow::Borrowed("TextDocument"),
                    fields: Vec::new(),
                    collect: no_collect,
                    skipped: false,
                },
                UnionArm {
                    name: "Count",
                    documentation: "Counted entries.",
                    face: Cow::Borrowed("CountDocument"),
                    fields: Vec::new(),
                    collect: no_collect,
                    skipped: false,
                },
            ],
        };
        assert_eq!(
            emit("DocumentBody", &data).unwrap_or_default(),
            "--- One document payload.\n--- @alias DocumentBody TextDocument | CountDocument"
        );
    }

    #[test]
    fn crossed_data_registers_flat_and_union_holders() {
        let holder = FaceData::Class {
            documentation: "One holder.",
            fields: vec![FieldDescriptor {
                name: "path",
                documentation: "The destination route.",
                face: Cow::Borrowed("Route"),
                optional: false,
                collect: no_collect,
            }],
        };
        let inner = FaceData::Class {
            documentation: "One body.",
            fields: vec![FieldDescriptor {
                name: "text",
                documentation: "The exact file text.",
                face: Cow::Borrowed("string"),
                optional: false,
                collect: no_collect,
            }],
        };
        let mut registry = Registry::new();
        assert!(register_crossed(&mut registry, "Holder", &holder, &inner).unwrap());
        assert!(registry.contains("Holder"));
        let union_inner = FaceData::Union {
            documentation: "One body.",
            arms: vec![
                UnionArm {
                    name: "Text",
                    documentation: "Plain text file.",
                    face: Cow::Borrowed("TextDocument"),
                    fields: vec![FieldDescriptor {
                        name: "text",
                        documentation: "The exact file text.",
                        face: Cow::Borrowed("string"),
                        optional: false,
                        collect: no_collect,
                    }],
                    collect: no_collect,
                    skipped: false,
                },
                UnionArm {
                    name: "Nothing",
                    documentation: "Empty marker.",
                    face: Cow::Borrowed("nil"),
                    fields: Vec::new(),
                    collect: no_collect,
                    skipped: false,
                },
            ],
        };
        let mut crossed = Registry::new();
        assert!(register_crossed(&mut crossed, "Document", &holder, &union_inner).unwrap());
        assert!(crossed.contains("Document_Text"));
        assert!(!crossed.contains("Document_Nothing"));
        assert!(crossed.contains("Document"));
        assert!(!register_crossed(&mut crossed, "Document", &holder, &union_inner).unwrap());
    }

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
                    collect: no_collect,
                },
                MethodReturn {
                    face: std::borrow::Cow::Borrowed("string?"),
                    documentation: "Failure reason, else nil.",
                    collect: no_collect,
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
                collect: no_collect,
            }],
            returns: vec![MethodReturn {
                face: std::borrow::Cow::Borrowed("ResourceHandle?"),
                documentation: "Member handle, else nil.",
                collect: no_collect,
            }],
        };
        let block = emit_method("ArchiveHandle", &named);
        assert!(block.contains("--- @param name string Archive-relative member name."));
        assert!(block.contains("function ArchiveHandle:member(name) end"));
        assert!(!block.contains("self"));
    }

    #[test]
    fn callback_method_emits_dot_call_with_field_docs() {
        let field = FieldDescriptor {
            name: "apply",
            documentation: "Run the patch.",
            face: Cow::Borrowed("fun(document: Doc)"),
            optional: false,
            collect: no_collect,
        };
        let params = vec![FunctionParameter {
            name: "document",
            documentation: "Live doc.",
            face: Cow::Borrowed("Doc"),
            optional: false,
            collect: no_collect,
        }];
        assert_eq!(
            emit_callback_method("Holder", &field, &params, &[]),
            "--- Run the patch.\n--- @param document Doc Live doc.\nfunction Holder.apply(document) end"
        );
    }

    #[test]
    fn callback_method_appends_return_faces() {
        let field = FieldDescriptor {
            name: "filter",
            documentation: "Keep matching routes.",
            face: Cow::Borrowed("fun(route: Route): boolean"),
            optional: false,
            collect: no_collect,
        };
        let params = vec![FunctionParameter {
            name: "route",
            documentation: "Target route of the candidate document.",
            face: Cow::Borrowed("Route"),
            optional: false,
            collect: no_collect,
        }];
        let returns = vec![FunctionReturn {
            face: Cow::Borrowed("boolean"),
            documentation: "True keeps the candidate document.",
            collect: no_collect,
        }];
        assert_eq!(
            emit_callback_method("Holder", &field, &params, &returns),
            "--- Keep matching routes.\n--- @param route Route Target route of the candidate document.\n--- @return boolean True keeps the candidate document.\nfunction Holder.filter(route) end"
        );
    }
}
