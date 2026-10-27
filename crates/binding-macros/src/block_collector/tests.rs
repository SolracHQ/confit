//! Collector tests
//! Expansion and rejection coverage for impl blocks.

use confit_binding::registry::Registry;
use syn::ItemImpl;

use super::attribute::ImplMode;
use super::blocks::expand_block;

fn expansion_of(source: &str) -> String {
    let block: ItemImpl = syn::parse_str(source).unwrap();
    expand_block(&block, ImplMode::Userdata)
        .unwrap()
        .to_string()
}

fn namespace_expansion_of(source: &str, namespace: &str) -> String {
    let block: ItemImpl = syn::parse_str(source).unwrap();
    expand_block(&block, ImplMode::Namespace(namespace.to_owned()))
        .unwrap()
        .to_string()
}

fn rejection_of(source: &str) {
    let block: ItemImpl = syn::parse_str(source).unwrap();
    assert!(
        expand_block(&block, ImplMode::Userdata).is_err(),
        "foreign block fails the build"
    );
}

fn namespace_rejection_of(source: &str, namespace: &str) {
    let block: ItemImpl = syn::parse_str(source).unwrap();
    assert!(
        expand_block(&block, ImplMode::Namespace(namespace.to_owned())).is_err(),
        "foreign block fails the build"
    );
}

fn global_rejection_of(source: &str) {
    let block: ItemImpl = syn::parse_str(source).unwrap();
    assert!(
        expand_block(&block, ImplMode::Global).is_err(),
        "foreign block fails the build"
    );
}

fn global_expansion_of(source: &str) -> String {
    let block: ItemImpl = syn::parse_str(source).unwrap();
    expand_block(&block, ImplMode::Global).unwrap().to_string()
}

#[test]
fn lua_method_registers_decoded_params() {
    let output = expansion_of(
        "impl Roster {
            /// List members.
            ///
            /// # Arguments
            ///
            /// - [prefix] Filter by prefix.
            ///
            /// # Returns
            ///
            /// - Member names.
            pub fn members(&self, lua: &Lua, prefix: String) -> mlua::Result<Vec<String>> {
                let _ = (lua, prefix);
                Ok(Vec::new())
            }
        }",
    );
    assert!(output.contains("add_methods"));
    assert!(output.contains("\"members\""));
    assert!(output.contains("take_arg"));
    assert!(output.contains("Decode"));
    assert!(output.contains("Encode"));
    assert!(output.contains("members.prefix"));
    assert!(output.contains("Variadic"));
}

#[test]
fn bare_method_registers_without_params() {
    let output = expansion_of(
        "impl Roster {
            /// Return the ping.
            ///
            /// # Returns
            ///
            /// - The ping text.
            pub fn ping(&self) -> String {
                String::new()
            }
        }",
    );
    assert!(output.contains("add_methods"));
    assert!(output.contains("\"ping\""));
    assert!(output.contains("Encode"));
    assert!(!output.contains("take_arg"));
}

#[test]
fn pair_return_encodes_each_element() {
    let output = expansion_of(
        "impl Roster {
            /// Return the pair.
            ///
            /// # Returns
            ///
            /// - First value, else nil.
            /// - Second value, else nil.
            pub fn pair(&self) -> mlua::Result<(Option<String>, Option<String>)> {
                Ok((None, None))
            }
        }",
    );
    assert!(output.contains("MultiValue"));
    assert!(output.contains("push_back"));
    assert!(output.contains("outcome . 0"));
    assert!(output.contains("outcome . 1"));
}

#[test]
fn foreign_receivers_fail_the_build() {
    rejection_of("impl Roster { pub fn bump(&mut self) {} }");
    rejection_of("impl Roster { pub fn bump(self) {} }");
    rejection_of("impl Fetch { pub fn fetch(url: String) {} }");
    namespace_rejection_of("impl Fetch { pub fn fetch(&self) {} }", "confit.fetch");
    global_rejection_of("impl Fetch { pub fn fetch(&self) {} }");
}

#[test]
fn static_namespace_registers_bare_function() {
    let output = namespace_expansion_of(
        "impl Fetch {
            /// Open one url.
            ///
            /// # Arguments
            ///
            /// - [url] The url to fetch.
            ///
            /// # Returns
            ///
            /// - The handle, else nil.
            /// - The reason, else nil.
            pub fn fetch(url: String) -> (Option<String>, Option<String>) {
                let _ = url;
                (None, None)
            }
        }",
        "confit.fetch",
    );
    assert!(output.contains("NAMESPACE"));
    assert!(output.contains("confit.fetch"));
    assert!(output.contains("VERBS"));
    assert!(output.contains("register"));
    assert!(output.contains("stubs"));
    assert!(output.contains("create_function"));
    assert!(!output.contains("add_methods"));
}

