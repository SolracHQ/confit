//! Collects Lua stub entries and renders them as LuaLS annotations.
//!
//! A name enters once, so shared and recursive shapes are safe to add. The
//! `stub` method from a `lua_module` block uses this generator.

use std::collections::{HashMap, HashSet};

use crate::object::{Method, ObjectField, Operator};
use crate::shape::{Field, Parameter, Return, ShapeData, Variant};
use crate::stub::{emit, emit_call_method, emit_method, emit_object_field, emit_operator};

/// One entry in a generated stub.
///
/// Each variant carries the data for a Lua declaration.
#[derive(Clone, Debug, PartialEq)]
pub enum Doc {
    /// A Lua table declaration.
    Table {
        /// The Lua name.
        name: String,
        /// The stub description.
        description: &'static str,
        /// The table fields.
        fields: Vec<Field>,
    },
    /// A Lua alias declaration.
    Alias {
        /// The Lua name.
        name: String,
        /// The stub description.
        description: &'static str,
        /// The alias variants.
        variants: Vec<Variant>,
    },
    /// A Lua function declaration.
    Function {
        /// The Lua name.
        name: String,
        /// The stub description.
        description: &'static str,
        /// The function parameters.
        parameters: Vec<Parameter>,
        /// The function return values.
        returns: Vec<Return>,
    },
    /// A Lua object declaration.
    Object {
        /// The Lua name.
        name: &'static str,
        /// The stub description.
        description: &'static str,
        /// The object fields.
        fields: Vec<ObjectField>,
        /// The object operators.
        operators: Vec<Operator>,
        /// The object methods.
        methods: Vec<Method>,
    },
}

/// The parameters and returns behind a callable shape text.
#[derive(Clone, Debug)]
struct CallableSignature {
    /// The callable parameters in order.
    parameters: Vec<Parameter>,
    /// The callable return values in order.
    returns: Vec<Return>,
}

/// Collects Lua stub entries and renders them as LuaLS annotations.
///
/// A name enters once, so shared and recursive shapes are safe to add.
#[derive(Debug, Default)]
pub struct Generator {
    /// The Lua names already present.
    seen: HashSet<String>,
    /// The entries in addition order.
    entries: Vec<Doc>,
    /// The callable signatures keyed by Lua shape text.
    signatures: HashMap<String, CallableSignature>,
}

impl Generator {
    /// Creates an empty generator.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a table under its Lua name.
    ///
    /// Returns `false` when the name is already present.
    pub(crate) fn add_table(
        &mut self,
        name: String,
        description: &'static str,
        fields: Vec<Field>,
    ) -> bool {
        self.insert(
            name.clone(),
            Doc::Table {
                name,
                description,
                fields,
            },
        )
    }

    /// Adds an alias under its Lua name.
    ///
    /// Returns `false` when the name is already present.
    pub(crate) fn add_alias(
        &mut self,
        name: String,
        description: &'static str,
        arms: Vec<Variant>,
    ) -> bool {
        self.insert(
            name.clone(),
            Doc::Alias {
                name,
                description,
                variants: arms,
            },
        )
    }

    /// Adds an object under its Lua name.
    ///
    /// Returns `false` when the name is already present.
    pub fn add_object(
        &mut self,
        class: &'static str,
        description: &'static str,
        fields: Vec<ObjectField>,
        methods: Vec<Method>,
        operators: Vec<Operator>,
    ) -> bool {
        self.insert(
            class.to_owned(),
            Doc::Object {
                name: class,
                description,
                fields,
                operators,
                methods,
            },
        )
    }

    /// Adds a table, alias, or function shape.
    ///
    /// Returns `false` for primitives, objects, and names already present.
    pub fn add(&mut self, name: String, data: &ShapeData) -> bool {
        match data {
            ShapeData::Primitive { .. } | ShapeData::Object { .. } => false,
            ShapeData::Table {
                description,
                fields,
            } => self.add_table(name, description, fields.clone()),
            ShapeData::Alias {
                description,
                variants: arms,
            } => self.add_alias(name, description, arms.clone()),
            ShapeData::Function {
                description,
                parameters,
                returns,
            } => self.insert(
                name.clone(),
                Doc::Function {
                    name,
                    description,
                    parameters: parameters.clone(),
                    returns: returns.clone(),
                },
            ),
        }
    }

    /// Adds a shape and every shape it references.
    ///
    /// Repeated names are ignored.
    pub fn collect(&mut self, name: String, data: &ShapeData) {
        if self.add(name, data) {
            data.collect_referenced(self);
        }
    }

    /// Records a callable signature under its Lua shape text.
    ///
    /// Returns `false` for a non-function shape or existing text.
    pub fn add_signature(&mut self, shape: String, data: &ShapeData) -> bool {
        match data {
            ShapeData::Function {
                parameters,
                returns,
                ..
            } => {
                if self.signatures.contains_key(&shape) {
                    return false;
                }
                self.signatures.insert(
                    shape,
                    CallableSignature {
                        parameters: parameters.clone(),
                        returns: returns.clone(),
                    },
                );
                true
            }
            _ => false,
        }
    }

