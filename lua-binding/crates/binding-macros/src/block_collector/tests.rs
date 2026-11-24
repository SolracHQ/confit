//! Tests for impl block expansion and rejection.

use lua_binding::generator::Generator;
use syn::ItemImpl;

use super::attribute::ImplTarget;
use super::blocks::expand_block;

fn expansion_of(source: &str) -> String {
    let block: ItemImpl = syn::parse_str(source).unwrap();
    expand_block(&block, ImplTarget::Object)
        .unwrap()
        .to_string()
}

fn module_expansion_of(source: &str, module: &str) -> String {
    let block: ItemImpl = syn::parse_str(source).unwrap();
    expand_block(&block, ImplTarget::Module(module.to_owned()))
        .unwrap()
        .to_string()
}

fn rejection_of(source: &str) {
    let block: ItemImpl = syn::parse_str(source).unwrap();
    assert!(
        expand_block(&block, ImplTarget::Object).is_err(),
        "foreign block fails the build"
    );
}

fn module_rejection_of(source: &str, module: &str) {
    let block: ItemImpl = syn::parse_str(source).unwrap();
    assert!(
        expand_block(&block, ImplTarget::Module(module.to_owned())).is_err(),
        "foreign block fails the build"
    );
}

fn global_rejection_of(source: &str) {
    let block: ItemImpl = syn::parse_str(source).unwrap();
    assert!(
        expand_block(&block, ImplTarget::Global("demo".to_owned())).is_err(),
        "foreign block fails the build"
    );
}

fn global_expansion_of(source: &str) -> String {
    let block: ItemImpl = syn::parse_str(source).unwrap();
    expand_block(&block, ImplTarget::Global("demo".to_owned()))
        .unwrap()
        .to_string()
}