#[test]
fn namespace_document_holds_header_with_bare_return() {
    let output = namespace_expansion_of(
        "impl Fetch {
            /// Open one url.
            ///
            /// # Arguments
            ///
            /// - [url] The url to fetch.
            ///
            /// # Returns
            ///
            /// - The handle, else nil.
            /// - The reason, else nil.
            pub fn fetch(url: String) -> (Option<String>, Option<String>) {
                let _ = url;
                (None, None)
            }
        }",
        "confit.fetch",
    );
    assert!(output.contains("fn stub_document"));
    assert!(output.contains("module_document"));
    assert!(output.contains("StubDocument"));
    let collector = Registry::new();
    let document = confit_binding::stub::module_document("confit.fetch", &["fetch"], &collector);
    assert!(
        document.text.starts_with("--- @meta confit.fetch\n"),
        "namespace document holds the meta header"
    );
    assert!(
        document.text.ends_with("\nreturn fetch\n"),
        "single verbs return the bare name"
    );
}

#[test]
fn global_document_holds_bare_header_with_no_return() {
    let output = global_expansion_of(
        "impl Global {
            /// Build one opaque id.
            ///
            /// # Arguments
            ///
            /// - [name] The base identity text.
            ///
            /// # Returns
            ///
            /// - The opaque identity.
            pub fn id(name: String) -> String {
                name
            }
        }",
    );
    assert!(output.contains("fn stub_document"));
    assert!(output.contains("globals_document"));
    assert!(output.contains("StubDocument"));
    let collector = Registry::new();
    let document = confit_binding::stub::globals_document("confit", &collector);
    assert!(
        document.text.starts_with("--- @meta _\n"),
        "globals document holds the bare header"
    );
    assert!(
        !document.text.contains("\nreturn "),
        "globals document holds no return line"
    );
}
#[test]
fn meta_tostring_registers_hook_and_skips_stub() {
    let output = expansion_of(
        "impl Boom {
            /// Read the message.
            ///
            /// # Returns
            ///
            /// - The message text.
            pub fn message(&self) -> String {
                String::new()
            }

            /// Render the error line.
            ///
            /// # Returns
            ///
            /// - The rendered error line.
            #[face_meta(__tostring)]
            fn render(&self) -> String {
                String::new()
            }
        }",
    );
    assert!(output.contains("add_meta_method"));
    assert!(output.contains("MetaMethod"));
    assert!(output.contains("ToString"));
    assert!(output.contains("add_method"));
    assert!(output.contains("\"message\""));
    assert!(!output.contains("\"render\""));
}

#[test]
fn unknown_meta_hook_fails_the_build() {
    rejection_of(
        "impl Boom {
            /// Render the error line.
            ///
            /// # Returns
            ///
            /// - The rendered error line.
            #[face_meta(__gc)]
            fn render(&self) -> String {
                String::new()
            }
        }",
    );
}

#[test]
fn meta_with_params_fails_the_build() {
    rejection_of(
        "impl Boom {
            /// Render the error line.
            ///
            /// # Arguments
            ///
            /// - [extra] Extra text.
            ///
            /// # Returns
            ///
            /// - The rendered error line.
            #[face_meta(__tostring)]
            fn render(&self, extra: String) -> String {
                extra
            }
        }",
    );
}

#[test]
fn meta_on_static_blocks_fails_the_build() {
    namespace_rejection_of(
        "impl Boom {
            /// Render the error line.
            ///
            /// # Returns
            ///
            /// - The rendered error line.
            #[face_meta(__tostring)]
            fn render(message: String) -> String {
                message
            }
        }",
        "confit.boom",
    );
    global_rejection_of(
        "impl Boom {
            /// Render the error line.
            ///
            /// # Returns
            ///
            /// - The rendered error line.
            #[face_meta(__tostring)]
            fn render(message: String) -> String {
                message
            }
        }",
    );
}

#[test]
fn optional_on_methods_fails_the_build() {
    rejection_of(
        "impl Roster {
            /// Greet.
            ///
            /// # Arguments
            ///
            /// - [prefix] The prefix.
            ///
            /// # Returns
            ///
            /// - Greeting.
            pub fn greet(&self, #[face(optional)] prefix: Option<String>) -> String {
                String::new()
            }
        }",
    );
}
