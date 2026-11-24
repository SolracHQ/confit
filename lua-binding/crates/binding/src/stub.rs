//! LuaLS stubs for collected shapes.
//!
//! Stubs give editors the Lua names and types a module declares. A `lua_module`
//! block exposes a `stub` method that renders its own stub.

use std::borrow::Cow;

use crate::generator::Generator;
use crate::object::{Method, ObjectField, Operator};
use crate::shape::{Field, Parameter, Return, ShapeData, Variant};

/// One rendered stub file.
///
/// Write `text` to a `.lua` file under a LuaLS library directory.
pub struct Stub {
    /// The dotted Lua module path.
    pub module: String,
    /// The complete stub text.
    pub text: String,
}

/// The LuaLS stub writer.
///
/// The header leads every stub the emitter produces.
pub struct Emitter {
    /// The file header lines.
    header: String,
}

impl Emitter {
    /// Creates an emitter whose header leads every stub.
    pub fn new(header: impl Into<String>) -> Self {
        Self {
            header: header.into(),
        }
    }

    /// Renders every collected entry with the header.
    pub fn emit(&self, module: &str, generator: &Generator) -> Stub {
        let mut text = self.header.clone();
        text.push('\n');
        text.push_str(&generator.generate());
        Stub {
            module: module.to_owned(),
            text,
        }
    }
}

/// Renders a `--- @field` line.
pub fn emit_field(field: &Field) -> String {
    let mut line = String::from("--- @field ");
    line.push_str(field.name);
    line.push(' ');
    line.push_str(&field.shape);
    line.push(' ');
    line.push_str(field.description.trim());
    line
}

/// Renders an object field as a `--- @field` line.
///
/// The shape and description carry trimmed spacing.
pub fn emit_object_field(field: &ObjectField) -> String {
    let mut line = String::from("--- @field ");
    line.push_str(field.name);
    line.push(' ');
    line.push_str(field.shape.trim());
    let description = field.description.trim();
    if !description.is_empty() {
        line.push(' ');
        line.push_str(description);
    }
    line
}

/// Renders an object operator as a `--- @operator` line.
///
/// Operand shapes join with commas inside parentheses. The first return shape
/// follows the colon.
pub fn emit_operator(operator: &Operator) -> String {
    let mut line = String::from("--- @operator ");
    line.push_str(operator.lua);
    if !operator.parameters.is_empty() {
        line.push('(');
        let shapes: Vec<&str> = operator
            .parameters
            .iter()
            .map(|parameter| parameter.shape.trim())
            .collect();
        line.push_str(&shapes.join(", "));
        line.push(')');
    }
    if let Some(returned) = operator.returns.first() {
        line.push_str(": ");
        line.push_str(returned.shape.trim());
    }
    line
}

