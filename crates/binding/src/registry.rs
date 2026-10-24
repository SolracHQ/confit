//! Registry
//! Collector holding every bound face once in order.

use std::collections::{HashMap, HashSet};

use crate::error::Result;
use crate::face::{FaceData, FieldDescriptor, FunctionParameter, FunctionReturn, UnionArm};
use crate::stub::{emit, emit_callback_method, emit_method};
use crate::userdata::MethodDescriptor;

/// One collected stub entry.
///
/// Rendering to LuaLS text happens once at emission.
#[derive(Clone, Debug, PartialEq)]
pub enum StubEntry {
    /// One table shape.
    Class {
        /// The class name.
        name: String,
        /// The holder documentation.
        documentation: &'static str,
        /// The table fields.
        fields: Vec<FieldDescriptor>,
    },
    /// One union face.
    Alias {
        /// The alias name.
        name: String,
        /// The holder documentation.
        documentation: &'static str,
        /// The union arms.
        arms: Vec<UnionArm>,
    },
    /// One static function.
    Function {
        /// The function name.
        name: String,
        /// The function documentation.
        documentation: &'static str,
        /// The function params.
        parameters: Vec<FunctionParameter>,
        /// The function returns.
        returns: Vec<FunctionReturn>,
    },
    /// One userdata handle.
    Userdata {
        /// The class name.
        class: &'static str,
        /// The class documentation.
        documentation: &'static str,
        /// The method descriptors.
        methods: Vec<MethodDescriptor>,
    },
}

/// One collected callback signature.
///
/// Params with returns for method stub emission.
#[derive(Clone, Debug)]
struct CallbackSignature {
    /// The callback params in order.
    parameters: Vec<FunctionParameter>,
    /// The callback returns in order.
    returns: Vec<FunctionReturn>,
}

/// Face collector with one entry per Lua name.
///
/// Registration order and
/// stub emission in that order.
#[derive(Debug, Default)]
pub struct Registry {
    /// The seen Lua names.
    seen: HashSet<String>,
    /// The data entries in registration order.
    entries: Vec<StubEntry>,
    /// The callback signatures by call face.
    callbacks: HashMap<String, CallbackSignature>,
}

impl Registry {
    /// Build an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register one class by name with its fields.
    ///
    /// - Repeated names keep the first entry.
    pub(crate) fn register_class(
        &mut self,
        name: String,
        documentation: &'static str,
        fields: Vec<FieldDescriptor>,
    ) -> bool {
        self.insert(
            name.clone(),
            StubEntry::Class {
                name,
                documentation,
                fields,
            },
        )
    }

    /// Register one alias by name with its arms.
    ///
    /// - Repeated names keep the first entry.
    pub(crate) fn register_alias(
        &mut self,
        name: String,
        documentation: &'static str,
        arms: Vec<UnionArm>,
    ) -> bool {
        self.insert(
            name.clone(),
            StubEntry::Alias {
                name,
                documentation,
                arms,
            },
        )
    }

    /// Register one userdata class with its methods.
    ///
    /// - Repeated names keep the first entry.
    pub fn register_userdata(
        &mut self,
        class: &'static str,
        documentation: &'static str,
        methods: Vec<MethodDescriptor>,
    ) -> bool {
        self.insert(
            class.to_owned(),
            StubEntry::Userdata {
                class,
                documentation,
                methods,
            },
        )
    }

    /// Register one face.
    ///
    /// - Repeated names keep the first entry.
    pub fn register(&mut self, name: String, data: &FaceData) -> bool {
        match data {
            FaceData::Scalar { .. } | FaceData::Userdata { .. } => false,
            FaceData::Class {
                documentation,
                fields,
            } => self.register_class(name, documentation, fields.clone()),
            FaceData::Union {
                documentation,
                arms,
            } => self.register_alias(name, documentation, arms.clone()),
            FaceData::Function {
                documentation,
                parameters,
                returns,
            } => self.insert(
                name.clone(),
                StubEntry::Function {
                    name,
                    documentation,
                    parameters: parameters.clone(),
                    returns: returns.clone(),
                },
            ),
        }
    }

    /// Collect one face with every referenced face.
    ///
    /// - Repeated names keep the first entry.
    ///
    /// # Errors
    ///
    /// - [`crate::error::BindingError`] for repeated registration.
    pub fn collect(&mut self, name: String, data: &FaceData) -> Result<()> {
        if self.register(name, data) {
            data.collect_referenced(self)?;
        }
        Ok(())
    }

