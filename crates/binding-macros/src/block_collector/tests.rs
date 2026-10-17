//! Collector tests
//! Expansion and rejection coverage for impl blocks.

use syn::ItemImpl;

use super::blocks::expand_block;

fn expansion_of(source: &str) -> String {
    let block: ItemImpl = syn::parse_str(source).unwrap();
    expand_block(&block, None).unwrap().to_string()
}

fn namespace_expansion_of(source: &str, namespace: &str) -> String {
    let block: ItemImpl = syn::parse_str(source).unwrap();
    expand_block(&block, Some(namespace.to_owned()))
        .unwrap()
        .to_string()
}

fn rejection_of(source: &str) -> String {
    let block: ItemImpl = syn::parse_str(source).unwrap();
    expand_block(&block, None).unwrap_err().to_string()
}

fn namespace_rejection_of(source: &str, namespace: &str) -> String {
    let block: ItemImpl = syn::parse_str(source).unwrap();
    expand_block(&block, Some(namespace.to_owned()))
        .unwrap_err()
        .to_string()
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
            /// Answer the ping.
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
            /// Answer the pair.
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
fn shared_receivers_fail_the_build() {
    let mutable = rejection_of("impl Roster { pub fn bump(&mut self) {} }");
    assert!(mutable.contains("&self alone"));
    let owned = rejection_of("impl Roster { pub fn bump(self) {} }");
    assert!(owned.contains("&self alone"));
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
fn statics_without_namespace_fail_the_build() {
    let failure = rejection_of("impl Fetch { pub fn fetch(url: String) {} }");
    assert!(failure.contains("Fetch.fetch: static methods read a namespace"));
    assert!(failure.contains("add namespace to #[face_impl]"));
}

#[test]
fn receivers_with_namespace_fail_the_build() {
    let failure = namespace_rejection_of("impl Fetch { pub fn fetch(&self) {} }", "confit.fetch");
    assert!(failure.contains("Fetch holds &self method fetch inside a namespace block"));
    assert!(failure.contains("namespace blocks hold statics alone"));
}

#[test]
fn optional_on_methods_fails_the_build() {
    let failure = rejection_of(
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
    assert!(failure.contains("Roster.greet.prefix: statics are expected for optional params"));
}