    /// Adds a named function.
    ///
    /// Returns `false` for a non-function shape or an existing name.
    pub fn add_function(&mut self, name: &'static str, data: &ShapeData) -> bool {
        match data {
            ShapeData::Function {
                description,
                parameters,
                returns,
            } => self.insert(
                name.to_owned(),
                Doc::Function {
                    name: name.to_owned(),
                    description,
                    parameters: parameters.clone(),
                    returns: returns.clone(),
                },
            ),
            _ => false,
        }
    }

    /// Renders every collected entry in addition order.
    ///
    /// Returns an empty string when the generator holds no entries.
    pub fn generate(&self) -> String {
        let mut blocks = Vec::new();
        for entry in &self.entries {
            match entry {
                Doc::Table {
                    name,
                    description,
                    fields,
                } => {
                    let data = ShapeData::Table {
                        description,
                        fields: fields.clone(),
                    };
                    blocks.extend(self.table_blocks(name, &data));
                }
                Doc::Alias {
                    name,
                    description,
                    variants: arms,
                } => {
                    let data = ShapeData::Alias {
                        description,
                        variants: arms.clone(),
                    };
                    blocks.push(emit(name, &data).unwrap_or_default());
                }
                Doc::Function {
                    name,
                    description,
                    parameters,
                    returns,
                } => {
                    let data = ShapeData::Function {
                        description,
                        parameters: parameters.clone(),
                        returns: returns.clone(),
                    };
                    blocks.push(emit(name, &data).unwrap_or_default());
                }
                Doc::Object {
                    name: class,
                    description,
                    fields,
                    operators,
                    methods,
                } => {
                    let data = ShapeData::Object { description };
                    let mut out = emit(class, &data).unwrap_or_default();
                    for field in fields {
                        out.push('\n');
                        out.push_str(&emit_object_field(field));
                    }
                    for operator in operators {
                        out.push('\n');
                        out.push_str(&emit_operator(operator));
                    }
                    out.push('\n');
                    out.push_str(class);
                    out.push_str(" = {}");
                    for method in methods {
                        out.push_str("\n\n");
                        out.push_str(&emit_method(class, method));
                    }
                    blocks.push(out);
                }
            }
        }
        let mut out = blocks.join("\n\n");
        if !out.is_empty() {
            out.push('\n');
        }
        out
    }

    /// Returns whether the generator holds a name.
    pub fn contains(&self, name: &str) -> bool {
        self.seen.contains(name)
    }

    /// Returns the signature behind a Lua shape text.
    ///
    /// Optional and parenthesized wrappers are removed before the lookup.
    fn callable_signature(&self, shape: &str) -> Option<&CallableSignature> {
        let plain = shape.strip_suffix('?').unwrap_or(shape);
        let plain = plain
            .strip_prefix('(')
            .and_then(|inner| inner.strip_suffix(')'))
            .unwrap_or(plain);
        self.signatures.get(plain)
    }

    /// Renders the table block and any callable field blocks.
    fn table_blocks(&self, name: &str, data: &ShapeData) -> Vec<String> {
        let ShapeData::Table { fields, .. } = data else {
            return Vec::new();
        };
        let mut methods = Vec::new();
        for field in fields {
            if let Some(signature) = self.callable_signature(field.shape.as_ref()) {
                methods.push(emit_call_method(
                    name,
                    field,
                    &signature.parameters,
                    &signature.returns,
                ));
            }
        }
        let mut head = emit(name, data).unwrap_or_default();
        if !methods.is_empty() {
            head.push('\n');
            head.push_str(name);
            head.push_str(" = {}");
        }
        let mut blocks = vec![head];
        blocks.extend(methods);
        blocks
    }

    /// Adds an entry unless its name is already present.
    fn insert(&mut self, name: String, entry: Doc) -> bool {
        if self.seen.contains(&name) {
            return false;
        }
        self.seen.insert(name);
        self.entries.push(entry);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Shape;

    /// Table shape for generator data tests.
    #[derive(Shape)]
    #[shape(table)]
    struct SnapshotTable {
        /// The destination name.
        path: String,
    }

    /// Alias shape for generator data tests.
    #[derive(Shape)]
    #[shape(table, untagged)]
    enum SnapshotUnion {
        /// Text payload.
        Text {
            /// The exact file text.
            content: String,
        },
        /// Empty marker.
        Nothing,
    }

    #[test]
    fn generator_data_walk_matches_generated_output() {
        let mut collector = Generator::new();
        SnapshotTable::collect(&mut collector);
        SnapshotUnion::collect(&mut collector);
        assert!(collector.contains("SnapshotTable"));
        assert!(collector.contains("SnapshotUnion"));
        assert!(collector.contains("SnapshotUnion_Text"));
        assert_eq!(
            collector.generate(),
            "--- Table shape for generator data tests.\n--- @class SnapshotTable\n--- @field path string The destination name.\n\n--- Alias shape for generator data tests.\n--- @alias SnapshotUnion SnapshotUnion_Text | nil\n\n--- Text payload.\n--- @class SnapshotUnion_Text\n--- @field content string The exact file text.\n"
        );
    }

    #[test]
    fn generator_data_addition_rejects_duplicates() {
        let mut collector = Generator::new();
        let data = SnapshotTable::shape_data();
        assert!(collector.add(SnapshotTable::name(), &data));
        assert!(!collector.add(SnapshotTable::name(), &data));
        assert!(!collector.add_signature("fun(x: string)".to_owned(), &data));
    }
}