    /// Register one callback signature.
    ///
    /// - Repeated faces keep the first signature.
    pub fn register_callback(&mut self, face: String, data: &FaceData) -> bool {
        match data {
            FaceData::Function {
                parameters,
                returns,
                ..
            } => {
                if self.callbacks.contains_key(&face) {
                    return false;
                }
                self.callbacks.insert(
                    face,
                    CallbackSignature {
                        parameters: parameters.clone(),
                        returns: returns.clone(),
                    },
                );
                true
            }
            _ => false,
        }
    }

    /// Register one static function.
    ///
    /// - Repeated names keep the first entry.
    pub fn register_function(&mut self, name: &'static str, data: &FaceData) -> bool {
        match data {
            FaceData::Function {
                documentation,
                parameters,
                returns,
            } => self.insert(
                name.to_owned(),
                StubEntry::Function {
                    name: name.to_owned(),
                    documentation,
                    parameters: parameters.clone(),
                    returns: returns.clone(),
                },
            ),
            _ => false,
        }
    }

    /// Render collected stubs in registration order.
    pub fn stub_text(&self) -> String {
        let mut blocks = Vec::new();
        for entry in &self.entries {
            match entry {
                StubEntry::Class {
                    name,
                    documentation,
                    fields,
                } => {
                    let data = FaceData::Class {
                        documentation,
                        fields: fields.clone(),
                    };
                    blocks.extend(self.class_blocks(name, &data));
                }
                StubEntry::Alias {
                    name,
                    documentation,
                    arms,
                } => {
                    let data = FaceData::Union {
                        documentation,
                        arms: arms.clone(),
                    };
                    blocks.push(emit(name, &data).unwrap_or_default());
                }
                StubEntry::Function {
                    name,
                    documentation,
                    parameters,
                    returns,
                } => {
                    let data = FaceData::Function {
                        documentation,
                        parameters: parameters.clone(),
                        returns: returns.clone(),
                    };
                    blocks.push(emit(name, &data).unwrap_or_default());
                }
                StubEntry::Userdata {
                    class,
                    documentation,
                    methods,
                } => {
                    let data = FaceData::Userdata { documentation };
                    let mut out = emit(class, &data).unwrap_or_default();
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

    /// Report whether one name is in the registry.
    pub fn contains(&self, name: &str) -> bool {
        self.seen.contains(name)
    }

    /// Return the signature for one function face.
    fn callback_signature(&self, face: &str) -> Option<&CallbackSignature> {
        let plain = face.strip_suffix('?').unwrap_or(face);
        let plain = plain
            .strip_prefix('(')
            .and_then(|inner| inner.strip_suffix(')'))
            .unwrap_or(plain);
        self.callbacks.get(plain)
    }

    /// Build one class block with its callback methods.
    ///
    /// - Class block comes first with method blocks after.
    /// - Fields without known params hold no method.
    /// - Holders with methods gain one table for dot calls.
    fn class_blocks(&self, name: &str, data: &FaceData) -> Vec<String> {
        let FaceData::Class { fields, .. } = data else {
            return Vec::new();
        };
        let mut methods = Vec::new();
        for field in fields {
            if let Some(signature) = self.callback_signature(field.face.as_ref()) {
                methods.push(emit_callback_method(
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

    /// Insert one entry under its name unless seen.
    fn insert(&mut self, name: String, entry: StubEntry) -> bool {
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
    use crate::face::Face;
    use confit_binding_macros::Face;

    /// Holder face for registry snapshot tests.
    #[derive(Face)]
    #[face(table)]
    struct SnapshotHolder {
        /// The destination route.
        path: String,
    }

    /// Union face for registry snapshot tests.
    #[derive(Face)]
    #[face(table, untagged)]
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
    fn data_walk_matches_snapshot() {
        let mut collector = Registry::new();
        SnapshotHolder::collect(&mut collector).unwrap();
        SnapshotUnion::collect(&mut collector).unwrap();
        assert!(collector.contains("SnapshotHolder"));
        assert!(collector.contains("SnapshotUnion"));
        assert!(collector.contains("SnapshotUnion_Text"));
        assert_eq!(
            collector.stub_text(),
            "--- Holder face for registry snapshot tests.\n--- @class SnapshotHolder\n--- @field path string The destination route.\n\n--- Union face for registry snapshot tests.\n--- @alias SnapshotUnion SnapshotUnion_Text | nil\n\n--- Text payload.\n--- @class SnapshotUnion_Text\n--- @field content string The exact file text.\n"
        );
    }

    #[test]
    fn data_registration_rejects_duplicates() {
        let mut collector = Registry::new();
        let data = SnapshotHolder::face();
        assert!(collector.register(SnapshotHolder::face_name(), &data));
        assert!(!collector.register(SnapshotHolder::face_name(), &data));
        assert!(!collector.register_callback("fun(x: string)".to_owned(), &data));
    }
}