#[test]
fn lua_method_adds_decoded_params() {
    let output = expansion_of(
        "impl Roster {
            /// Lists members.
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
fn bare_method_adds_without_params() {
    let output = expansion_of(
        "impl Roster {
            /// Returns the ping.
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
            /// Returns the pair.
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
fn wrong_receivers_fail_the_build() {
    let mutable = expansion_of("impl Roster { pub fn bump(&mut self) {} }");
    assert!(mutable.contains("add_method_mut"));
    rejection_of("impl Roster { pub fn bump(self) {} }");
    rejection_of("impl Fetch { pub fn fetch(url: String) {} }");
    module_rejection_of("impl Fetch { pub fn fetch(&self) {} }", "demo.fetch");
    global_rejection_of("impl Fetch { pub fn fetch(&self) {} }");
}

#[test]
fn static_module_adds_bare_function() {
    let output = module_expansion_of(
        "impl Fetch {
            /// Opens a url.
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
        "demo.fetch",
    );
    assert!(output.contains("MODULE"));
    assert!(output.contains("demo.fetch"));
    assert!(output.contains("FUNCTIONS"));
    assert!(output.contains("add"));
    assert!(output.contains("stubs"));
    assert!(output.contains("create_function"));
    assert!(!output.contains("add_methods"));
}

#[test]
fn module_stub_holds_header_with_bare_return() {
    let output = module_expansion_of(
        "impl Fetch {
            /// Opens a url.
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
        "demo.fetch",
    );
    assert!(output.contains("fn stub"));
    assert!(output.contains("generate_module"));
    assert!(output.contains("Stub"));
    let collector = Generator::new();
    let stub = lua_binding::stub::generate_module("demo.fetch", &["fetch"], &collector);
    assert!(
        stub.text.starts_with("--- @meta demo.fetch\n"),
        "module stub holds the meta header"
    );
    assert!(
        stub.text.ends_with("\nreturn fetch\n"),
        "single functions return the bare name"
    );
}

#[test]
fn global_stub_holds_bare_header_with_no_return() {
    let output = global_expansion_of(
        "impl Global {
            /// Builds an opaque id.
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
    assert!(output.contains("fn stub"));
    assert!(output.contains("generate_globals"));
    assert!(output.contains("\"demo\""));
    assert!(output.contains("Stub"));
    let collector = Generator::new();
    let stub = lua_binding::stub::generate_globals("demo", &collector);
    assert!(
        stub.text.starts_with("--- @meta _\n"),
        "globals stub holds the bare header"
    );
    assert!(
        !stub.text.contains("\nreturn "),
        "globals stub holds no return line"
    );
}
#[test]
fn tostring_operator_adds_hook_and_skips_stub() {
    let output = expansion_of(
        "impl Boom {
            /// Returns the message.
            ///
            /// # Returns
            ///
            /// - The message text.
            pub fn message(&self) -> String {
                String::new()
            }

            /// Renders the error line.
            ///
            /// # Returns
            ///
            /// - The rendered error line.
            #[operator(__tostring)]
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
fn unknown_operator_fails_the_build() {
    rejection_of(
        "impl Boom {
            /// Renders the error line.
            ///
            /// # Returns
            ///
            /// - The rendered error line.
            #[operator(__gc)]
            fn render(&self) -> String {
                String::new()
            }
        }",
    );
}

#[test]
fn operator_with_params_fails_the_build() {
    rejection_of(
        "impl Boom {
            /// Renders the error line.
            ///
            /// # Arguments
            ///
            /// - [extra] Extra text.
            ///
            /// # Returns
            ///
            /// - The rendered error line.
            #[operator(__tostring)]
            fn render(&self, extra: String) -> String {
                extra
            }
        }",
    );
}

#[test]
fn operator_on_function_blocks_fails_the_build() {
    module_rejection_of(
        "impl Boom {
            /// Renders the error line.
            ///
            /// # Returns
            ///
            /// - The rendered error line.
            #[operator(__tostring)]
            fn render(message: String) -> String {
                message
            }
        }",
        "demo.boom",
    );
    global_rejection_of(
        "impl Boom {
            /// Renders the error line.
            ///
            /// # Returns
            ///
            /// - The rendered error line.
            #[operator(__tostring)]
            fn render(message: String) -> String {
                message
            }
        }",
    );
}

#[test]
fn add_operator_adds_shared_meta_method() {
    let output = expansion_of(
        "impl Vec2 {
            /// Adds two vectors.
            ///
            /// # Arguments
            ///
            /// - [other] The vector to add.
            ///
            /// # Returns
            ///
            /// - The sum vector.
            #[operator(__add)]
            fn add(&self, other: Vec2) -> Vec2 {
                other
            }
        }",
    );
    assert!(output.contains("add_meta_method"));
    assert!(output.contains("MetaMethod :: Add"));
    assert!(!output.contains("add_meta_method_mut"));
}

#[test]
fn mutable_receiver_adds_mut_method() {
    let output = expansion_of(
        "impl Counter {
            /// Increments the count.
            fn bump(&mut self) {
                self.count += 1;
            }
        }",
    );
    assert!(output.contains("add_method_mut"));
}

#[test]
fn object_field_access_adds_get_and_set() {
    let input: syn::DeriveInput = syn::parse_str(
        "pub struct Counter {
            /// The current count.
            #[shape(get, set)]
            count: i64,
        }",
    )
    .unwrap();
    let syn::Data::Struct(data) = &input.data else {
        panic!("want struct input");
    };
    let output =
        crate::structs::expand_struct_object(&input.ident, data, &input.attrs, &None, &None, false)
            .unwrap()
            .to_string();
    assert!(output.contains("add_field_function_get"));
    assert!(output.contains("add_field_function_set"));
}

#[test]
fn optional_on_methods_fails_the_build() {
    rejection_of(
        "impl Roster {
            /// Greets.
            ///
            /// # Arguments
            ///
            /// - [prefix] The prefix.
            ///
            /// # Returns
            ///
            /// - Greeting.
            pub fn greet(&self, #[shape(optional)] prefix: Option<String>) -> String {
                String::new()
            }
        }",
    );
}