/// Renders a table field that holds a function as a dotted function stub.
pub fn emit_call_method(
    table: &str,
    field: &Field,
    parameters: &[Parameter],
    returns: &[Return],
) -> String {
    let mut block = String::from("--- ");
    block.push_str(field.description.trim());
    for parameter in parameters {
        block.push_str("\n--- @param ");
        block.push_str(parameter.name);
        block.push(' ');
        block.push_str(&parameter.shape);
        block.push(' ');
        block.push_str(parameter.description.trim());
    }
    for returned in returns {
        block.push_str("\n--- @return ");
        block.push_str(returned.shape.trim());
        let description = returned.description.trim();
        if !description.is_empty() {
            block.push(' ');
            block.push_str(description);
        }
    }
    block.push_str("\nfunction ");
    block.push_str(table);
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

/// Renders an object method as a colon function stub.
pub fn emit_method(class: &str, method: &Method) -> String {
    let mut block = String::from("---");
    let description = method.description.trim();
    if !description.is_empty() {
        block.push(' ');
        block.push_str(description);
    }
    for parameter in &method.parameters {
        block.push_str("\n--- @param ");
        block.push_str(parameter.name);
        block.push(' ');
        block.push_str(parameter.shape.trim());
        block.push(' ');
        block.push_str(parameter.description.trim());
    }
    for returned in &method.returns {
        block.push_str("\n--- @return ");
        block.push_str(returned.shape.trim());
        block.push(' ');
        block.push_str(returned.description.trim());
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

/// Renders a shape as a LuaLS annotation block.
///
/// Returns `None` for primitives.
pub fn emit(name: &str, data: &ShapeData) -> Option<String> {
    match data {
        ShapeData::Primitive { .. } => None,
        ShapeData::Table {
            description,
            fields,
        } => {
            let mut block = String::from("--- ");
            block.push_str(description.trim());
            block.push_str("\n--- @class ");
            block.push_str(name);
            for field in fields {
                block.push('\n');
                block.push_str(&emit_field(field));
            }
            Some(block)
        }
        ShapeData::Alias {
            description,
            variants: arms,
        } => {
            let mut block = String::from("--- ");
            block.push_str(description.trim());
            block.push_str("\n--- @alias ");
            block.push_str(name);
            block.push(' ');
            let shapes: Vec<String> = arms
                .iter()
                .map(|arm| arm.shape.clone().into_owned())
                .collect();
            block.push_str(&shapes.join(" | "));
            Some(block)
        }
        ShapeData::Function {
            description,
            parameters,
            returns,
        } => Some(emit_function(name, description, parameters, returns)),
        ShapeData::Object { description } => {
            let mut block = String::from("--- ");
            block.push_str(description.trim());
            block.push_str("\n--- @class ");
            block.push_str(name);
            Some(block)
        }
    }
}

/// Renders the module return line.
///
/// One function returns itself. Several return a table of named functions.
pub fn return_line(functions: &[&str]) -> String {
    match functions {
        [single] => format!("return {single}"),
        _ => {
            let entries: Vec<String> = functions
                .iter()
                .map(|function| format!("{function} = {function}"))
                .collect();
            format!("return {{{}}}", entries.join(", "))
        }
    }
}

/// Renders a module stub with its return line.
pub fn generate_module(module: &str, functions: &[&str], generator: &Generator) -> Stub {
    let emitter = Emitter::new(format!("--- @meta {module}"));
    let mut stub = emitter.emit(module, generator);
    stub.text.push('\n');
    stub.text.push_str(&return_line(functions));
    stub.text.push('\n');
    stub
}

/// Renders a globals stub with the `--- @meta _` header.
pub fn generate_globals(module: &str, generator: &Generator) -> Stub {
    let emitter = Emitter::new("--- @meta _");
    emitter.emit(module, generator)
}

/// Adds every inlined field shape to the generator.
fn collect_inlined(generator: &mut Generator, fields: &[Field]) {
    for field in fields {
        (field.collect)(generator);
    }
}

/// Adds a crossed table that combines owner fields with a variant payload.
///
/// A table payload merges its fields. An alias payload adds one crossed table
/// per backed variant. Other payloads keep the owner fields. Returns
/// `false` for a non-table owner shape or an existing name.
pub fn add_crossed(
    generator: &mut Generator,
    name: &str,
    shape: &ShapeData,
    inner: &ShapeData,
) -> bool {
    let ShapeData::Table {
        description,
        fields: owner_fields,
    } = shape
    else {
        return false;
    };
    match inner {
        ShapeData::Table {
            fields: inner_fields,
            ..
        } => {
            if generator.contains(name) {
                return false;
            }
            let mut fields = owner_fields.clone();
            fields.extend(inner_fields.iter().cloned());
            let inserted = generator.add_table(name.to_owned(), description, fields.clone());
            if inserted {
                collect_inlined(generator, &fields);
            }
            inserted
        }
        ShapeData::Alias { variants: arms, .. } => {
            if generator.contains(name) {
                return false;
            }
            let mut members = Vec::new();
            let mut backed = 0;
            for arm in arms {
                if arm.fields.is_empty() {
                    continue;
                }
                let mut crossed = owner_fields.clone();
                crossed.extend(arm.fields.iter().cloned());
                generator.add_table(
                    crossed_name(name, arm.name),
                    arm.description,
                    crossed.clone(),
                );
                collect_inlined(generator, &crossed);
                members.push(Variant {
                    name: arm.name,
                    description: arm.description,
                    shape: Cow::Owned(crossed_name(name, arm.name)),
                    fields: Vec::new(),
                    collect: arm.collect,
                });
                backed += 1;
            }
            if backed == 0 {
                let inserted =
                    generator.add_table(name.to_owned(), description, owner_fields.clone());
                if inserted {
                    collect_inlined(generator, owner_fields);
                }
                return inserted;
            }
            generator.add_alias(name.to_owned(), description, members)
        }
        _ => {
            if generator.contains(name) {
                return false;
            }
            let inserted = generator.add_table(name.to_owned(), description, owner_fields.clone());
            if inserted {
                collect_inlined(generator, owner_fields);
            }
            inserted
        }
    }
}

/// Renders a function block.
fn emit_function(
    name: &str,
    description: &str,
    parameters: &[Parameter],
    returns: &[Return],
) -> String {
    let mut block = String::from("--- ");
    block.push_str(description.trim());
    for parameter in parameters {
        block.push_str("\n--- @param ");
        block.push_str(parameter.name);
        if parameter.optional {
            block.push('?');
        }
        block.push(' ');
        if parameter.optional {
            block.push_str(
                parameter
                    .shape
                    .strip_suffix('?')
                    .unwrap_or(&parameter.shape),
            );
        } else {
            block.push_str(&parameter.shape);
        }
        block.push(' ');
        block.push_str(parameter.description.trim());
    }
    for returned in returns {
        block.push_str("\n--- @return ");
        block.push_str(&returned.shape);
        block.push(' ');
        block.push_str(returned.description.trim());
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

/// Renders a type as optional.
///
/// Function types are parenthesized before the `?`.
pub fn emit_optional(shape: &str) -> String {
    if shape.ends_with('?') {
        shape.to_owned()
    } else if shape.starts_with("fun(") {
        format!("({shape})?")
    } else {
        format!("{shape}?")
    }
}

/// Builds a crossed table name from the owner name and the variant name.
fn crossed_name(table: &str, variant: &str) -> String {
    format!("{table}_{variant}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{MethodParameter, MethodReturn};
    use crate::shape::no_collect;

    #[test]
    fn alias_variants_render_in_variant_order() {
        let data = ShapeData::Alias {
            description: "A table payload.",
            variants: vec![
                Variant {
                    name: "Text",
                    description: "Plain text file.",
                    shape: Cow::Borrowed("TextTable"),
                    fields: Vec::new(),
                    collect: no_collect,
                },
                Variant {
                    name: "Count",
                    description: "Counted entries.",
                    shape: Cow::Borrowed("CountTable"),
                    fields: Vec::new(),
                    collect: no_collect,
                },
            ],
        };
        assert_eq!(
            emit("TableBody", &data).unwrap_or_default(),
            "--- A table payload.\n--- @alias TableBody TextTable | CountTable"
        );
    }

    #[test]
    fn crossed_data_adds_flat_and_alias_tables() {
        let table = ShapeData::Table {
            description: "A table.",
            fields: vec![Field {
                name: "path",
                description: "The destination name.",
                shape: Cow::Borrowed("Name"),
                optional: false,
                collect: no_collect,
            }],
        };
        let inner = ShapeData::Table {
            description: "A body.",
            fields: vec![Field {
                name: "text",
                description: "The exact file text.",
                shape: Cow::Borrowed("string"),
                optional: false,
                collect: no_collect,
            }],
        };
        let mut generator = Generator::new();
        assert!(add_crossed(&mut generator, "Table", &table, &inner));
        assert!(generator.contains("Table"));
        let union_inner = ShapeData::Alias {
            description: "A body.",
            variants: vec![
                Variant {
                    name: "Text",
                    description: "Plain text file.",
                    shape: Cow::Borrowed("TextTable"),
                    fields: vec![Field {
                        name: "text",
                        description: "The exact file text.",
                        shape: Cow::Borrowed("string"),
                        optional: false,
                        collect: no_collect,
                    }],
                    collect: no_collect,
                },
                Variant {
                    name: "Nothing",
                    description: "Empty marker.",
                    shape: Cow::Borrowed("nil"),
                    fields: Vec::new(),
                    collect: no_collect,
                },
            ],
        };
        let mut crossed = Generator::new();
        assert!(add_crossed(&mut crossed, "Table", &table, &union_inner));
        assert!(crossed.contains("Table_Text"));
        assert!(!crossed.contains("Table_Nothing"));
        assert!(crossed.contains("Table"));
        assert!(!add_crossed(&mut crossed, "Table", &table, &union_inner));
    }

    #[test]
    fn method_block_emits_colon_call_without_receiver() {
        let method = Method {
            name: "members",
            description: "List the archive member names, else nil with reason.",
            parameters: vec![],
            returns: vec![
                MethodReturn {
                    shape: std::borrow::Cow::Borrowed("string[]?"),
                    description: "Member names, else nil.",
                    collect: no_collect,
                },
                MethodReturn {
                    shape: std::borrow::Cow::Borrowed("string?"),
                    description: "Failure reason, else nil.",
                    collect: no_collect,
                },
            ],
        };
        assert_eq!(
            emit_method("ArchiveHandle", &method),
            "--- List the archive member names, else nil with reason.\n--- @return string[]? Member names, else nil.\n--- @return string? Failure reason, else nil.\nfunction ArchiveHandle:members() end"
        );
        let named = Method {
            name: "member",
            description: "Open one archive member as its handle, else nil with reason.",
            parameters: vec![MethodParameter {
                name: "name",
                shape: std::borrow::Cow::Borrowed("string"),
                description: "Archive-relative member name.",
                collect: no_collect,
            }],
            returns: vec![MethodReturn {
                shape: std::borrow::Cow::Borrowed("ResourceHandle?"),
                description: "Member handle, else nil.",
                collect: no_collect,
            }],
        };
        let block = emit_method("ArchiveHandle", &named);
        assert!(block.contains("--- @param name string Archive-relative member name."));
        assert!(block.contains("function ArchiveHandle:member(name) end"));
        assert!(!block.contains("self"));
    }

    #[test]
    fn call_method_emits_dot_call_with_field_descriptions() {
        let field = Field {
            name: "run",
            description: "Runs the process.",
            shape: Cow::Borrowed("fun(process: Process)"),
            optional: false,
            collect: no_collect,
        };
        let params = vec![Parameter {
            name: "process",
            description: "Live process.",
            shape: Cow::Borrowed("Process"),
            optional: false,
            collect: no_collect,
        }];
        assert_eq!(
            emit_call_method("Table", &field, &params, &[]),
            "--- Runs the process.\n--- @param process Process Live process.\nfunction Table.run(process) end"
        );
    }

    #[test]
    fn object_field_and_operator_render_stub_lines() {
        let field = ObjectField {
            name: "x",
            description: " The x coordinate. ",
            shape: Cow::Borrowed("number"),
            readable: true,
            writable: true,
            collect: no_collect,
        };
        assert_eq!(
            emit_object_field(&field),
            "--- @field x number The x coordinate."
        );
        let operator = Operator {
            lua: "add",
            parameters: vec![MethodParameter {
                name: "other",
                shape: Cow::Borrowed("Vec2"),
                description: "The second vector.",
                collect: no_collect,
            }],
            returns: vec![MethodReturn {
                shape: Cow::Borrowed("Vec2"),
                description: "The sum vector.",
                collect: no_collect,
            }],
        };
        assert_eq!(emit_operator(&operator), "--- @operator add(Vec2): Vec2");
    }

    #[test]
    fn call_method_appends_return_shapes() {
        let field = Field {
            name: "keep",
            description: "Keeps matching names.",
            shape: Cow::Borrowed("fun(name: Name): boolean"),
            optional: false,
            collect: no_collect,
        };
        let params = vec![Parameter {
            name: "name",
            description: "Target name of the pending process.",
            shape: Cow::Borrowed("Name"),
            optional: false,
            collect: no_collect,
        }];
        let returns = vec![Return {
            shape: Cow::Borrowed("boolean"),
            description: "True keeps the pending process.",
            collect: no_collect,
        }];
        assert_eq!(
            emit_call_method("Table", &field, &params, &returns),
            "--- Keeps matching names.\n--- @param name Name Target name of the pending process.\n--- @return boolean True keeps the pending process.\nfunction Table.keep(name) end"
        );
    }
}
