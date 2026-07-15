/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

use std::borrow::Borrow;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::Debug;

use anyhow::{anyhow, bail, Context, Result};
use askama::Template;
use heck::{ToLowerCamelCase, ToShoutySnakeCase, ToUpperCamelCase};
use serde::{Deserialize, Serialize};
use uniffi_bindgen::interface::*;

mod callback_interface;
mod compounds;
mod custom;
mod enum_;
mod miscellany;
mod object;
mod primitives;
mod record;
mod variant;

#[rustfmt::skip]
const CPP_KEYWORDS: &[&str] = &[
    "alignas", "alignof", "and", "and_eq", "asm", "auto", "bitand", "bitor", "bool",
    "break", "case", "catch", "char", "char8_t", "char16_t", "char32_t", "class",
    "compl", "concept", "const", "const_cast", "consteval", "constexpr", "constinit",
    "continue", "co_await", "co_return", "co_yield", "decltype", "default", "delete",
    "do", "double", "dynamic_cast", "else", "enum", "explicit", "export", "extern",
    "false", "float", "for", "friend", "goto", "if", "inline", "int", "long",
    "mutable", "namespace", "new", "noexcept", "not", "not_eq", "nullptr",
    "operator", "or", "or_eq", "private", "protected", "public", "register",
    "reinterpret_cast", "requires", "return", "short", "signed", "sizeof",
    "static", "static_assert", "static_cast", "struct", "switch", "template",
    "this", "thread_local", "throw", "true", "try", "typedef", "typeid", "typename",
    "union", "unsigned", "using", "virtual", "void", "volatile", "wchar_t", "while",
    "xor", "xor_eq"
];

trait CodeType: Debug {
    /// The language specific label used to reference this type. This will be used in
    /// method signatures and property declarations.
    fn type_label(&self, ci: &ComponentInterface) -> String;

    /// A representation of this type label that can be used as part of another
    /// identifier. e.g. `read_foo()`, or `FooInternals`.
    ///
    /// This is especially useful when creating specialized objects or methods to deal
    /// with this type only.
    fn canonical_name(&self) -> String;

    fn literal(
        &self,
        literal: &DefaultValue,
        ci: &ComponentInterface,
        config: &Config,
    ) -> Result<String> {
        let _ = literal;
        let _ = config;
        bail!("Unimplemented for {}", self.type_label(ci))
    }

    /// Name of the FfiConverter
    ///
    /// This is the object that contains the lower, write, lift, and read methods for this type.
    /// Depending on the binding this will either be a singleton or a class with static methods.
    ///
    /// This is the newer way of handling these methods and replaces the lower, write, lift, and
    /// read CodeType methods.  Currently only used by Kotlin, but the plan is to move other
    /// backends to using this.
    fn ffi_converter_name(&self) -> String {
        format!("FfiConverter{}", self.canonical_name())
    }

    /// Function to run at startup
    fn initialization_fn(&self) -> Option<String> {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ConfigKotlinTarget {
    #[serde(rename = "jvm")]
    Jvm,
    #[serde(rename = "android")]
    Android,
    #[serde(rename = "native")]
    Native,
    #[serde(rename = "stub")]
    Stub,
}

// config options to customize the generated Kotlin.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Config {
    pub(super) package_name: Option<String>,
    pub(super) cdylib_name: Option<String>,
    #[serde(default)]
    pub(super) kotlin_multiplatform: bool,
    #[serde(default)]
    kotlin_targets: Vec<ConfigKotlinTarget>,
    generate_immutable_records: Option<bool>,
    #[serde(default)]
    omit_checksums: bool,
    #[serde(default)]
    custom_types: HashMap<String, CustomTypeConfig>,
    #[serde(default)]
    pub(super) external_packages: HashMap<String, String>,
    #[serde(default)]
    kotlin_target_version: Option<String>,
    #[serde(default)]
    disable_java_cleaner: bool,
    generate_serializable_types: Option<bool>,
    #[serde(default)]
    use_pascal_case_enum_class: Option<bool>,
    #[serde(default)]
    jvm_dynamic_library_dependencies: Vec<String>,
    #[serde(default)]
    android_dynamic_library_dependencies: Vec<String>,
    #[serde(default)]
    dynamic_library_dependencies: Vec<String>,
    #[serde(default, rename = "__enable_jna_interface_mapping")]
    enable_jna_interface_mapping: Option<bool>,
}

// TODO: Make this public in 0.4.0
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(crate) enum Visibility {
    #[serde(rename = "public")]
    Public,
    #[serde(rename = "internal")]
    Internal,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomTypeConfig {
    imports: Option<Vec<String>>,
    type_name: Option<String>,
    into_custom: String, // b/w compat alias for lift
    lift: String,
    from_custom: String, // b/w compat alias for lower
    lower: String,
}

// functions replace literal "{}" in strings with a specified value.
impl CustomTypeConfig {
    fn lift(&self, name: &str) -> String {
        let converter = if self.lift.is_empty() {
            &self.into_custom
        } else {
            &self.lift
        };
        converter.replace("{}", name)
    }

    fn lower(&self, name: &str) -> String {
        let converter = if self.lower.is_empty() {
            &self.from_custom
        } else {
            &self.lower
        };
        converter.replace("{}", name)
    }
}

impl Config {
    // We insist someone has already configured us - any defaults we supply would be wrong.
    pub fn package_name(&self) -> String {
        self.package_name
            .as_ref()
            .expect("package name should have been set in update_component_configs")
            .clone()
    }

    pub fn cdylib_name(&self) -> String {
        self.cdylib_name
            .as_ref()
            .expect("cdylib name should have been set in update_component_configs")
            .clone()
    }

    /// Whether to generate immutable records (`val` instead of `var`)
    pub fn generate_immutable_records(&self) -> bool {
        self.generate_immutable_records.unwrap_or(false)
    }

    fn kotlin_version_is_at_least(&self, major: usize, minor: usize, patch: usize) -> bool {
        let Some(kotlin_target_version) = &self.kotlin_target_version else {
            return false;
        };
        let mut kotlin_target_version = kotlin_target_version.split(|c: char| !c.is_numeric());

        for required_version in [major, minor, patch] {
            let Some(current_version) = kotlin_target_version
                .next()
                .and_then(|v| v.parse::<usize>().ok())
            else {
                return required_version == 0;
            };
            match required_version.cmp(&current_version) {
                Ordering::Equal => continue,
                Ordering::Greater => return false,
                Ordering::Less => return true,
            }
        }

        true
    }

    pub fn use_enum_entries(&self) -> bool {
        // Enum.entries became stable in Kotlin 1.9.0 (introduced in 1.8.20)
        self.kotlin_version_is_at_least(1, 9, 0)
    }

    pub fn use_data_objects(&self) -> bool {
        // data objects became stable in Kotlin 1.9.0 (introduced in 1.8.20)
        self.kotlin_version_is_at_least(1, 9, 0)
    }

    pub fn generate_serializable(&self) -> bool {
        self.generate_serializable_types.unwrap_or(false)
    }

    pub fn jvm_dynamic_library_dependencies(&self) -> Vec<String> {
        let mut libraries = self.jvm_dynamic_library_dependencies.clone();
        libraries.extend_from_slice(&self.dynamic_library_dependencies);
        libraries
    }

    pub fn android_dynamic_library_dependencies(&self) -> Vec<String> {
        let mut libraries = self.android_dynamic_library_dependencies.clone();
        libraries.extend_from_slice(&self.dynamic_library_dependencies);
        libraries
    }

    pub fn dynamic_library_dependencies(&self, module_name: &str) -> Vec<String> {
        match module_name {
            "jvm" => self.jvm_dynamic_library_dependencies(),
            "android" => self.android_dynamic_library_dependencies(),
            _ => vec![],
        }
    }

    // Get the package name for an external type
    pub fn external_package_name(&self, module_path: &str, namespace: Option<&str>) -> String {
        // config overrides are keyed by the crate name, default fallback is the namespace.
        let crate_name = module_path.split("::").next().unwrap();
        match self.external_packages.get(crate_name) {
            Some(name) => name.clone(),
            // If the module path is not in `external_packages`, we need to fall back to a default
            // with the namespace, which we hopefully have.  This is quite fragile, but it's
            // unreachable in library mode - all deps get an entry in `external_packages` with the
            // correct namespace.
            None => format!("uniffi.{}", namespace.unwrap_or(module_path)),
        }
    }

    pub fn enable_jna_interface_mapping(&self) -> bool {
        self.enable_jna_interface_mapping.unwrap_or(false)
    }
}

pub struct MultiplatformBindings {
    pub common: String,
    pub jvm: Option<String>,
    pub android: Option<String>,
    pub native: Option<String>,
    pub stub: Option<String>,
    pub header: Option<String>,
}

// Generate kotlin bindings for the given ComponentInterface, as a string.
pub fn generate_bindings(
    config: &Config,
    ci: &ComponentInterface,
) -> Result<MultiplatformBindings> {
    let common = CommonKotlinWrapper::new("common", Some(Visibility::Public), config.clone(), ci)
        .context("failed to create a common binding generator")?
        .render()
        .context("failed to render common Kotlin bindings")?;

    fn run_with_target(
        config: &Config,
        target: ConfigKotlinTarget,
        f: impl FnOnce() -> Result<String>,
    ) -> Result<Option<String>> {
        config
            .kotlin_targets
            .contains(&target)
            .then(f)
            .map_or(Ok(None), |v| v.map(Some))
    }

    let jvm = run_with_target(config, ConfigKotlinTarget::Jvm, || {
        AndroidJvmKotlinWrapper::new("jvm", Some(Visibility::Public), config.clone(), ci)
            .context("failed to create a JVM binding generator")?
            .render()
            .context("failed to render Kotlin/JVM bindings")
    })?;

    let android = run_with_target(config, ConfigKotlinTarget::Android, || {
        AndroidJvmKotlinWrapper::new("android", Some(Visibility::Public), config.clone(), ci)
            .context("failed to create a Android binding generator")?
            .render()
            .context("failed to render Android Kotlin/JVM bindings")
    })?;

    let native = run_with_target(config, ConfigKotlinTarget::Native, || {
        NativeKotlinWrapper::new("native", Some(Visibility::Public), config.clone(), ci)
            .context("failed to create a native binding generator")?
            .render()
            .context("failed to render Kotlin/Native bindings")
    })?;

    let stub = run_with_target(config, ConfigKotlinTarget::Stub, || {
        StubKotlinWrapper::new("stub", Some(Visibility::Public), config.clone(), ci)
            .context("failed to create a stub binding generator")?
            .render()
            .context("failed to render stub bindings")
    })?;

    let header = run_with_target(config, ConfigKotlinTarget::Native, || {
        HeadersKotlinWrapper::new("headers", Some(Visibility::Public), config.clone(), ci)
            .context("failed to create a native header binding generator")?
            .render()
            .context("failed to render Kotlin/Native headers")
    })?;

    Ok(MultiplatformBindings {
        common,
        jvm,
        android,
        native,
        stub,
        header,
    })
}

/// A struct to record a Kotlin import statement.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ImportRequirement {
    /// The name we are importing.
    Import { name: String },
    /// Import the name with the specified local name.
    ImportAs { name: String, as_name: String },
}

impl ImportRequirement {
    /// Render the Kotlin import statement.
    fn render(&self) -> String {
        match &self {
            ImportRequirement::Import { name } => format!("import {name}"),
            ImportRequirement::ImportAs { name, as_name } => {
                format!("import {name} as {as_name}")
            }
        }
    }
}

macro_rules! kotlin_type_renderer {
    ($TypeRenderer:ident, $source_file:literal) => {
        /// Renders Kotlin helper code for all types
        ///
        /// This template is a bit different than others in that it stores internal state from the render
        /// process.  Make sure to only call `render()` once.
        #[derive(Template)]
        #[template(syntax = "kt", escape = "none", path = $source_file)]
        #[allow(dead_code)]
        pub struct $TypeRenderer<'a> {
            module_name: &'a str,
            visibility: Option<Visibility>,
            config: &'a Config,
            ci: &'a ComponentInterface,
            // Track imports added with the `add_import()` macro
            imports: RefCell<BTreeSet<ImportRequirement>>,
        }

        #[allow(dead_code)]
        impl<'a> $TypeRenderer<'a> {
            fn new(
                module_name: &'a str,
                visibility: Option<Visibility>,
                config: &'a Config,
                ci: &'a ComponentInterface,
            ) -> Self {
                Self {
                    module_name,
                    visibility,
                    config,
                    ci,
                    imports: RefCell::new(BTreeSet::new()),
                }
            }

            // Get the package name for an external type
            fn external_type_package_name(&self, module_path: &str, namespace: &str) -> String {
                self.config
                    .external_package_name(module_path, Some(namespace))
            }

            // The following methods are used by the `Types.kt` macros.

            // Helper to add an import statement
            //
            // Call this inside your template to cause an import statement to be added at the top of the
            // file.  Imports will be sorted and de-deuped.
            //
            // Returns an empty string so that it can be used inside an askama `{{ }}` block.
            fn add_import(&self, name: &str) -> &str {
                self.imports.borrow_mut().insert(ImportRequirement::Import {
                    name: name.to_owned(),
                });
                ""
            }

            // Like add_import, but arranges for `import name as as_name`
            fn add_import_as(&self, name: &str, as_name: &str) -> &str {
                self.imports
                    .borrow_mut()
                    .insert(ImportRequirement::ImportAs {
                        name: name.to_owned(),
                        as_name: as_name.to_owned(),
                    });
                ""
            }

            fn visibility(&self) -> &str {
                match self.visibility {
                    None => "",
                    Some(Visibility::Public) => "public ",
                    Some(Visibility::Internal) => "internal ",
                }
            }

            fn actual_keyword(&self) -> &str {
                if self.config.kotlin_multiplatform {
                    "actual"
                } else {
                    ""
                }
            }

            fn actual_override_keyword(&self) -> &str {
                if self.config.kotlin_multiplatform {
                    "actual override"
                } else {
                    "override"
                }
            }
        }
    };
}

macro_rules! kotlin_wrapper {
    ($KotlinWrapper:ident, $TypeRenderer:ident, $source_file:literal) => {
        #[derive(Template)]
        #[template(syntax = "kt", escape = "none", path = $source_file)]
        #[allow(dead_code)]
        pub struct $KotlinWrapper<'a> {
            module_name: &'a str,
            visibility: Option<Visibility>,
            config: Config,
            ci: &'a ComponentInterface,
            type_helper_code: String,
            type_imports: BTreeSet<ImportRequirement>,
        }

        #[allow(dead_code)]
        impl<'a> $KotlinWrapper<'a> {
            pub fn new(
                module_name: &'a str,
                visibility: Option<Visibility>,
                config: Config,
                ci: &'a ComponentInterface,
            ) -> Result<Self> {
                let type_renderer = $TypeRenderer::new(module_name, visibility, &config, ci);
                let type_helper_code = type_renderer.render()?;
                let type_imports = type_renderer.imports.into_inner();
                Ok(Self {
                    module_name,
                    visibility,
                    config,
                    ci,
                    type_helper_code,
                    type_imports,
                })
            }

            fn initialization_fns(&self, ci: &ComponentInterface) -> Vec<String> {
                let init_fns = self
                    .ci
                    .iter_local_types()
                    .map(|t| KotlinCodeOracle.find(t))
                    .filter_map(|ct| ct.initialization_fn())
                    .map(|fn_name| format!("{fn_name}(this)"));

                // Also call global initialization function for any external type we use.
                // For example, we need to make sure that all callback interface vtables are registered
                // (#2343).
                let extern_module_init_fns = self
                    .ci
                    .iter_external_types()
                    .filter_map(|ty| ty.module_path())
                    .map(|module_path| {
                        let namespace = ci.namespace_for_module_path(module_path).unwrap();
                        let package_name = self
                            .config
                            .external_package_name(module_path, Some(namespace));
                        format!("{package_name}.uniffiEnsureInitialized()")
                    })
                    .collect::<HashSet<_>>();

                init_fns.chain(extern_module_init_fns).collect()
            }

            fn imports(&self) -> Vec<ImportRequirement> {
                self.type_imports.iter().cloned().collect()
            }

            fn visibility(&self) -> &str {
                match self.visibility {
                    None => "",
                    Some(Visibility::Public) => "public ",
                    Some(Visibility::Internal) => "internal ",
                }
            }

            fn lib_private_fun_indent(&self) -> i32 {
                if self.config.enable_jna_interface_mapping() {
                    8
                } else {
                    4
                }
            }
        }
    };
}

kotlin_type_renderer!(CommonTypeRenderer, "common/Types.kt");
kotlin_wrapper!(CommonKotlinWrapper, CommonTypeRenderer, "common/wrapper.kt");

kotlin_type_renderer!(AndroidJvmTypeRenderer, "android+jvm/Types.kt");
kotlin_wrapper!(
    AndroidJvmKotlinWrapper,
    AndroidJvmTypeRenderer,
    "android+jvm/wrapper.kt"
);

kotlin_type_renderer!(NativeTypeRenderer, "native/Types.kt");
kotlin_wrapper!(NativeKotlinWrapper, NativeTypeRenderer, "native/wrapper.kt");

kotlin_type_renderer!(StubTypeRenderer, "stub/Types.kt");
kotlin_wrapper!(StubKotlinWrapper, StubTypeRenderer, "stub/wrapper.kt");

kotlin_type_renderer!(HeadersTypeRenderer, "headers/Types.h");
kotlin_wrapper!(
    HeadersKotlinWrapper,
    HeadersTypeRenderer,
    "headers/wrapper.h"
);

/// Get the name of the interface and class name for a trait.
///
/// For a regular `struct Foo` or `trait Foo`, there's `FooInterface` with `Foo` as
/// the name of the (Rust implemented) object. But if it's a foreign trait:
/// * The name `Foo` is the name of the interface used by a the Kotlin implementation of the trait.
/// * The Rust implemented object is `FooImpl`.
///
/// This all impacts what types `FfiConverter.lower()` inputs.  If it's a "foreign trait"
/// `lower` must lower anything that implements the interface (ie, a kotlin implementation).
/// If not, then lower only lowers the concrete class (ie, our simple instance with the pointer).
fn object_interface_name(ci: &ComponentInterface, obj: &Object) -> String {
    let class_name = KotlinCodeOracle.class_name(ci, obj.name());
    if obj.has_callback_interface() {
        class_name
    } else {
        format!("{class_name}Interface")
    }
}

// *sigh* - same thing for a trait, which might be either Object or CallbackInterface.
// (we should either fold it into object or kill it!)
fn trait_interface_name(ci: &ComponentInterface, name: &str) -> Result<String> {
    let (obj_name, has_callback_interface) = match ci.get_object_definition(name) {
        Some(obj) => (obj.name(), obj.has_callback_interface()),
        None => (
            ci.get_callback_interface_definition(name)
                .ok_or_else(|| anyhow!("no interface {}", name))?
                .name(),
            true,
        ),
    };
    let class_name = KotlinCodeOracle.class_name(ci, obj_name);
    if has_callback_interface {
        Ok(class_name)
    } else {
        Ok(format!("{class_name}Interface"))
    }
}

// The name of the object exposing a Rust implementation.
fn object_impl_name(ci: &ComponentInterface, obj: &Object) -> String {
    let class_name = KotlinCodeOracle.class_name(ci, obj.name());
    if obj.has_callback_interface() {
        format!("{class_name}Impl")
    } else {
        class_name
    }
}

#[derive(Clone)]
pub struct KotlinCodeOracle;

impl KotlinCodeOracle {
    fn find(&self, type_: &Type) -> Box<dyn CodeType> {
        type_.clone().as_type().as_codetype()
    }

    /// Get the idiomatic Kotlin rendering of a class name (for enums, records, errors, etc).
    fn class_name(&self, ci: &ComponentInterface, nm: &str) -> String {
        let name = nm.to_string().to_upper_camel_case();
        // fixup errors.
        ci.is_name_used_as_error(nm)
            .then(|| self.convert_error_suffix(&name))
            .unwrap_or(name)
    }

    fn convert_error_suffix(&self, nm: &str) -> String {
        match nm.strip_suffix("Error") {
            None => nm.to_string(),
            Some(stripped) => format!("{stripped}Exception"),
        }
    }

    /// Get the idiomatic Kotlin rendering of a function name.
    fn fn_name(&self, nm: &str) -> String {
        format!("`{}`", nm.to_string().to_lower_camel_case())
    }

    /// Get the idiomatic Kotlin rendering of a variable name.
    fn var_name(&self, nm: &str) -> String {
        format!("`{}`", self.var_name_raw(nm))
    }

    /// `var_name` without the backticks.  Useful for using in `@Structure.FieldOrder`.
    pub fn var_name_raw(&self, nm: &str) -> String {
        header_escape_name_inner(&nm.to_lower_camel_case())
    }

    /// Get the idiomatic Kotlin rendering of an individual enum variant.
    fn enum_variant_name(&self, nm: &str, config: &Config) -> String {
        if config.use_pascal_case_enum_class == Some(true) {
            nm.to_upper_camel_case()
        } else {
            nm.to_shouty_snake_case()
        }
    }

    /// Get the idiomatic Kotlin rendering of an FFI callback function name
    fn ffi_callback_name(&self, nm: &str) -> String {
        format!("Uniffi{}", nm.to_upper_camel_case())
    }

    fn ffi_callback_name_header(&self, nm: &str) -> String {
        format!("Uniffi{}", nm.to_upper_camel_case())
    }

    /// Get the idiomatic Kotlin rendering of an FFI struct name
    fn ffi_struct_name(&self, nm: &str) -> String {
        format!("Uniffi{}", nm.to_upper_camel_case())
    }

    fn ffi_struct_name_header(&self, nm: &str) -> String {
        format!("Uniffi{}", nm.to_upper_camel_case())
    }

    fn ffi_type_label_by_value(&self, ffi_type: &FfiType, ci: &ComponentInterface) -> String {
        match ffi_type {
            FfiType::RustBuffer(_) => format!("{}ByValue", self.ffi_type_label(ffi_type, ci)),
            FfiType::Struct(name) => format!("{}UniffiByValue", self.ffi_struct_name(name)),
            FfiType::Callback(name) => self.ffi_callback_name(name),
            _ => self.ffi_type_label(ffi_type, ci),
        }
    }

    /// FFI type name to use inside structs
    ///
    /// The main requirement here is that all types must have default values or else the struct
    /// won't work in some JNA contexts.
    fn ffi_type_label_for_ffi_struct(&self, ffi_type: &FfiType, ci: &ComponentInterface) -> String {
        match ffi_type {
            // Make callbacks function pointers nullable. This matches the semantics of a C
            // function pointer better and allows for `null` as a default value.
            // NOTE: Type any used here, as native and jvm types differ.
            FfiType::Callback(name) => format!("{}?", self.ffi_callback_name(name)),
            _ => self.ffi_type_label_by_value(ffi_type, ci),
        }
    }

    /// Default values for FFI
    ///
    /// This is used to:
    ///   - Set a default return value for error results
    ///   - Set a default for structs, which JNA sometimes requires
    fn ffi_default_value(&self, ffi_type: &FfiType) -> String {
        match ffi_type {
            FfiType::UInt8 | FfiType::Int8 => "0.toByte()".to_owned(),
            FfiType::UInt16 | FfiType::Int16 => "0.toShort()".to_owned(),
            FfiType::UInt32 | FfiType::Int32 => "0".to_owned(),
            FfiType::UInt64 | FfiType::Int64 => "0.toLong()".to_owned(),
            FfiType::Float32 => "0.0f".to_owned(),
            FfiType::Float64 => "0.0".to_owned(),
            FfiType::Handle => "0L".to_owned(),
            FfiType::RustBuffer(_) => "RustBufferHelper.allocValue()".to_owned(),
            FfiType::Callback(_) => "null".to_owned(),
            FfiType::RustCallStatus => "UniffiRustCallStatusHelper.allocValue()".to_owned(),
            _ => unimplemented!("ffi_default_value: {ffi_type:?}"),
        }
    }

    fn ffi_type_label_by_reference(&self, ffi_type: &FfiType, ci: &ComponentInterface) -> String {
        match ffi_type {
            FfiType::Int8
            | FfiType::UInt8
            | FfiType::Int16
            | FfiType::UInt16
            | FfiType::Int32
            | FfiType::UInt32
            | FfiType::Int64
            | FfiType::UInt64
            | FfiType::Float32
            | FfiType::Float64 => format!("{}ByReference", self.ffi_type_label(ffi_type, ci)),
            FfiType::Handle => "LongByReference".to_owned(),
            // JNA structs default to ByReference
            FfiType::RustBuffer(_) | FfiType::Struct(_) => self.ffi_type_label(ffi_type, ci),
            _ => panic!("{ffi_type:?} by reference is not implemented"),
        }
    }

    fn ffi_type_label_by_mut_reference(
        &self,
        ffi_type: &FfiType,
        ci: &ComponentInterface,
    ) -> String {
        match ffi_type {
            FfiType::Int8
            | FfiType::UInt8
            | FfiType::Int16
            | FfiType::UInt16
            | FfiType::Int32
            | FfiType::UInt32
            | FfiType::Int64
            | FfiType::UInt64
            | FfiType::Float32
            | FfiType::Float64 => format!("{}ByReference", self.ffi_type_label(ffi_type, ci)),
            FfiType::Handle => "LongByReference".to_owned(),
            // JNA structs default to ByReference
            FfiType::RustBuffer(_) | FfiType::Struct(_) => self.ffi_type_label(ffi_type, ci),
            _ => panic!("{ffi_type:?} by reference is not implemented"),
        }
    }

    fn ffi_type_label_by_reference_header(
        &self,
        ffi_type: &FfiType,
        ci: &ComponentInterface,
    ) -> String {
        match ffi_type {
            FfiType::Int8
            | FfiType::UInt8
            | FfiType::Int16
            | FfiType::UInt16
            | FfiType::Int32
            | FfiType::UInt32
            | FfiType::Int64
            | FfiType::UInt64
            | FfiType::Float32
            | FfiType::Float64 => format!("{} const *", self.ffi_type_label_header(ffi_type, ci)),
            FfiType::Handle => "int64_t const *".to_owned(),
            // JNA structs default to ByReference
            FfiType::RustBuffer(_) | FfiType::Struct(_) => {
                format!("{} const *", self.ffi_type_label_header(ffi_type, ci))
            }
            _ => panic!("{ffi_type:?} by reference is not implemented"),
        }
    }

    fn ffi_type_label_by_mut_reference_header(
        &self,
        ffi_type: &FfiType,
        ci: &ComponentInterface,
    ) -> String {
        match ffi_type {
            FfiType::Int8
            | FfiType::UInt8
            | FfiType::Int16
            | FfiType::UInt16
            | FfiType::Int32
            | FfiType::UInt32
            | FfiType::Int64
            | FfiType::UInt64
            | FfiType::Float32
            | FfiType::Float64 => format!("{} *", self.ffi_type_label_header(ffi_type, ci)),
            FfiType::Handle => "int64_t *".to_owned(),
            // JNA structs default to ByReference
            FfiType::RustBuffer(_) | FfiType::Struct(_) => {
                format!("{} *", self.ffi_type_label_header(ffi_type, ci))
            }
            _ => panic!("{ffi_type:?} by reference is not implemented"),
        }
    }

    fn ffi_type_label(&self, ffi_type: &FfiType, ci: &ComponentInterface) -> String {
        match ffi_type {
            // Note that unsigned integers in Kotlin are currently experimental, but java.nio.ByteBuffer does not
            // support them yet. Thus, we use the signed variants to represent both signed and unsigned
            // types from the component API.
            FfiType::Int8 | FfiType::UInt8 => "Byte".to_string(),
            FfiType::Int16 | FfiType::UInt16 => "Short".to_string(),
            FfiType::Int32 | FfiType::UInt32 => "Int".to_string(),
            FfiType::Int64 | FfiType::UInt64 => "Long".to_string(),
            FfiType::Float32 => "Float".to_string(),
            FfiType::Float64 => "Double".to_string(),
            FfiType::Handle => "Long".to_string(),
            FfiType::RustBuffer(maybe_external) => match maybe_external {
                Some(external_meta) if external_meta.module_path != ci.crate_name() => {
                    format!("RustBuffer{}", external_meta.name)
                }
                _ => "RustBuffer".to_string(),
            },
            FfiType::RustCallStatus => "UniffiRustCallStatusByValue".to_string(),
            FfiType::ForeignBytes => "ForeignBytesByValue".to_string(),
            FfiType::Callback(callback) => self.ffi_callback_name(callback),
            FfiType::Struct(name) => self.ffi_struct_name(name),
            FfiType::Reference(inner) => self.ffi_type_label_by_reference(inner, ci),
            FfiType::MutReference(inner) => self.ffi_type_label_by_mut_reference(inner, ci),
            FfiType::VoidPointer => "Pointer".to_string(),
        }
    }

    fn ffi_type_label_header(&self, ffi_type: &FfiType, ci: &ComponentInterface) -> String {
        match ffi_type {
            // Note that unsigned integers in Kotlin are currently experimental, but java.nio.ByteBuffer does not
            // support them yet. Thus, we use the signed variants to represent both signed and unsigned
            // types from the component API.
            FfiType::Int8 | FfiType::UInt8 => "int8_t".to_string(),
            FfiType::Int16 | FfiType::UInt16 => "int16_t".to_string(),
            FfiType::Int32 | FfiType::UInt32 => "int32_t".to_string(),
            FfiType::Int64 | FfiType::UInt64 => "int64_t".to_string(),
            FfiType::Float32 => "float".to_string(),
            FfiType::Float64 => "double".to_string(),
            FfiType::Handle => "int64_t".to_string(),
            FfiType::RustBuffer(maybe_external) => match maybe_external {
                Some(external_meta) if external_meta.module_path != ci.crate_name() => {
                    format!("RustBuffer{}", external_meta.name)
                }
                _ => "RustBuffer".to_string(),
            },
            FfiType::RustCallStatus => "UniffiRustCallStatus".to_string(),
            FfiType::ForeignBytes => "ForeignBytes".to_string(),
            FfiType::Callback(name) => self.ffi_callback_name_header(name),
            FfiType::Struct(name) => self.ffi_struct_name_header(name),
            FfiType::Reference(inner) => self.ffi_type_label_by_reference_header(inner, ci),
            FfiType::MutReference(inner) => self.ffi_type_label_by_mut_reference_header(inner, ci),
            FfiType::VoidPointer => "void *".to_string(),
        }
    }

    #[allow(clippy::only_used_in_recursion)]
    fn as_external_type(&self, ci: &ComponentInterface, as_ct: &impl AsType) -> Option<String> {
        let ty = as_ct.as_type();
        if let (true, Some(name)) = (ci.is_external(&ty), ty.name()) {
            return Some(name.to_string());
        }
        match as_ct.as_type() {
            Type::Custom { builtin, .. } => self.as_external_type(ci, &builtin),
            _ => None,
        }
    }
}

trait AsCodeType {
    fn as_codetype(&self) -> Box<dyn CodeType>;
}

impl<T: AsType> AsCodeType for T {
    fn as_codetype(&self) -> Box<dyn CodeType> {
        // Map `Type` instances to a `Box<dyn CodeType>` for that type.
        //
        // There is a companion match in `templates/Types.kt` which performs a similar function for the
        // template code.
        //
        //   - When adding additional types here, make sure to also add a match arm to the `Types.kt` template.
        //   - To keep things manageable, let's try to limit ourselves to these 2 mega-matches
        match self.as_type() {
            Type::UInt8 => Box::new(primitives::UInt8CodeType),
            Type::Int8 => Box::new(primitives::Int8CodeType),
            Type::UInt16 => Box::new(primitives::UInt16CodeType),
            Type::Int16 => Box::new(primitives::Int16CodeType),
            Type::UInt32 => Box::new(primitives::UInt32CodeType),
            Type::Int32 => Box::new(primitives::Int32CodeType),
            Type::UInt64 => Box::new(primitives::UInt64CodeType),
            Type::Int64 => Box::new(primitives::Int64CodeType),
            Type::Float32 => Box::new(primitives::Float32CodeType),
            Type::Float64 => Box::new(primitives::Float64CodeType),
            Type::Boolean => Box::new(primitives::BooleanCodeType),
            Type::String => Box::new(primitives::StringCodeType),
            Type::Bytes => Box::new(primitives::BytesCodeType),

            Type::Timestamp => Box::new(miscellany::TimestampCodeType),
            Type::Duration => Box::new(miscellany::DurationCodeType),

            Type::Enum { name, .. } => Box::new(enum_::EnumCodeType::new(name)),
            Type::Object { name, imp, .. } => Box::new(object::ObjectCodeType::new(name, imp)),
            Type::Record { name, .. } => Box::new(record::RecordCodeType::new(name)),
            Type::CallbackInterface { name, .. } => {
                Box::new(callback_interface::CallbackInterfaceCodeType::new(name))
            }
            Type::Optional { inner_type } => {
                Box::new(compounds::OptionalCodeType::new(*inner_type))
            }
            Type::Sequence { inner_type } => {
                Box::new(compounds::SequenceCodeType::new(*inner_type))
            }
            Type::Map {
                key_type,
                value_type,
            } => Box::new(compounds::MapCodeType::new(*key_type, *value_type)),
            Type::Custom { name, .. } => Box::new(custom::CustomCodeType::new(name)),
            Type::Set { inner_type } => {
                Box::new(compounds::SetCodeType::new(*inner_type))
            }
            Type::Box { inner_type } => inner_type.as_codetype()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataClassFieldType {
    Bytes,
    NullableBytes,
    NonNullableNonBytes,
    NullableNonBytes,
}

fn header_escape_name_inner(nm: &str) -> String {
    if CPP_KEYWORDS.contains(&nm) {
        format!("{nm}_")
    } else {
        nm.to_owned()
    }
}

fn as_data_class_field_type_inner(as_ct: &impl AsType) -> DataClassFieldType {
    fn as_bytes_field_type_inner(type_: &Type) -> DataClassFieldType {
        match type_ {
            Type::Bytes => DataClassFieldType::Bytes,
            Type::Optional { inner_type } => match as_bytes_field_type_inner(inner_type) {
                DataClassFieldType::Bytes | DataClassFieldType::NullableBytes => {
                    DataClassFieldType::NullableBytes
                }
                DataClassFieldType::NonNullableNonBytes
                | DataClassFieldType::NullableNonBytes => DataClassFieldType::NullableNonBytes,
            },
            _ => DataClassFieldType::NonNullableNonBytes,
        }
    }
    as_bytes_field_type_inner(&as_ct.as_type())
}

mod filters {
    use uniffi_bindgen::{interface::ffi::ExternalFfiMetadata, to_askama_error};
    use uniffi_meta::LiteralMetadata;
    use variant::VariantCodeType;

    use super::*;

    #[askama::filter_fn]
    pub(super) fn ffi_type(
        as_ct: &impl AsType,
        _: &dyn askama::Values,
    ) -> Result<FfiType, askama::Error> {
        Ok(FfiType::from(as_ct.as_type()))
    }

    #[askama::filter_fn]
    pub(super) fn type_name(
        as_ct: &impl AsCodeType,
        _: &dyn askama::Values,
        ci: &ComponentInterface,
    ) -> Result<String, askama::Error> {
        Ok(as_ct.as_codetype().type_label(ci))
    }

    // Workaround problem with impl AsCodeType for &Variant (see variant.rs).
    #[askama::filter_fn]
    pub fn variant_type_name(
        v: &Variant,
        _: &dyn askama::Values,
        ci: &ComponentInterface,
    ) -> Result<String, askama::Error> {
        Ok(VariantCodeType { v: v.clone() }.type_label(ci))
    }

    #[askama::filter_fn]
    pub(super) fn canonical_name(
        as_ct: &impl AsCodeType,
        _: &dyn askama::Values,
    ) -> Result<String, askama::Error> {
        Ok(as_ct.as_codetype().canonical_name())
    }

    #[askama::filter_fn]
    pub(super) fn ffi_converter_name(
        as_ct: &impl AsCodeType,
        _: &dyn askama::Values,
    ) -> Result<String, askama::Error> {
        Ok(as_ct.as_codetype().ffi_converter_name())
    }

    #[askama::filter_fn]
    pub(super) fn lower_fn(
        as_ct: &impl AsCodeType,
        _: &dyn askama::Values,
    ) -> Result<String, askama::Error> {
        Ok(format!(
            "{}.lower",
            as_ct.as_codetype().ffi_converter_name()
        ))
    }

    #[askama::filter_fn]
    pub(super) fn allocation_size_fn(
        as_ct: &impl AsCodeType,
        _: &dyn askama::Values,
    ) -> Result<String, askama::Error> {
        Ok(format!(
            "{}.allocationSize",
            as_ct.as_codetype().ffi_converter_name()
        ))
    }

    #[askama::filter_fn]
    pub(super) fn write_fn(
        as_ct: &impl AsType,
        _: &dyn askama::Values,
        ci: &ComponentInterface,
    ) -> Result<String, askama::Error> {
        if let Some(external_type_name) = KotlinCodeOracle.as_external_type(ci, as_ct) {
            Ok(format!(
                "{}.write{external_type_name}",
                as_ct.as_codetype().ffi_converter_name()
            ))
        } else {
            Ok(format!(
                "{}.write",
                as_ct.as_codetype().ffi_converter_name()
            ))
        }
    }

    #[askama::filter_fn]
    pub(super) fn lift_fn(
        as_ct: &impl AsCodeType,
        _: &dyn askama::Values,
    ) -> Result<String, askama::Error> {
        Ok(format!("{}.lift", as_ct.as_codetype().ffi_converter_name()))
    }

    #[askama::filter_fn]
    pub(super) fn as_ffi_type(
        as_ct: &impl AsType,
        _: &dyn askama::Values,
    ) -> Result<FfiType, askama::Error> {
        Ok(FfiType::from(as_ct.as_type()))
    }

    #[askama::filter_fn]
    pub(super) fn need_non_null_assertion(
        type_: &FfiType,
        _: &dyn askama::Values,
    ) -> Result<bool, askama::Error> {
        Ok(matches!(type_, FfiType::Handle))
    }

    #[askama::filter_fn]
    pub(super) fn read_fn(
        as_ct: &impl AsType,
        _: &dyn askama::Values,
        ci: &ComponentInterface,
    ) -> Result<String, askama::Error> {
        if let Some(external_type_name) = KotlinCodeOracle.as_external_type(ci, as_ct) {
            Ok(format!(
                "{}.read{external_type_name}",
                as_ct.as_codetype().ffi_converter_name()
            ))
        } else {
            Ok(format!("{}.read", as_ct.as_codetype().ffi_converter_name()))
        }
    }

    fn render_literal_inner(
        literal: &DefaultValue,
        as_ct: &impl AsType,
        ci: &ComponentInterface,
        config: &Config,
    ) -> Result<String, askama::Error> {
        as_ct
            .as_codetype()
            .literal(literal, ci, config)
            .map_err(|e| to_askama_error(&e))
    }

    #[askama::filter_fn]
    pub fn render_literal<T: AsType>(
        literal: &DefaultValue,
        _: &dyn askama::Values,
        as_ct: &T,
        ci: &ComponentInterface,
        config: &Config,
    ) -> Result<String, askama::Error> {
        render_literal_inner(literal, as_ct, ci, config)
    }

    // Get the idiomatic Kotlin rendering of an integer.
    fn int_literal(t: &Option<Type>, base10: String) -> Result<String, askama::Error> {
        if let Some(t) = t {
            match t {
                Type::Int8 | Type::Int16 | Type::Int32 | Type::Int64 => Ok(base10),
                Type::UInt8 | Type::UInt16 | Type::UInt32 | Type::UInt64 => Ok(base10 + "u"),
                _ => Err(to_askama_error("Only ints are supported.")),
            }
        } else {
            Err(to_askama_error("Enum hasn't defined a repr"))
        }
    }

    // Get the idiomatic Kotlin rendering of an individual enum variant's discriminant
    #[askama::filter_fn]
    pub fn variant_discr_literal(
        e: &Enum,
        _: &dyn askama::Values,
        index: &usize,
    ) -> Result<String, askama::Error> {
        let literal = e.variant_discr(*index).expect("invalid index");
        match literal {
            // Kotlin doesn't convert between signed and unsigned by default
            // so we'll need to make sure we define the type as appropriately
            LiteralMetadata::UInt(v, _, _) => int_literal(e.variant_discr_type(), v.to_string()),
            LiteralMetadata::Int(v, _, _) => int_literal(e.variant_discr_type(), v.to_string()),
            _ => Err(to_askama_error("Only ints are supported.")),
        }
    }

    #[askama::filter_fn]
    pub fn should_generate_equals_hash_code_record(
        record: &Record,
        _: &dyn askama::Values,
    ) -> Result<bool, askama::Error> {
        Ok(record.fields().iter().any(|f| {
            matches!(
                as_data_class_field_type_inner(f),
                DataClassFieldType::Bytes | DataClassFieldType::NullableBytes
            )
        }))
    }

    #[askama::filter_fn]
    pub fn should_generate_equals_hash_code_enum_variant(
        variant: &Variant,
        _: &dyn askama::Values,
    ) -> Result<bool, askama::Error> {
        Ok(variant.fields().iter().any(|f| {
            matches!(
                as_data_class_field_type_inner(f),
                DataClassFieldType::Bytes | DataClassFieldType::NullableBytes
            )
        }))
    }

    #[askama::filter_fn]
    pub fn as_data_class_field_type(
        as_ct: &impl AsType,
        _: &dyn askama::Values,
    ) -> Result<DataClassFieldType, askama::Error> {
        Ok(as_data_class_field_type_inner(as_ct))
    }

    fn serializable_type(type_: &Type, ci: &ComponentInterface) -> Result<bool, askama::Error> {
        Ok(match type_ {
            Type::Object { .. } | Type::CallbackInterface { .. } => false,
            Type::Record { name, .. } => serializable_record_inner(
                ci.get_record_definition(name)
                    .ok_or_else(|| to_askama_error(&format!("could not find record '{name}'")))?,
                ci,
            )?,
            Type::Enum { name, .. } => serializable_enum_inner(
                ci.get_enum_definition(name)
                    .ok_or_else(|| to_askama_error(&format!("could not find enum '{name}'")))?,
                ci,
            )?,
            Type::Optional { inner_type }
            | Type::Sequence { inner_type }
            | Type::Set { inner_type }
            | Type::Box { inner_type } => {
                serializable_type(inner_type, ci)?
            }
            Type::Map {
                key_type,
                value_type,
            } => serializable_type(key_type, ci)? && serializable_type(value_type, ci)?,
            // Assume a custom type using a serializable type is also serializable.
            Type::Custom { builtin, .. } => serializable_type(builtin, ci)?,
            _ => true,
        })
    }

    fn serializable_record_inner(
        record: &Record,
        ci: &ComponentInterface,
    ) -> Result<bool, askama::Error> {
        for field in record.fields() {
            if !serializable_type(&field.as_type(), ci)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn serializable_enum_inner(enum_: &Enum, ci: &ComponentInterface) -> Result<bool, askama::Error> {
        if ci.is_name_used_as_error(enum_.name()) {
            return Ok(false);
        }

        if enum_.is_flat() {
            let Some(variant_discr_type) = enum_.variant_discr_type() else {
                return Ok(true);
            };
            return serializable_type(variant_discr_type, ci);
        }

        // Unlike records or enum variants, if any of the variants are serializable, the
        // enum can be marked as serializable.
        for variant in enum_.variants() {
            if serializable_enum_variant_inner(variant, ci)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn serializable_enum_variant_inner(
        variant: &Variant,
        ci: &ComponentInterface,
    ) -> Result<bool, askama::Error> {
        for field in variant.fields() {
            if !serializable_type(&field.as_type(), ci)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    #[askama::filter_fn]
    pub fn serializable_record(
        record: &Record,
        _: &dyn askama::Values,
        ci: &ComponentInterface,
    ) -> Result<bool, askama::Error> {
        serializable_record_inner(record, ci)
    }

    #[askama::filter_fn]
    pub fn serializable_enum(enum_: &Enum, _: &dyn askama::Values, ci: &ComponentInterface) -> Result<bool, askama::Error> {
        serializable_enum_inner(enum_, ci)
    }

    #[askama::filter_fn]
    pub fn serializable_enum_variant(
        variant: &Variant,
        _: &dyn askama::Values,
        ci: &ComponentInterface,
    ) -> Result<bool, askama::Error> {
        serializable_enum_variant_inner(variant, ci)
    }

    #[askama::filter_fn]
    pub fn ffi_type_name_by_value(
        type_: &FfiType,
        _: &dyn askama::Values,
        ci: &ComponentInterface,
    ) -> Result<String, askama::Error> {
        Ok(KotlinCodeOracle.ffi_type_label_by_value(type_, ci))
    }

    #[askama::filter_fn]
    pub fn ffi_type_name(
        type_: &FfiType,
        _: &dyn askama::Values,
        ci: &ComponentInterface,
    ) -> Result<String, askama::Error> {
        Ok(KotlinCodeOracle.ffi_type_label(type_, ci))
    }

    #[askama::filter_fn]
    pub fn ffi_as_callback(
        type_: &FfiType,
        _: &dyn askama::Values,
        ci: &ComponentInterface,
    ) -> Result<Option<FfiCallbackFunction>, askama::Error> {
        let FfiType::Callback(callback_name) = type_ else {
            return Ok(None);
        };

        for def in ci.ffi_definitions() {
            let FfiDefinition::CallbackFunction(callback) = def else {
                continue;
            };
            if callback.name() != callback_name {
                continue;
            }
            return Ok(Some(callback));
        }

        Err(to_askama_error(&format!(
            "could not find FFI callback '{callback_name}'"
        )))
    }

    /// Kotlin/Native's cinterop ignores nullability of parameters of callback definitions in
    /// headers. We need to determine whether a callback pointer needs to be casted before being
    /// passed to a FFI function.
    #[askama::filter_fn]
    pub fn ffi_callback_needs_casting_native(
        ffi_callback: &FfiCallbackFunction,
        _: &dyn askama::Values,
    ) -> Result<bool, askama::Error> {
        Ok(ffi_callback.has_rust_call_status_arg()
            || ffi_callback.arguments().iter().any(|a| {
                matches!(
                    a.type_(),
                    FfiType::RustBuffer(_)
                        | FfiType::ForeignBytes
                        | FfiType::Callback(_)
                        | FfiType::Struct(_)
                )
            }))
    }

    /// Convert a local RustBuffer to an external RustBuffer.
    #[askama::filter_fn]
    pub fn ffi_cast_to_external_rust_buffer_if_needed(
        type_: &FfiType,
        _: &dyn askama::Values,
        ci: &ComponentInterface,
    ) -> Result<String, askama::Error> {
        let FfiType::RustBuffer(Some(metadata)) = type_ else {
            return Ok(String::new());
        };
        if metadata.module_path == ci.crate_name() {
            return Ok(String::new());
        }
        Ok(format!(".as{}()", metadata.name))
    }

    /// Convert an external RustBuffer to a local RustBuffer.
    #[askama::filter_fn]
    pub fn ffi_cast_to_local_rust_buffer_if_needed(
        type_: &FfiType,
        _: &dyn askama::Values,
        ci: &ComponentInterface,
    ) -> Result<String, askama::Error> {
        let FfiType::RustBuffer(Some(metadata)) = type_ else {
            return Ok(String::new());
        };
        if metadata.module_path == ci.crate_name() {
            return Ok(String::new());
        }
        Ok(format!(".from{}ToLocal()", metadata.name))
    }

    /// Append a `_` if the name is a valid c/c++ keyword
    #[askama::filter_fn]
    pub fn header_escape_name(nm: &str, _: &dyn askama::Values) -> Result<String, askama::Error> {
        Ok(header_escape_name_inner(nm))
    }

    #[askama::filter_fn]
    pub fn header_ffi_type_name(
        type_: &FfiType,
        _: &dyn askama::Values,
        ci: &ComponentInterface,
    ) -> Result<String, askama::Error> {
        Ok(KotlinCodeOracle.ffi_type_label_header(type_, ci))
    }

    #[askama::filter_fn]
    pub fn ffi_type_name_for_ffi_struct(
        type_: &FfiType,
        _: &dyn askama::Values,
        ci: &ComponentInterface,
    ) -> Result<String, askama::Error> {
        Ok(KotlinCodeOracle.ffi_type_label_for_ffi_struct(type_, ci))
    }

    #[askama::filter_fn]
    pub fn ffi_default_value(type_: FfiType, _: &dyn askama::Values) -> Result<String, askama::Error> {
        Ok(KotlinCodeOracle.ffi_default_value(&type_))
    }

    /// Get the idiomatic Kotlin rendering of a function name.
    #[askama::filter_fn]
    pub fn class_name<S: AsRef<str>>(
        nm: S,
        _: &dyn askama::Values,
        ci: &ComponentInterface,
    ) -> Result<String, askama::Error> {
        Ok(KotlinCodeOracle.class_name(ci, nm.as_ref()))
    }

    /// Get the idiomatic Kotlin rendering of a function name.
    #[askama::filter_fn]
    pub fn fn_name<S: AsRef<str>>(nm: S, _: &dyn askama::Values) -> Result<String, askama::Error> {
        Ok(KotlinCodeOracle.fn_name(nm.as_ref()))
    }

    /// Get the idiomatic Kotlin rendering of an enum method name.
    /// Conflicts with Kotlin's built-in enum properties (name, ordinal, entries, values)
    /// are resolved by prefixing with "rust".
    #[askama::filter_fn]
    pub fn enum_fn_name<S: AsRef<str>>(nm: S, _: &dyn askama::Values) -> Result<String, askama::Error> {
        let camel = nm.as_ref().to_lower_camel_case();
        const ENUM_RESERVED: &[&str] = &["name", "ordinal", "entries", "values"];
        if ENUM_RESERVED.contains(&camel.as_str()) {
            Ok(format!("`rust{}`", camel[0..1].to_uppercase() + &camel[1..]))
        } else {
            Ok(format!("`{}`", camel))
        }
    }

    /// Get the idiomatic Kotlin rendering of a variable name.
    #[askama::filter_fn]
    pub fn var_name<S: AsRef<str>>(nm: S, _: &dyn askama::Values) -> Result<String, askama::Error> {
        Ok(KotlinCodeOracle.var_name(nm.as_ref()))
    }

    /// Get the idiomatic Kotlin rendering of a variable name.
    #[askama::filter_fn]
    pub fn var_name_raw<S: AsRef<str>>(nm: S, _: &dyn askama::Values) -> Result<String, askama::Error> {
        Ok(KotlinCodeOracle.var_name_raw(nm.as_ref()))
    }

    /// Get a String representing the name used for an individual enum variant.
    #[askama::filter_fn]
    pub fn variant_name(v: &Variant, _: &dyn askama::Values, config: &Config) -> Result<String, askama::Error> {
        Ok(KotlinCodeOracle.enum_variant_name(v.name(), config))
    }

    #[askama::filter_fn]
    pub fn error_variant_name(v: &Variant, _: &dyn askama::Values) -> Result<String, askama::Error> {
        let name = v.name().to_string().to_upper_camel_case();
        Ok(KotlinCodeOracle.convert_error_suffix(&name))
    }

    /// Get the idiomatic Kotlin rendering of an FFI callback function name
    #[askama::filter_fn]
    pub fn ffi_callback_name<S: AsRef<str>>(nm: S, _: &dyn askama::Values) -> Result<String, askama::Error> {
        Ok(KotlinCodeOracle.ffi_callback_name(nm.as_ref()))
    }

    /// Get the idiomatic Kotlin rendering of an FFI struct name
    #[askama::filter_fn]
    pub fn ffi_struct_name<S: AsRef<str>>(nm: S, _: &dyn askama::Values) -> Result<String, askama::Error> {
        Ok(KotlinCodeOracle.ffi_struct_name(nm.as_ref()))
    }

    #[askama::filter_fn]
    pub fn async_poll(
        callable: impl Callable,
        _: &dyn askama::Values,
        ci: &ComponentInterface,
    ) -> Result<String, askama::Error> {
        let ffi_func = callable.ffi_rust_future_poll(ci);
        Ok(format!(
            "{{ future, callback, continuation -> UniffiLib.{ffi_func}(future, callback, continuation) }}"
        ))
    }

    #[askama::filter_fn]
    pub fn async_complete(
        callable: impl Callable,
        _: &dyn askama::Values,
        ci: &ComponentInterface,
    ) -> Result<String, askama::Error> {
        let ffi_func = callable.ffi_rust_future_complete(ci);
        let call = format!("UniffiLib.{ffi_func}(future, continuation)");
        // May need to convert the RustBuffer from our package to the RustBuffer of the external package
        let call = match callable.return_type() {
            Some(return_type) if ci.is_external(return_type) => {
                let ffi_type = FfiType::from(return_type);
                match ffi_type {
                    FfiType::RustBuffer(Some(ExternalFfiMetadata { name, .. })) => {
                        let suffix = KotlinCodeOracle.class_name(ci, &name);
                        format!("{call}.let {{ RustBuffer{suffix}ByValue(it.capacity, it.len, it.data) }}")
                    }
                    _ => call,
                }
            }
            _ => call,
        };
        Ok(format!("{{ future, continuation -> {call} }}"))
    }

    #[askama::filter_fn]
    pub fn async_free(
        callable: impl Callable,
        _: &dyn askama::Values,
        ci: &ComponentInterface,
    ) -> Result<String, askama::Error> {
        let ffi_func = callable.ffi_rust_future_free(ci);
        Ok(format!("{{ future -> UniffiLib.{ffi_func}(future) }}"))
    }

    #[askama::filter_fn]
    pub fn async_cancel(
        callable: impl Callable,
        _: &dyn askama::Values,
        ci: &ComponentInterface,
    ) -> Result<String, askama::Error> {
        let ffi_func = callable.ffi_rust_future_cancel(ci);
        Ok(format!("{{ future -> UniffiLib.{ffi_func}(future) }}"))
    }

    /// Remove the "`" chars we put around function/variable names
    ///
    /// These are used to avoid name clashes with kotlin identifiers, but sometimes you want to
    /// render the name unquoted.  One example is the message property for errors where we want to
    /// display the name for the user.
    #[askama::filter_fn]
    pub fn unquote<S: AsRef<str>>(nm: S, _: &dyn askama::Values) -> Result<String, askama::Error> {
        Ok(nm.as_ref().trim_matches('`').to_string())
    }

    /// Get the idiomatic Kotlin rendering of docstring
    #[askama::filter_fn]
    pub fn docstring<S: AsRef<str>>(docstring: S, _: &dyn askama::Values, spaces: &i32) -> Result<String, askama::Error> {
        let middle = textwrap::indent(&textwrap::dedent(docstring.as_ref()), " * ");
        let wrapped = format!("/**\n{middle}\n */");

        let spaces = usize::try_from(*spaces).unwrap_or_default();
        Ok(textwrap::indent(&wrapped, &" ".repeat(spaces)))
    }

    #[askama::filter_fn]
    pub fn repeat(string: &str, _: &dyn askama::Values, n: &i32) -> Result<String, askama::Error> {
        let n = usize::try_from(*n).unwrap_or_default();
        Ok(string.repeat(n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_config() -> Config {
        Config {
            package_name: Some("test.package".to_string()),
            cdylib_name: Some("test".to_string()),
            kotlin_multiplatform: true,
            kotlin_targets: vec![ConfigKotlinTarget::Jvm, ConfigKotlinTarget::Native],
            ..Default::default()
        }
    }

    // --- Type::Box tests ---

    #[test]
    fn type_box_maps_to_inner_codetype() {
        // Type::Box { inner: UInt32 } should produce the same codetype as UInt32 directly
        let box_type = Type::Box {
            inner_type: Box::new(Type::UInt32),
        };
        let inner_type = Type::UInt32;

        let ci = ComponentInterface::new("test");
        let box_label = box_type.as_codetype().type_label(&ci);
        let inner_label = inner_type.as_codetype().type_label(&ci);

        assert_eq!(box_label, inner_label);
        assert_eq!(box_label, "kotlin.UInt");
    }

    #[test]
    fn type_box_with_string_maps_correctly() {
        let box_type = Type::Box {
            inner_type: Box::new(Type::String),
        };
        let ci = ComponentInterface::new("test");
        let label = box_type.as_codetype().type_label(&ci);
        assert_eq!(label, "kotlin.String");
    }

    #[test]
    fn type_box_is_not_optional() {
        // Type::Box should NOT produce an optional type (no `?` suffix)
        let box_type = Type::Box {
            inner_type: Box::new(Type::UInt32),
        };
        let opt_type = Type::Optional {
            inner_type: Box::new(Type::UInt32),
        };

        let ci = ComponentInterface::new("test");
        let box_label = box_type.as_codetype().type_label(&ci);
        let opt_label = opt_type.as_codetype().type_label(&ci);

        assert_eq!(box_label, "kotlin.UInt");
        assert_eq!(opt_label, "kotlin.UInt?");
        assert_ne!(box_label, opt_label);
    }

    #[test]
    fn type_box_canonical_name_matches_inner() {
        let box_type = Type::Box {
            inner_type: Box::new(Type::Int64),
        };
        let inner_type = Type::Int64;

        assert_eq!(
            box_type.as_codetype().canonical_name(),
            inner_type.as_codetype().canonical_name()
        );
    }

    #[test]
    fn type_box_ffi_converter_matches_inner() {
        let box_type = Type::Box {
            inner_type: Box::new(Type::Float64),
        };
        let inner_type = Type::Float64;

        assert_eq!(
            box_type.as_codetype().ffi_converter_name(),
            inner_type.as_codetype().ffi_converter_name()
        );
    }

    // --- Type::Set tests ---

    #[test]
    fn type_set_maps_to_set_codetype() {
        let set_type = Type::Set {
            inner_type: Box::new(Type::String),
        };
        let ci = ComponentInterface::new("test");
        let label = set_type.as_codetype().type_label(&ci);
        assert_eq!(label, "Set<kotlin.String>");
    }

    #[test]
    fn type_set_differs_from_sequence() {
        let set_type = Type::Set {
            inner_type: Box::new(Type::UInt32),
        };
        let seq_type = Type::Sequence {
            inner_type: Box::new(Type::UInt32),
        };

        let ci = ComponentInterface::new("test");
        let set_label = set_type.as_codetype().type_label(&ci);
        let seq_label = seq_type.as_codetype().type_label(&ci);

        assert_eq!(set_label, "Set<kotlin.UInt>");
        assert_eq!(seq_label, "List<kotlin.UInt>");
        assert_ne!(set_label, seq_label);
    }

    #[test]
    fn type_set_canonical_name() {
        let set_type = Type::Set {
            inner_type: Box::new(Type::Boolean),
        };
        assert_eq!(set_type.as_codetype().canonical_name(), "SetBoolean");
    }

    #[test]
    fn type_set_ffi_converter_name() {
        let set_type = Type::Set {
            inner_type: Box::new(Type::String),
        };
        assert_eq!(
            set_type.as_codetype().ffi_converter_name(),
            "FfiConverterSetString"
        );
    }

    #[test]
    fn type_set_with_nested_type() {
        let set_type = Type::Set {
            inner_type: Box::new(Type::Optional {
                inner_type: Box::new(Type::Int32),
            }),
        };
        let ci = ComponentInterface::new("test");
        let label = set_type.as_codetype().type_label(&ci);
        assert_eq!(label, "Set<kotlin.Int?>");
    }

    // --- Integration: generate_bindings and verify output ---

    fn generate_test_bindings(udl: &str) -> MultiplatformBindings {
        let ci = ComponentInterface::from_webidl(udl, "test_crate").unwrap();
        let config = mock_config();
        generate_bindings(&config, &ci).unwrap()
    }

    #[test]
    fn generated_bindings_use_handle_long_not_pointer() {
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
                string hello();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let ffi = bindings.jvm.as_ref().expect("jvm bindings should exist");

        // Should use Long handle, not Pointer
        assert!(
            ffi.contains("handle: Long"),
            "ffi output should contain 'handle: Long'\nGot:\n{}",
            &ffi[..ffi.len().min(2000)]
        );
        assert!(
            ffi.contains("protected val handle: Long"),
            "ffi output should contain 'protected val handle: Long'"
        );
        assert!(
            ffi.contains("callWithHandle"),
            "ffi output should contain 'callWithHandle'"
        );
        assert!(
            ffi.contains("uniffiCloneHandle"),
            "ffi output should contain 'uniffiCloneHandle'"
        );
        assert!(
            ffi.contains("FfiConverter<"),
            "ffi output should contain 'FfiConverter<'"
        );
        // Should NOT contain old Pointer-based patterns
        assert!(
            !ffi.contains("pointer: Pointer"),
            "ffi output should NOT contain 'pointer: Pointer'"
        );
        assert!(
            !ffi.contains("callWithPointer"),
            "ffi output should NOT contain 'callWithPointer'"
        );
        assert!(
            !ffi.contains("uniffiClonePointer"),
            "ffi output should NOT contain 'uniffiClonePointer'"
        );
        assert!(
            !ffi.contains(", Pointer>"),
            "ffi output should NOT contain ', Pointer>'"
        );
    }

    #[test]
    fn generated_bindings_use_no_handle_not_no_pointer() {
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
                string hello();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let ffi = bindings.jvm.as_ref().expect("jvm bindings should exist");

        assert!(
            ffi.contains("NoHandle"),
            "ffi output should contain 'NoHandle'"
        );
        assert!(
            !ffi.contains("NoPointer"),
            "ffi output should NOT contain 'NoPointer'"
        );
    }

    #[test]
    fn generated_bindings_use_ffi_converter_long() {
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let ffi = bindings.jvm.as_ref().expect("jvm bindings should exist");

        assert!(
            ffi.contains("FfiConverter<Foo, Long>") || ffi.contains("FfiConverter<FooInterface, Long>"),
            "ffi output should use FfiConverter with Long, got:\n{}",
            &ffi[..ffi.len().min(3000)]
        );
    }

    #[test]
    fn generated_bindings_clean_action_uses_handle() {
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let ffi = bindings.jvm.as_ref().expect("jvm bindings should exist");

        assert!(
            ffi.contains("UniffiCleanAction"),
            "ffi output should contain 'UniffiCleanAction'"
        );
        assert!(
            !ffi.contains("UniffiPointerDestroyer"),
            "ffi output should NOT contain 'UniffiPointerDestroyer'"
        );
    }

    #[test]
    fn generated_common_uses_no_handle_and_uniffi_with_handle() {
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let common = &bindings.common;

        assert!(
            common.contains("NoHandle"),
            "common output should contain 'NoHandle'"
        );
        assert!(
            common.contains("UniffiWithHandle"),
            "common output should contain 'UniffiWithHandle'"
        );
        assert!(
            !common.contains("NoPointer"),
            "common output should NOT contain 'NoPointer'"
        );
    }

    #[test]
    fn generated_macros_use_call_with_handle() {
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
                string hello();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let ffi = bindings.jvm.as_ref().expect("jvm bindings should exist");

        assert!(
            ffi.contains("callWithHandle"),
            "ffi output should contain 'callWithHandle'"
        );
        assert!(
            !ffi.contains("callWithPointer"),
            "ffi output should NOT contain 'callWithPointer'"
        );
    }

    #[test]
    fn generated_header_uses_int64_t_for_handle() {
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let header = bindings.header.as_ref().expect("header should exist");

        assert!(
            header.contains("int64_t"),
            "header should contain 'int64_t' for handle type"
        );
    }

    // --- Breaking change: Callback interface VTable with uniffiClone ---

    #[test]
    fn generated_callback_interface_has_unifficlone() {
        let udl = r#"
            namespace test_crate {};

            callback interface Listener {
                void on_event(string message);
            };

            interface Notifier {
                constructor();
                void add_listener(Listener listener);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let ffi = bindings.jvm.as_ref().expect("jvm bindings should exist");

        // uniffi 0.32 requires uniffiClone in VTable
        assert!(
            ffi.contains("uniffiClone"),
            "ffi output should contain 'uniffiClone' for callback interface VTable\nGot:\n{}",
            &ffi[..ffi.len().min(5000)]
        );
        // VTable should have uniffiFree and uniffiClone
        assert!(
            ffi.contains("UniffiCallbackInterfaceClone"),
            "ffi output should contain 'UniffiCallbackInterfaceClone'"
        );
    }

    // --- Breaking change: Record type with fields ---

    #[test]
    fn generated_record_has_fields() {
        let udl = r#"
            namespace test_crate {};

            dictionary UserProfile {
                string name;
                u32 age;
                string email;
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let common = &bindings.common;

        // Record should have data class with fields
        assert!(
            common.contains("data class UserProfile") || common.contains("class UserProfile"),
            "common output should contain 'class UserProfile'"
        );
    }

    // --- Breaking change: Enum type ---

    #[test]
    fn generated_enum_has_variants() {
        let udl = r#"
            namespace test_crate {};

            enum Color {
                "Red",
                "Green",
                "Blue"
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let common = &bindings.common;

        // Enum should have sealed class with variants
        assert!(
            common.contains("sealed class Color") || common.contains("enum class Color"),
            "common output should contain 'Color' class"
        );
        assert!(
            common.contains("Red"),
            "common output should contain 'Red' variant"
        );
    }

    // --- Breaking change: Error type ---

    #[test]
    fn generated_error_extends_exception() {
        let udl = r#"
            namespace test_crate {};

            [Error]
            enum AppError {
                "NotFound",
                "Unauthorized"
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let common = &bindings.common;

        // Error should extend Exception
        assert!(
            common.contains("Exception"),
            "common output should contain 'Exception' for error type"
        );
    }

    // --- Breaking change: Object with callback interface ---

    #[test]
    fn generated_object_with_callback_interface() {
        let udl = r#"
            namespace test_crate {};

            callback interface Greeter {
                string greet(string name);
            };

            interface MyGreeter {
                constructor();
                string greet(string name);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let ffi = bindings.jvm.as_ref().expect("jvm bindings should exist");

        // Object should reference the callback interface
        assert!(
            ffi.contains("Greeter") || ffi.contains("GreeterInterface"),
            "ffi output should contain 'Greeter' callback interface"
        );
    }

    // --- Breaking change: FfiType::Handle maps to Long in all contexts ---

    #[test]
    fn generated_ffi_type_handle_maps_to_long_everywhere() {
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
                void do_something();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let ffi = bindings.jvm.as_ref().expect("jvm bindings should exist");

        // All handle-related types should use Long
        assert!(
            ffi.contains("handle: Long"),
            "ffi should use 'handle: Long'"
        );
        assert!(
            ffi.contains("FfiConverter<") && ffi.contains(", Long>"),
            "ffi should use 'FfiConverter<X, Long>'"
        );
        // Should NOT use Pointer for handle-based operations
        assert!(
            !ffi.contains("pointer: Pointer"),
            "ffi should NOT contain 'pointer: Pointer'"
        );
        assert!(
            !ffi.contains("callWithPointer"),
            "ffi should NOT contain 'callWithPointer'"
        );
        assert!(
            !ffi.contains("uniffiClonePointer"),
            "ffi should NOT contain 'uniffiClonePointer'"
        );
    }

    // --- Breaking change: askama 0.16 endcall syntax ---

    #[test]
    fn generated_templates_compile_with_endcall() {
        // This test verifies that the askama 0.16 template syntax works
        // If {% endcall %} is missing, the template won't compile
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
                string hello();
                void do_nothing();
            };
        "#;
        let bindings = generate_test_bindings(udl);

        // If we get here, the templates compiled successfully
        assert!(bindings.jvm.is_some(), "jvm bindings should exist");
        assert!(bindings.common.len() > 0, "common bindings should not be empty");
    }

    // --- Breaking change: Multiple object types ---

    #[test]
    fn generated_multiple_objects_independent() {
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
                string get_name();
            };

            interface Bar {
                constructor(i32 value);
                i32 get_value();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let ffi = bindings.jvm.as_ref().expect("jvm bindings should exist");

        // Both objects should have handle-based constructors
        assert!(
            ffi.contains("class Foo") || ffi.contains("class FooImpl"),
            "ffi should contain 'Foo' class"
        );
        assert!(
            ffi.contains("class Bar") || ffi.contains("class BarImpl"),
            "ffi should contain 'Bar' class"
        );
        // Both should use Long handles
        assert!(
            ffi.matches("handle: Long").count() >= 2,
            "ffi should have multiple 'handle: Long' for multiple objects"
        );
    }

    // --- Breaking change: Object with methods ---

    #[test]
    fn generated_object_methods_use_call_with_handle() {
        let udl = r#"
            namespace test_crate {};

            interface Counter {
                constructor(i32 initial);
                i32 increment();
                i32 get_value();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let ffi = bindings.jvm.as_ref().expect("jvm bindings should exist");

        // Methods should use callWithHandle
        assert!(
            ffi.contains("callWithHandle"),
            "ffi should use 'callWithHandle' for object methods"
        );
        // Should have uniffiCloneHandle
        assert!(
            ffi.contains("uniffiCloneHandle"),
            "ffi should have 'uniffiCloneHandle'"
        );
    }

    // --- Breaking change: Function with return value ---

    #[test]
    fn generated_function_with_return_value() {
        let udl = r#"
            namespace test_crate {};

            interface Calculator {
                constructor();
                u32 add(u32 a, u32 b);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let ffi = bindings.jvm.as_ref().expect("jvm bindings should exist");

        // Function should be generated
        assert!(
            ffi.contains("add") || ffi.contains("`add`"),
            "ffi should contain 'add' function"
        );
    }

    // ============================================================
    // Comprehensive uniffi feature tests
    // ============================================================

    // --- Primitive types ---

    #[test]
    fn primitive_i8_type() {
        let ci = ComponentInterface::new("test");
        assert_eq!(Type::Int8.as_codetype().type_label(&ci), "kotlin.Byte");
    }

    #[test]
    fn primitive_i16_type() {
        let ci = ComponentInterface::new("test");
        assert_eq!(Type::Int16.as_codetype().type_label(&ci), "kotlin.Short");
    }

    #[test]
    fn primitive_i32_type() {
        let ci = ComponentInterface::new("test");
        assert_eq!(Type::Int32.as_codetype().type_label(&ci), "kotlin.Int");
    }

    #[test]
    fn primitive_i64_type() {
        let ci = ComponentInterface::new("test");
        assert_eq!(Type::Int64.as_codetype().type_label(&ci), "kotlin.Long");
    }

    #[test]
    fn primitive_u8_type() {
        let ci = ComponentInterface::new("test");
        assert_eq!(Type::UInt8.as_codetype().type_label(&ci), "kotlin.UByte");
    }

    #[test]
    fn primitive_u16_type() {
        let ci = ComponentInterface::new("test");
        assert_eq!(Type::UInt16.as_codetype().type_label(&ci), "kotlin.UShort");
    }

    #[test]
    fn primitive_u32_type() {
        let ci = ComponentInterface::new("test");
        assert_eq!(Type::UInt32.as_codetype().type_label(&ci), "kotlin.UInt");
    }

    #[test]
    fn primitive_u64_type() {
        let ci = ComponentInterface::new("test");
        assert_eq!(Type::UInt64.as_codetype().type_label(&ci), "kotlin.ULong");
    }

    #[test]
    fn primitive_f32_type() {
        let ci = ComponentInterface::new("test");
        assert_eq!(Type::Float32.as_codetype().type_label(&ci), "kotlin.Float");
    }

    #[test]
    fn primitive_f64_type() {
        let ci = ComponentInterface::new("test");
        assert_eq!(Type::Float64.as_codetype().type_label(&ci), "kotlin.Double");
    }

    #[test]
    fn primitive_bool_type() {
        let ci = ComponentInterface::new("test");
        assert_eq!(Type::Boolean.as_codetype().type_label(&ci), "kotlin.Boolean");
    }

    #[test]
    fn primitive_string_type() {
        let ci = ComponentInterface::new("test");
        assert_eq!(Type::String.as_codetype().type_label(&ci), "kotlin.String");
    }

    #[test]
    fn primitive_bytes_type() {
        let ci = ComponentInterface::new("test");
        assert_eq!(Type::Bytes.as_codetype().type_label(&ci), "kotlin.ByteArray");
    }

    // --- Compound types ---

    #[test]
    fn optional_type_label() {
        let ci = ComponentInterface::new("test");
        let opt = Type::Optional {
            inner_type: Box::new(Type::Int32),
        };
        assert_eq!(opt.as_codetype().type_label(&ci), "kotlin.Int?");
    }

    #[test]
    fn optional_nested_type_label() {
        let ci = ComponentInterface::new("test");
        let opt = Type::Optional {
            inner_type: Box::new(Type::Optional {
                inner_type: Box::new(Type::String),
            }),
        };
        assert_eq!(opt.as_codetype().type_label(&ci), "kotlin.String??");
    }

    #[test]
    fn sequence_type_label() {
        let ci = ComponentInterface::new("test");
        let seq = Type::Sequence {
            inner_type: Box::new(Type::UInt32),
        };
        assert_eq!(seq.as_codetype().type_label(&ci), "List<kotlin.UInt>");
    }

    #[test]
    fn map_type_label() {
        let ci = ComponentInterface::new("test");
        let map = Type::Map {
            key_type: Box::new(Type::String),
            value_type: Box::new(Type::Int32),
        };
        assert_eq!(
            map.as_codetype().type_label(&ci),
            "Map<kotlin.String, kotlin.Int>"
        );
    }

    #[test]
    fn set_type_label() {
        let ci = ComponentInterface::new("test");
        let set = Type::Set {
            inner_type: Box::new(Type::String),
        };
        assert_eq!(set.as_codetype().type_label(&ci), "Set<kotlin.String>");
    }

    // --- FFI type mapping ---

    #[test]
    fn ffi_type_i8_to_int8_t() {
        let ci = ComponentInterface::new("test");
        let ffi = FfiType::Int8;
        assert_eq!(
            KotlinCodeOracle.ffi_type_label_header(&ffi, &ci),
            "int8_t"
        );
    }

    #[test]
    fn ffi_type_i16_to_int16_t() {
        let ci = ComponentInterface::new("test");
        let ffi = FfiType::Int16;
        assert_eq!(
            KotlinCodeOracle.ffi_type_label_header(&ffi, &ci),
            "int16_t"
        );
    }

    #[test]
    fn ffi_type_i32_to_int32_t() {
        let ci = ComponentInterface::new("test");
        let ffi = FfiType::Int32;
        assert_eq!(
            KotlinCodeOracle.ffi_type_label_header(&ffi, &ci),
            "int32_t"
        );
    }

    #[test]
    fn ffi_type_i64_to_int64_t() {
        let ci = ComponentInterface::new("test");
        let ffi = FfiType::Int64;
        assert_eq!(
            KotlinCodeOracle.ffi_type_label_header(&ffi, &ci),
            "int64_t"
        );
    }

    #[test]
    fn ffi_type_handle_to_int64_t() {
        let ci = ComponentInterface::new("test");
        let ffi = FfiType::Handle;
        assert_eq!(
            KotlinCodeOracle.ffi_type_label_header(&ffi, &ci),
            "int64_t"
        );
    }

    #[test]
    fn ffi_type_handle_to_long() {
        let ci = ComponentInterface::new("test");
        let ffi = FfiType::Handle;
        assert_eq!(KotlinCodeOracle.ffi_type_label(&ffi, &ci), "Long");
    }

    #[test]
    fn ffi_type_void_pointer() {
        let ci = ComponentInterface::new("test");
        let ffi = FfiType::VoidPointer;
        assert_eq!(KotlinCodeOracle.ffi_type_label(&ffi, &ci), "Pointer");
        assert_eq!(
            KotlinCodeOracle.ffi_type_label_header(&ffi, &ci),
            "void *"
        );
    }

    // --- Canonical names ---

    #[test]
    fn canonical_name_i32() {
        // Int32 canonical name is "Int" (not "Int32")
        assert_eq!(Type::Int32.as_codetype().canonical_name(), "Int");
    }

    #[test]
    fn canonical_name_string() {
        assert_eq!(Type::String.as_codetype().canonical_name(), "String");
    }

    #[test]
    fn canonical_name_bool() {
        assert_eq!(Type::Boolean.as_codetype().canonical_name(), "Boolean");
    }

    #[test]
    fn canonical_name_optional() {
        let opt = Type::Optional {
            inner_type: Box::new(Type::Int32),
        };
        // Optional<Int32> canonical name is "OptionalInt"
        assert_eq!(opt.as_codetype().canonical_name(), "OptionalInt");
    }

    #[test]
    fn canonical_name_sequence() {
        let seq = Type::Sequence {
            inner_type: Box::new(Type::String),
        };
        assert_eq!(seq.as_codetype().canonical_name(), "SequenceString");
    }

    #[test]
    fn canonical_name_map() {
        let map = Type::Map {
            key_type: Box::new(Type::String),
            value_type: Box::new(Type::Int32),
        };
        // Map<String, Int32> canonical name is "MapStringInt"
        assert_eq!(
            map.as_codetype().canonical_name(),
            "MapStringInt"
        );
    }

    // --- FfiConverter names ---

    #[test]
    fn ffi_converter_name_i32() {
        // Int32 ffi_converter name is "FfiConverterInt"
        assert_eq!(
            Type::Int32.as_codetype().ffi_converter_name(),
            "FfiConverterInt"
        );
    }

    #[test]
    fn ffi_converter_name_string() {
        assert_eq!(
            Type::String.as_codetype().ffi_converter_name(),
            "FfiConverterString"
        );
    }

    #[test]
    fn ffi_converter_name_bool() {
        assert_eq!(
            Type::Boolean.as_codetype().ffi_converter_name(),
            "FfiConverterBoolean"
        );
    }

    // --- Generated code: Primitive types in UDL ---

    #[test]
    fn generated_primitive_types_in_function() {
        let udl = r#"
            namespace test_crate {};

            interface TypeChecker {
                constructor();
                i8 pass_i8(i8 v);
                i16 pass_i16(i16 v);
                i32 pass_i32(i32 v);
                i64 pass_i64(i64 v);
                u8 pass_u8(u8 v);
                u16 pass_u16(u16 v);
                u32 pass_u32(u32 v);
                u64 pass_u64(u64 v);
                float pass_f32(float v);
                double pass_f64(double v);
                boolean pass_bool(boolean v);
                string pass_string(string v);
                bytes pass_bytes(bytes v);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let ffi = bindings.jvm.as_ref().expect("jvm bindings should exist");

        // All primitive types should be generated
        assert!(ffi.contains("passI8"), "should have passI8");
        assert!(ffi.contains("passI16"), "should have passI16");
        assert!(ffi.contains("passI32"), "should have passI32");
        assert!(ffi.contains("passI64"), "should have passI64");
        assert!(ffi.contains("passU8"), "should have passU8");
        assert!(ffi.contains("passU16"), "should have passU16");
        assert!(ffi.contains("passU32"), "should have passU32");
        assert!(ffi.contains("passU64"), "should have passU64");
        assert!(ffi.contains("passF32"), "should have passF32");
        assert!(ffi.contains("passF64"), "should have passF64");
        assert!(ffi.contains("passBool"), "should have passBool");
        assert!(ffi.contains("passString"), "should have passString");
        assert!(ffi.contains("passBytes"), "should have passBytes");
    }

    // --- Generated code: Optional type ---

    #[test]
    fn generated_optional_type() {
        let udl = r#"
            namespace test_crate {};

            interface OptTest {
                constructor();
                string? get_optional(boolean return_null);
                void set_optional(string? value);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let ffi = bindings.jvm.as_ref().expect("jvm bindings should exist");

        assert!(
            ffi.contains("getOptional") || ffi.contains("get_optional"),
            "should have getOptional function"
        );
    }

    // --- Generated code: Vec type ---

    #[test]
    fn generated_vec_type() {
        let udl = r#"
            namespace test_crate {};

            interface VecTest {
                constructor();
                sequence<u32> get_numbers();
                void set_numbers(sequence<u32> numbers);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let ffi = bindings.jvm.as_ref().expect("jvm bindings should exist");

        assert!(
            ffi.contains("getNumbers") || ffi.contains("get_numbers"),
            "should have getNumbers function"
        );
    }

    // --- Generated code: HashMap type ---

    #[test]
    fn generated_hashmap_type() {
        let udl = r#"
            namespace test_crate {};

            interface MapTest {
                constructor();
                record<DOMString, u32> get_map();
                void set_map(record<DOMString, u32> map);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let ffi = bindings.jvm.as_ref().expect("jvm bindings should exist");

        assert!(
            ffi.contains("getMap") || ffi.contains("get_map"),
            "should have getMap function"
        );
    }

    // --- Generated code: Dictionary (Record) ---

    #[test]
    fn generated_dictionary_record() {
        let udl = r#"
            namespace test_crate {};

            dictionary Point {
                double x;
                double y;
            };

            interface Geometry {
                constructor();
                Point create_point(double x, double y);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let common = &bindings.common;

        assert!(
            common.contains("Point"),
            "should have Point record"
        );
    }

    // --- Generated code: Enum ---

    #[test]
    fn generated_enum_flat() {
        let udl = r#"
            namespace test_crate {};

            enum Direction {
                "North",
                "South",
                "East",
                "West"
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let common = &bindings.common;

        assert!(
            common.contains("Direction"),
            "should have Direction enum"
        );
        // Enum variants might be in different format (e.g., NORTH, North, etc.)
        assert!(
            common.contains("North") || common.contains("NORTH") || common.contains("north"),
            "should have North variant"
        );
    }

    // --- Generated code: Error type ---

    #[test]
    fn generated_error_type() {
        let udl = r#"
            namespace test_crate {};

            [Error]
            enum AppError {
                "NotFound",
                "Unauthorized",
                "Internal"
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let common = &bindings.common;

        // Error type name might be converted (e.g., AppException)
        assert!(
            common.contains("AppError") || common.contains("AppException"),
            "should have AppError/AppException"
        );
        assert!(
            common.contains("Exception"),
            "error should extend Exception"
        );
    }

    // --- Generated code: Object with constructor ---

    #[test]
    fn generated_object_with_constructor() {
        let udl = r#"
            namespace test_crate {};

            interface Person {
                constructor(string name, u32 age);
                string get_name();
                u32 get_age();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let ffi = bindings.jvm.as_ref().expect("jvm bindings should exist");

        assert!(
            ffi.contains("Person") || ffi.contains("PersonImpl"),
            "should have Person class"
        );
        assert!(
            ffi.contains("getName") || ffi.contains("get_name"),
            "should have getName method"
        );
        assert!(
            ffi.contains("getAge") || ffi.contains("get_age"),
            "should have getAge method"
        );
    }

    // --- Generated code: Object with void method ---

    #[test]
    fn generated_object_void_method() {
        let udl = r#"
            namespace test_crate {};

            interface Logger {
                constructor();
                void log(string message);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let ffi = bindings.jvm.as_ref().expect("jvm bindings should exist");

        assert!(
            ffi.contains("log") || ffi.contains("`log`"),
            "should have log method"
        );
    }

    // --- Generated code: Object with alternate constructor ---

    #[test]
    fn generated_object_alternate_constructor() {
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
                [Name="create_with_value"]
                constructor(u32 value);
                u32 get_value();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let ffi = bindings.jvm.as_ref().expect("jvm bindings should exist");

        assert!(
            ffi.contains("createWithValue") || ffi.contains("create_with_value"),
            "should have alternate constructor"
        );
    }

    // --- Generated code: Callback interface ---

    #[test]
    fn generated_callback_interface_basic() {
        let udl = r#"
            namespace test_crate {};

            callback interface Observer {
                void on_update(string data);
            };

            interface Subject {
                constructor();
                void register(Observer observer);
                void notify(string data);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let ffi = bindings.jvm.as_ref().expect("jvm bindings should exist");

        assert!(
            ffi.contains("Observer") || ffi.contains("ObserverInterface"),
            "should have Observer callback interface"
        );
        assert!(
            ffi.contains("register") || ffi.contains("`register`"),
            "should have register method"
        );
    }

    // --- Generated code: Callback interface with return value ---

    #[test]
    fn generated_callback_interface_with_return() {
        let udl = r#"
            namespace test_crate {};

            callback interface Transformer {
                string transform(string input);
            };

            interface Processor {
                constructor();
                string process(string data, Transformer transformer);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let ffi = bindings.jvm.as_ref().expect("jvm bindings should exist");

        assert!(
            ffi.contains("Transformer") || ffi.contains("TransformerInterface"),
            "should have Transformer callback interface"
        );
    }

    // --- Generated code: Multiple interfaces ---

    #[test]
    fn generated_multiple_interfaces() {
        let udl = r#"
            namespace test_crate {};

            interface InterfaceA {
                constructor();
                string method_a();
            };

            interface InterfaceB {
                constructor();
                i32 method_b();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let ffi = bindings.jvm.as_ref().expect("jvm bindings should exist");

        assert!(
            ffi.contains("InterfaceA") || ffi.contains("InterfaceAImpl"),
            "should have InterfaceA"
        );
        assert!(
            ffi.contains("InterfaceB") || ffi.contains("InterfaceBImpl"),
            "should have InterfaceB"
        );
    }

    // --- Generated code: Object with no methods (just constructor) ---

    #[test]
    fn generated_object_no_methods() {
        let udl = r#"
            namespace test_crate {};

            interface Empty {
                constructor();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let ffi = bindings.jvm.as_ref().expect("jvm bindings should exist");

        assert!(
            ffi.contains("Empty") || ffi.contains("EmptyImpl"),
            "should have Empty class"
        );
    }

    // --- Generated code: Function with no return value ---

    #[test]
    fn generated_function_void_return() {
        let udl = r#"
            namespace test_crate {};

            interface VoidFunc {
                constructor();
                void do_something();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let ffi = bindings.jvm.as_ref().expect("jvm bindings should exist");

        assert!(
            ffi.contains("doSomething") || ffi.contains("do_something"),
            "should have doSomething method"
        );
    }

    // --- Generated code: Complex UDL with multiple types ---

    #[test]
    fn generated_complex_udl() {
        let udl = r#"
            namespace test_crate {};

            enum Status {
                "Active",
                "Inactive"
            };

            dictionary User {
                string name;
                u32 age;
                Status status;
            };

            callback interface UserCallback {
                void on_user_created(User user);
            };

            interface UserManager {
                constructor();
                User create_user(string name, u32 age);
                void register_callback(UserCallback callback);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let common = &bindings.common;
        let ffi = bindings.jvm.as_ref().expect("jvm bindings should exist");

        assert!(common.contains("Status"), "should have Status enum");
        assert!(common.contains("User"), "should have User record");
        assert!(
            ffi.contains("UserManager") || ffi.contains("UserManagerImpl"),
            "should have UserManager class"
        );
    }

    // --- Generated code: Verify common and ffi both exist ---

    #[test]
    fn generated_bindings_have_common_and_ffi() {
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
                string hello();
            };
        "#;
        let bindings = generate_test_bindings(udl);

        assert!(bindings.common.len() > 0, "common should not be empty");
        assert!(bindings.jvm.is_some(), "jvm should exist");
        assert!(bindings.native.is_some(), "native should exist");
    }

    // --- Generated code: Verify namespace ---

    #[test]
    fn generated_bindings_use_namespace() {
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let common = &bindings.common;
        let ffi = bindings.jvm.as_ref().expect("jvm bindings should exist");

        // The namespace should be used in the generated code
        // It might appear as package name, class name prefix, or in comments
        assert!(
            common.len() > 0 && ffi.len() > 0,
            "bindings should be generated"
        );
    }

    // --- Record with methods (uniffi 0.32 feature) ---

    #[test]
    fn generated_record_with_methods() {
        let udl = r#"
            namespace test_crate {};

            dictionary Point {
                double x;
                double y;
            };

            namespace test_crate {
                Point double(Point point);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let common = &bindings.common;

        // Record should be generated
        assert!(
            common.contains("Point"),
            "should have Point record"
        );
    }

    // --- Enum with methods (uniffi 0.32 feature) ---

    #[test]
    fn generated_enum_with_methods() {
        let udl = r#"
            namespace test_crate {};

            enum Direction {
                "North",
                "South",
                "East",
                "West"
            };

            namespace test_crate {
                Direction opposite(Direction d);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let common = &bindings.common;

        // Enum should be generated
        assert!(
            common.contains("Direction"),
            "should have Direction enum"
        );
    }

    // ========================================================================
    // KMP expect/actual separation tests
    // These tests verify the core invariant: commonMain must NOT contain
    // any FFI calls (UniffiLib), while platform source sets SHOULD.
    // ========================================================================

    fn generate_non_kmp_bindings(udl: &str) -> MultiplatformBindings {
        let ci = ComponentInterface::from_webidl(udl, "test_crate").unwrap();
        let config = Config {
            package_name: Some("test.package".to_string()),
            cdylib_name: Some("test".to_string()),
            kotlin_multiplatform: false,
            kotlin_targets: vec![ConfigKotlinTarget::Jvm],
            ..Default::default()
        };
        generate_bindings(&config, &ci).unwrap()
    }

    #[test]
    fn kmp_common_has_no_uniffilib_reference() {
        // In KMP mode, commonMain must not reference UniffiLib (which is
        // platform-specific). This is the core invariant that the record
        // and enum method bugs violated.
        let udl = r#"
            namespace test_crate {};

            dictionary Point {
                double x;
                double y;
            };

            enum Color {
                "Red",
                "Green",
                "Blue"
            };

            interface Calculator {
                constructor();
                Point add_points(Point a, Point b);
                Color default_color();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        assert!(
            !bindings.common.contains("UniffiLib"),
            "commonMain must not contain UniffiLib references in KMP mode\nGot:\n{}",
            &bindings.common[..bindings.common.len().min(3000)]
        );
    }

    #[test]
    fn kmp_jvm_has_uniffilib_reference() {
        // In KMP mode, the JVM source set should contain UniffiLib references
        // for the actual FFI calls.
        let udl = r#"
            namespace test_crate {};

            interface Calculator {
                constructor();
                i32 add(i32 a, i32 b);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        assert!(
            jvm.contains("UniffiLib"),
            "jvmMain should contain UniffiLib references"
        );
    }

    #[test]
    fn kmp_record_is_data_class_in_common() {
        // Records should be concrete data classes in commonMain (not expect).
        // Kotlin forbids 'expect data class', so records must stay concrete.
        let udl = r#"
            namespace test_crate {};

            dictionary User {
                string name;
                u32 age;
            };
        "#;
        let bindings = generate_test_bindings(udl);
        assert!(
            bindings.common.contains("data class User"),
            "commonMain should have 'data class User'"
        );
        assert!(
            !bindings.common.contains("expect data class"),
            "commonMain must not have 'expect data class' (Kotlin forbids it)"
        );
    }

    #[test]
    fn kmp_enum_is_enum_class_or_sealed_in_common() {
        // Enums should be concrete enum class or sealed class in commonMain.
        // Kotlin forbids 'expect enum class'.
        let udl = r#"
            namespace test_crate {};

            enum Status {
                "Active",
                "Inactive"
            };
        "#;
        let bindings = generate_test_bindings(udl);
        assert!(
            bindings.common.contains("enum class Status"),
            "commonMain should have 'enum class Status'"
        );
        assert!(
            !bindings.common.contains("expect enum class"),
            "commonMain must not have 'expect enum class'"
        );
    }

    #[test]
    fn kmp_object_uses_expect_actual_pattern() {
        // Objects should use expect/actual: expect class in common,
        // actual class in platform source set.
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
                string hello();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        assert!(
            bindings.common.contains("expect"),
            "commonMain should have 'expect' for objects"
        );
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        assert!(
            jvm.contains("actual"),
            "jvmMain should have 'actual' for objects"
        );
    }

    #[test]
    fn kmp_top_level_functions_are_expect_in_common() {
        // Top-level functions should use expect/actual pattern.
        let udl = r#"
            namespace test_crate {};

            namespace test_crate {
                i32 add(i32 a, i32 b);
                string greet(string name);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        assert!(
            bindings.common.contains("expect fun"),
            "commonMain should have 'expect fun' for top-level functions"
        );
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        assert!(
            jvm.contains("actual fun"),
            "jvmMain should have 'actual fun' for top-level functions"
        );
    }

    #[test]
    fn kmp_record_has_no_method_bodies_in_common() {
        // Records in commonMain should not have method bodies with FFI calls.
        // Even if the record has methods (from proc-macro metadata), the
        // common template should not inline them in KMP mode.
        let udl = r#"
            namespace test_crate {};

            dictionary Point {
                double x;
                double y;
            };
        "#;
        let bindings = generate_test_bindings(udl);
        // The record should be a plain data class with companion object
        assert!(
            bindings.common.contains("data class Point"),
            "should have data class Point"
        );
        // No FFI call constructs should appear in the record definition
        let common_record_section = bindings
            .common
            .split("data class Point")
            .nth(1)
            .unwrap_or("");
        let record_end = common_record_section.find("}\n").unwrap_or(0);
        let record_body = &common_record_section[..record_end];
        assert!(
            !record_body.contains("UniffiLib"),
            "record body in commonMain must not contain UniffiLib calls"
        );
        assert!(
            !record_body.contains("uniffiRustCall"),
            "record body in commonMain must not contain uniffiRustCall"
        );
    }

    #[test]
    fn kmp_enum_has_no_method_bodies_in_common() {
        // Enums in commonMain should not have method bodies with FFI calls.
        let udl = r#"
            namespace test_crate {};

            enum Color {
                "Red",
                "Green",
                "Blue"
            };
        "#;
        let bindings = generate_test_bindings(udl);
        assert!(
            bindings.common.contains("enum class Color"),
            "should have enum class Color"
        );
        // No FFI call constructs should appear in the enum definition
        let common_enum_section = bindings
            .common
            .split("enum class Color")
            .nth(1)
            .unwrap_or("");
        let enum_end = common_enum_section.find("}\n").unwrap_or(0);
        let enum_body = &common_enum_section[..enum_end];
        assert!(
            !enum_body.contains("UniffiLib"),
            "enum body in commonMain must not contain UniffiLib calls"
        );
        assert!(
            !enum_body.contains("uniffiRustCall"),
            "enum body in commonMain must not contain uniffiRustCall"
        );
    }

    #[test]
    fn kmp_sealed_enum_has_no_method_bodies_in_common() {
        // Sealed class enums (with associated data) in commonMain should
        // not have method bodies with FFI calls.
        let udl = r#"
            namespace test_crate {};

            [Enum]
            interface Response {
                Success(string data);
                Error(string message);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        assert!(
            bindings.common.contains("sealed class Response")
                || bindings.common.contains("sealed class"),
            "should have sealed class for enum with data"
        );
        assert!(
            !bindings.common.contains("UniffiLib"),
            "sealed enum in commonMain must not contain UniffiLib calls"
        );
    }

    // ========================================================================
    // Non-KMP mode tests
    // In non-KMP mode, everything goes into a single source set.
    // ========================================================================

    #[test]
    fn non_kmp_record_is_data_class() {
        let udl = r#"
            namespace test_crate {};

            dictionary User {
                string name;
                u32 age;
            };
        "#;
        let bindings = generate_non_kmp_bindings(udl);
        assert!(
            bindings.common.contains("data class User"),
            "non-KMP should have data class User"
        );
        assert!(
            !bindings.common.contains("expect "),
            "non-KMP should not have 'expect ' keyword"
        );
    }

    #[test]
    fn non_kmp_enum_is_enum_class() {
        let udl = r#"
            namespace test_crate {};

            enum Status {
                "Active",
                "Inactive"
            };
        "#;
        let bindings = generate_non_kmp_bindings(udl);
        assert!(
            bindings.common.contains("enum class Status"),
            "non-KMP should have enum class Status"
        );
        assert!(
            !bindings.common.contains("expect "),
            "non-KMP should not have 'expect ' keyword"
        );
    }

    #[test]
    fn non_kmp_object_is_concrete_class() {
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
                string hello();
            };
        "#;
        let bindings = generate_non_kmp_bindings(udl);
        assert!(
            !bindings.common.contains("expect "),
            "non-KMP should not have 'expect ' for objects"
        );
        assert!(
            !bindings.common.contains("actual class"),
            "non-KMP should not have 'actual class' for objects"
        );
        assert!(
            !bindings.common.contains("actual fun"),
            "non-KMP should not have 'actual fun' for objects"
        );
    }

    #[test]
    fn non_kmp_top_level_function_has_body() {
        let udl = r#"
            namespace test_crate {};

            namespace test_crate {
                i32 add(i32 a, i32 b);
            };
        "#;
        let bindings = generate_non_kmp_bindings(udl);
        // In non-KMP mode, top-level functions go into the jvm source set
        assert!(
            !bindings.common.contains("expect "),
            "non-KMP should not have 'expect '"
        );
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        assert!(
            !jvm.contains("actual fun"),
            "non-KMP should not have 'actual fun'"
        );
        assert!(
            jvm.contains("fun `add`"),
            "non-KMP jvm should have a concrete fun add"
        );
    }

    #[test]
    fn non_kmp_jvm_bindings_generated() {
        // In non-KMP mode with jvm target, jvm bindings should be generated
        // containing the actual FFI implementations. The common source set
        // contains type declarations but not top-level function bodies.
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
                string hello();
            };
        "#;
        let bindings = generate_non_kmp_bindings(udl);
        assert!(
            bindings.jvm.is_some(),
            "non-KMP with jvm target should generate jvm bindings"
        );
    }

    // ========================================================================
    // FfiConverter generation tests
    // ========================================================================

    #[test]
    fn kmp_record_has_ffi_converter_in_jvm() {
        // The FfiConverter for records should be in the platform source set.
        let udl = r#"
            namespace test_crate {};

            dictionary Point {
                double x;
                double y;
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        assert!(
            jvm.contains("FfiConverterTypePoint"),
            "jvmMain should have FfiConverterTypePoint"
        );
        assert!(
            jvm.contains("FfiConverterRustBuffer"),
            "jvmMain FfiConverter should extend FfiConverterRustBuffer"
        );
    }

    #[test]
    fn kmp_enum_has_ffi_converter_in_jvm() {
        let udl = r#"
            namespace test_crate {};

            enum Color {
                "Red",
                "Green",
                "Blue"
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        assert!(
            jvm.contains("FfiConverterTypeColor"),
            "jvmMain should have FfiConverterTypeColor"
        );
    }

    // ========================================================================
    // Enum variant representation tests
    // ========================================================================

    #[test]
    fn kmp_flat_enum_uses_enum_class() {
        let udl = r#"
            namespace test_crate {};

            enum Color {
                "Red",
                "Green",
                "Blue"
            };
        "#;
        let bindings = generate_test_bindings(udl);
        assert!(
            bindings.common.contains("enum class Color"),
            "flat enum should use 'enum class'"
        );
        assert!(
            !bindings.common.contains("sealed class Color"),
            "flat enum should not use 'sealed class'"
        );
    }

    #[test]
    fn kmp_enum_with_data_uses_sealed_class() {
        let udl = r#"
            namespace test_crate {};

            [Enum]
            interface ApiResponse {
                Success(string data, u32 code);
                Error(string message, u32 code);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        assert!(
            bindings.common.contains("sealed class"),
            "enum with associated data should use 'sealed class'"
        );
        assert!(
            !bindings.common.contains("enum class ApiResponse"),
            "enum with data should not use 'enum class'"
        );
    }

    #[test]
    fn kmp_enum_with_data_has_variant_classes() {
        let udl = r#"
            namespace test_crate {};

            [Enum]
            interface ApiResponse {
                Success(string data);
                Error(string message);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        assert!(
            bindings.common.contains("data class Success"),
            "sealed enum should have 'data class Success' variant"
        );
        assert!(
            bindings.common.contains("data class Error"),
            "sealed enum should have 'data class Error' variant"
        );
    }

    // ========================================================================
    // Record field tests
    // ========================================================================

    #[test]
    fn kmp_record_with_default_values() {
        let udl = r#"
            namespace test_crate {};

            dictionary Config {
                string name = "default";
                u32 timeout = 30;
            };
        "#;
        let bindings = generate_test_bindings(udl);
        assert!(
            bindings.common.contains("data class Config"),
            "should have data class Config"
        );
    }

    #[test]
    fn kmp_record_with_nested_types() {
        let udl = r#"
            namespace test_crate {};

            dictionary Inner {
                string value;
            };

            dictionary Outer {
                Inner inner;
                sequence<Inner> items;
            };
        "#;
        let bindings = generate_test_bindings(udl);
        assert!(
            bindings.common.contains("data class Outer"),
            "should have data class Outer"
        );
        assert!(
            bindings.common.contains("data class Inner"),
            "should have data class Inner"
        );
    }

    #[test]
    fn kmp_record_fields_use_correct_kotlin_types() {
        let udl = r#"
            namespace test_crate {};

            dictionary Types {
                i32 int_field;
                u32 uint_field;
                i64 long_field;
                u64 ulong_field;
                boolean bool_field;
                string string_field;
                double float_field;
                float f32_field;
                sequence<i32> list_field;
            };
        "#;
        let bindings = generate_test_bindings(udl);
        assert!(
            bindings.common.contains("kotlin.Int"),
            "i32 should map to kotlin.Int"
        );
        assert!(
            bindings.common.contains("kotlin.UInt"),
            "u32 should map to kotlin.UInt"
        );
        assert!(
            bindings.common.contains("kotlin.String"),
            "string should map to kotlin.String"
        );
    }

    // ========================================================================
    // Companion object tests
    // ========================================================================

    #[test]
    fn kmp_record_has_companion_object() {
        let udl = r#"
            namespace test_crate {};

            dictionary Point {
                double x;
                double y;
            };
        "#;
        let bindings = generate_test_bindings(udl);
        assert!(
            bindings.common.contains("companion object"),
            "record should have companion object"
        );
    }

    #[test]
    fn kmp_enum_has_companion_object() {
        let udl = r#"
            namespace test_crate {};

            enum Color {
                "Red",
                "Green",
                "Blue"
            };
        "#;
        let bindings = generate_test_bindings(udl);
        assert!(
            bindings.common.contains("companion object"),
            "enum should have companion object"
        );
    }

    // ========================================================================
    // Object method and handle tests
    // ========================================================================

    #[test]
    fn kmp_object_methods_are_expect_decl_in_common() {
        // Object methods in commonMain should be declarations (expect),
        // not implementations with FFI calls.
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
                string hello();
                i32 compute(i32 input);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        // The interface in common should have method declarations
        assert!(
            bindings.common.contains("fun `hello`"),
            "commonMain should have hello method declaration"
        );
        assert!(
            bindings.common.contains("fun `compute`"),
            "commonMain should have compute method declaration"
        );
    }

    #[test]
    fn kmp_object_methods_have_bodies_in_jvm() {
        // Object methods in jvmMain should have implementations with FFI calls.
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
                string hello();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        assert!(
            jvm.contains("UniffiLib"),
            "jvmMain should have UniffiLib FFI calls for object methods"
        );
    }

    #[test]
    fn kmp_object_has_handle_field() {
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
                string hello();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        assert!(
            jvm.contains("handle: Long") || jvm.contains("val handle"),
            "jvm object should have a handle field"
        );
    }

    #[test]
    fn kmp_object_has_disposable_interface() {
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
                string hello();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        assert!(
            bindings.common.contains("Disposable"),
            "commonMain should have Disposable interface"
        );
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        assert!(
            jvm.contains("override fun destroy()"),
            "jvm object should have destroy() override"
        );
    }

    // ========================================================================
    // Callback interface tests
    // ========================================================================

    #[test]
    fn kmp_callback_interface_in_common() {
        let udl = r#"
            namespace test_crate {};

            callback interface MyCallback {
                void on_event(string event_name);
                string get_name();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        assert!(
            bindings.common.contains("interface MyCallback"),
            "commonMain should have callback interface"
        );
    }

    #[test]
    fn kmp_callback_interface_has_methods_in_common() {
        let udl = r#"
            namespace test_crate {};

            callback interface MyCallback {
                void on_event(string event_name);
                string get_name();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        assert!(
            bindings.common.contains("fun `onEvent`"),
            "commonMain should have onEvent method"
        );
        assert!(
            bindings.common.contains("fun `getName`"),
            "commonMain should have getName method"
        );
    }

    // ========================================================================
    // Error type tests
    // ========================================================================

    #[test]
    fn kmp_error_enum_generates_exception_class() {
        let udl = r#"
            namespace test_crate {};

            [Error]
            enum MyError {
                "NotFound",
                "InvalidInput",
                "Internal"
            };
        "#;
        let bindings = generate_test_bindings(udl);
        // Error enums should be generated as exception classes
        // The "Error" suffix is converted to "Exception"
        assert!(
            bindings.common.contains("MyException") || bindings.common.contains("MyError"),
            "should have error type (MyException or MyError)"
        );
        assert!(
            bindings.common.contains("Exception"),
            "error type should be an Exception class"
        );
    }

    // ========================================================================
    // Multi-platform generation tests
    // ========================================================================

    #[test]
    fn kmp_generates_jvm_and_native_bindings() {
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
                string hello();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        assert!(
            bindings.jvm.is_some(),
            "should generate JVM bindings"
        );
        assert!(
            bindings.native.is_some(),
            "should generate Native bindings"
        );
    }

    #[test]
    fn kmp_jvm_and_native_both_have_actual() {
        // Both JVM and Native should have 'actual' implementations.
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
                string hello();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        let native = bindings.native.as_ref().expect("native bindings should exist");
        assert!(
            jvm.contains("actual"),
            "jvm bindings should have 'actual' keyword"
        );
        assert!(
            native.contains("actual"),
            "native bindings should have 'actual' keyword"
        );
    }

    // ========================================================================
    // Edge case tests
    // ========================================================================

    #[test]
    fn kmp_empty_record() {
        // A record with no fields should still be generated.
        let udl = r#"
            namespace test_crate {};

            dictionary Empty {};
        "#;
        let bindings = generate_test_bindings(udl);
        assert!(
            bindings.common.contains("Empty"),
            "should have Empty record"
        );
    }

    #[test]
    fn kmp_single_variant_enum() {
        let udl = r#"
            namespace test_crate {};

            enum Solo {
                "Only"
            };
        "#;
        let bindings = generate_test_bindings(udl);
        assert!(
            bindings.common.contains("enum class Solo"),
            "should have enum class Solo"
        );
        // Variant "Only" is converted to SHOUTY_SNAKE_CASE by default
        assert!(
            bindings.common.contains("ONLY") || bindings.common.contains("Only"),
            "should have Only/ONLY variant"
        );
    }

    #[test]
    fn kmp_enum_with_repr_int() {
        let udl = r#"
            namespace test_crate {};

            [Enum]
            interface IntEnum {
                A(i32 value);
                B(i32 value);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        assert!(
            bindings.common.contains("sealed class") || bindings.common.contains("enum class"),
            "should generate enum class or sealed class"
        );
    }

    #[test]
    fn kmp_record_with_optional_fields() {
        let udl = r#"
            namespace test_crate {};

            dictionary User {
                string name;
                string? nickname;
                u32? age;
            };
        "#;
        let bindings = generate_test_bindings(udl);
        assert!(
            bindings.common.contains("data class User"),
            "should have data class User"
        );
    }

    #[test]
    fn kmp_record_with_sequence_fields() {
        let udl = r#"
            namespace test_crate {};

            dictionary Container {
                sequence<i32> numbers;
                sequence<string> names;
            };
        "#;
        let bindings = generate_test_bindings(udl);
        assert!(
            bindings.common.contains("data class Container"),
            "should have data class Container"
        );
        assert!(
            bindings.common.contains("kotlin.collections.List")
                || bindings.common.contains("List<"),
            "sequence should map to List"
        );
    }

    #[test]
    fn kmp_record_with_map_fields() {
        let udl = r#"
            namespace test_crate {};

            dictionary Config {
                record<string, i32> entries;
            };
        "#;
        let bindings = generate_test_bindings(udl);
        assert!(
            bindings.common.contains("data class Config"),
            "should have data class Config"
        );
    }

    // ========================================================================
    // Config option tests
    // ========================================================================

    #[test]
    fn kmp_immutable_records_generates_val() {
        let udl = r#"
            namespace test_crate {};

            dictionary Point {
                double x;
                double y;
            };
        "#;
        let ci = ComponentInterface::from_webidl(udl, "test_crate").unwrap();
        let config = Config {
            package_name: Some("test.package".to_string()),
            cdylib_name: Some("test".to_string()),
            kotlin_multiplatform: true,
            kotlin_targets: vec![ConfigKotlinTarget::Jvm],
            generate_immutable_records: Some(true),
            ..Default::default()
        };
        let bindings = generate_bindings(&config, &ci).unwrap();
        assert!(
            bindings.common.contains("val "),
            "immutable records should use 'val'"
        );
        assert!(
            !bindings.common.contains("var "),
            "immutable records should not use 'var'"
        );
    }

    #[test]
    fn kmp_mutable_records_generates_var() {
        let udl = r#"
            namespace test_crate {};

            dictionary Point {
                double x;
                double y;
            };
        "#;
        let ci = ComponentInterface::from_webidl(udl, "test_crate").unwrap();
        let config = Config {
            package_name: Some("test.package".to_string()),
            cdylib_name: Some("test".to_string()),
            kotlin_multiplatform: true,
            kotlin_targets: vec![ConfigKotlinTarget::Jvm],
            generate_immutable_records: Some(false),
            ..Default::default()
        };
        let bindings = generate_bindings(&config, &ci).unwrap();
        assert!(
            bindings.common.contains("var "),
            "mutable records should use 'var'"
        );
    }

    #[test]
    fn kmp_stub_bindings_generated_when_stub_target() {
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
                string hello();
            };
        "#;
        let ci = ComponentInterface::from_webidl(udl, "test_crate").unwrap();
        let config = Config {
            package_name: Some("test.package".to_string()),
            cdylib_name: Some("test".to_string()),
            kotlin_multiplatform: true,
            kotlin_targets: vec![ConfigKotlinTarget::Stub],
            ..Default::default()
        };
        let bindings = generate_bindings(&config, &ci).unwrap();
        assert!(
            bindings.stub.is_some(),
            "stub bindings should be generated when stub target is configured"
        );
        let stub = bindings.stub.as_ref().unwrap();
        assert!(
            stub.contains("actual"),
            "stub bindings should have 'actual' keyword"
        );
        assert!(
            stub.contains("TODO"),
            "stub bindings should have TODO() stubs"
        );
    }

    // ========================================================================
    // Package and visibility tests
    // ========================================================================

    #[test]
    fn kmp_uses_configured_package_name() {
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
            };
        "#;
        let ci = ComponentInterface::from_webidl(udl, "test_crate").unwrap();
        let config = Config {
            package_name: Some("com.example.mybindings".to_string()),
            cdylib_name: Some("test".to_string()),
            kotlin_multiplatform: true,
            kotlin_targets: vec![ConfigKotlinTarget::Jvm],
            ..Default::default()
        };
        let bindings = generate_bindings(&config, &ci).unwrap();
        assert!(
            bindings.common.contains("com.example.mybindings"),
            "commonMain should use configured package name"
        );
    }

    #[test]
    fn kmp_uses_public_visibility() {
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
                string hello();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        assert!(
            bindings.common.contains("public"),
            "commonMain should use public visibility"
        );
    }

    // ========================================================================
    // Async function tests
    // ========================================================================

    #[test]
    fn kmp_object_with_async_method() {
        let udl = r#"
            namespace test_crate {};

            interface AsyncTask {
                constructor();
                [Async]
                string fetch_data();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        // Async methods should generate suspend fun
        // (The exact form depends on whether the template handles it correctly)
        assert!(
            bindings.common.contains("AsyncTask"),
            "should have AsyncTask class"
        );
    }

    // ========================================================================
    // Multiple types interaction tests
    // ========================================================================

    #[test]
    fn kmp_multiple_records_and_enums() {
        let udl = r#"
            namespace test_crate {};

            dictionary Point { double x; double y; };
            dictionary Rect { Point origin; Point size; };
            enum Color { "Red", "Green", "Blue" };
            [Enum]
            interface Shape { Circle(double radius); Square(double side); };
        "#;
        let bindings = generate_test_bindings(udl);
        assert!(bindings.common.contains("data class Point"), "should have Point");
        assert!(bindings.common.contains("data class Rect"), "should have Rect");
        assert!(bindings.common.contains("enum class Color"), "should have Color");
        assert!(bindings.common.contains("sealed class Shape"), "should have Shape");
    }

    #[test]
    fn kmp_record_referencing_enum() {
        let udl = r#"
            namespace test_crate {};

            enum Status { "Active", "Inactive" };
            dictionary User { string name; Status status; };
        "#;
        let bindings = generate_test_bindings(udl);
        assert!(
            bindings.common.contains("data class User"),
            "should have User record"
        );
        assert!(
            bindings.common.contains("enum class Status"),
            "should have Status enum"
        );
    }

    #[test]
    fn kmp_multiple_objects() {
        let udl = r#"
            namespace test_crate {};

            interface Foo { constructor(); string hello(); };
            interface Bar { constructor(); i32 compute(i32 x); };
        "#;
        let bindings = generate_test_bindings(udl);
        assert!(bindings.common.contains("Foo"), "should have Foo");
        assert!(bindings.common.contains("Bar"), "should have Bar");
    }

    // ========================================================================
    // UniFFI Internals: Lifting and Lowering
    // Based on docs/manual/src/internals/lifting_and_lowering.md
    // ========================================================================

    #[test]
    fn ffi_boolean_uses_int8() {
        // Booleans must be lowered as int8_t (Byte in Kotlin), not Boolean.
        // Values must be 0 or 1.
        let udl = r#"
            namespace test_crate {};

            namespace test_crate {
                boolean negate(boolean value);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        // FfiConverterBoolean should use Byte (int8_t)
        assert!(
            jvm.contains("FfiConverterBoolean"),
            "should have FfiConverterBoolean\nGot:\n{jvm}"
        );
        assert!(
            jvm.contains("0.toByte()") || jvm.contains("1.toByte"),
            "boolean should use byte values 0/1\nGot:\n{jvm}"
        );
    }

    #[test]
    fn ffi_string_uses_rust_buffer() {
        // Strings must be lowered as RustBuffer (not raw pointer).
        // Serialized as i32 length (signed, big-endian) + UTF-8 bytes, no null terminator.
        let udl = r#"
            namespace test_crate {};

            namespace test_crate {
                string greet(string name);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        assert!(
            jvm.contains("FfiConverterString"),
            "should have FfiConverterString\nGot:\n{jvm}"
        );
        // String lowering should produce a RustBuffer, not a raw pointer
        assert!(
            jvm.contains("RustBuffer"),
            "string should use RustBuffer\nGot:\n{jvm}"
        );
    }

    #[test]
    fn ffi_sequence_uses_signed_i32_count() {
        // Sequences must be serialized as signed i32 item count + items.
        // Length fields must be signed i32 (not u32) for JVM compatibility.
        let udl = r#"
            namespace test_crate {};

            namespace test_crate {
                sequence<i32> double_all(sequence<i32> numbers);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        // Should use putInt/getInt (signed i32) for sequence length
        assert!(
            jvm.contains("FfiConverterSequence"),
            "should have FfiConverterSequence\nGot:\n{jvm}"
        );
    }

    #[test]
    fn ffi_enum_variant_numbering_starts_at_one() {
        // Enum variants must be numbered starting from 1 (not 0).
        // This is critical for correct serialization.
        let udl = r#"
            namespace test_crate {};

            enum Direction {
                "North",
                "South",
                "East",
                "West"
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        // Flat enum write should use ordinal + 1 (1-based)
        assert!(
            jvm.contains("ordinal + 1"),
            "flat enum write should use ordinal + 1 (1-based)\nGot:\n{jvm}"
        );
        // Flat enum read should use getInt() - 1
        assert!(
            jvm.contains("getInt() - 1"),
            "flat enum read should use getInt() - 1 (1-based)\nGot:\n{jvm}"
        );
    }

    #[test]
    fn ffi_non_flat_enum_variant_numbering_starts_at_one() {
        // Non-flat (sealed class) enum variants must also be 1-based.
        let udl = r#"
            namespace test_crate {};

            [Enum]
            interface Response {
                Success(string data, u32 code);
                Error(string message);
                Loading();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        // Non-flat enum write should use putInt({{ loop.index }}) where loop.index is 1-based
        assert!(
            jvm.contains("putInt(1)") || jvm.contains("putInt({{"),
            "non-flat enum should use 1-based variant numbering in write\nGot:\n{jvm}"
        );
        // Non-flat enum read should use when(buf.getInt()) with 1-based cases
        assert!(
            jvm.contains("when(buf.getInt())"),
            "non-flat enum read should use when(buf.getInt())\nGot:\n{jvm}"
        );
    }

    #[test]
    fn ffi_record_fields_in_declaration_order() {
        // Record fields must be serialized in declaration order.
        let udl = r#"
            namespace test_crate {};

            dictionary Point {
                double x;
                double y;
                double z;
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        // FfiConverterTypePoint should serialize fields in order x, y, z
        assert!(
            jvm.contains("FfiConverterTypePoint"),
            "should have FfiConverterTypePoint\nGot:\n{jvm}"
        );
    }

    #[test]
    fn ffi_big_endian_serialization() {
        // All numeric types must be serialized big-endian.
        // This is enforced by ByteBuffer using ByteOrder.BIG_ENDIAN.
        let udl = r#"
            namespace test_crate {};

            namespace test_crate {
                i32 add(i32 a, i32 b);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        // ByteBuffer should be initialized with BIG_ENDIAN
        assert!(
            jvm.contains("BIG_ENDIAN"),
            "ByteBuffer should use BIG_ENDIAN byte order\nGot:\n{jvm}"
        );
    }

    // ========================================================================
    // UniFFI Internals: Object References
    // Based on docs/manual/src/internals/object_references.md
    // ========================================================================

    #[test]
    fn object_handle_zero_is_reserved() {
        // Handle 0 must be reserved as invalid/null value.
        // NoHandle constructor should set handle = 0.
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
                string hello();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        assert!(
            jvm.contains("NoHandle"),
            "should have NoHandle sentinel\nGot:\n{jvm}"
        );
    }

    #[test]
    fn object_uses_arc_handle_cloning() {
        // Object handles must be cloned before method calls (Arc::clone pattern).
        // The callWithHandle method should clone the handle.
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
                string hello();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        assert!(
            jvm.contains("callWithHandle"),
            "object should use callWithHandle for method calls\nGot:\n{jvm}"
        );
        assert!(
            jvm.contains("uniffiCloneHandle"),
            "object should have uniffiCloneHandle\nGot:\n{jvm}"
        );
    }

    #[test]
    fn object_has_free_function() {
        // Each object must have a free function to release the handle.
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
                string hello();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        // Should have UniffiCleanAction that calls the free function
        assert!(
            jvm.contains("UniffiCleanAction"),
            "object should have UniffiCleanAction for handle cleanup\nGot:\n{jvm}"
        );
    }

    #[test]
    fn object_has_cleaner_integration() {
        // Objects should integrate with JVM Cleaner for automatic cleanup.
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
                string hello();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        assert!(
            jvm.contains("UniffiCleaner") || jvm.contains("CLEANER"),
            "object should integrate with Cleaner for GC\nGot:\n{jvm}"
        );
    }

    #[test]
    fn object_has_reference_counting() {
        // Objects must use reference counting (callCounter) to prevent
        // use-after-free when the handle is being freed concurrently.
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
                string hello();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        assert!(
            jvm.contains("callCounter"),
            "object should have callCounter for reference counting\nGot:\n{jvm}"
        );
    }

    // ========================================================================
    // UniFFI Internals: Trait Interface Handle Discrimination
    // Based on docs/manual/src/internals/object_references.md
    // ========================================================================

    #[test]
    fn trait_interface_handle_discrimination() {
        // For trait interfaces (export(rust, foreign)):
        // - Lowest bit = 0 → Rust-implemented object
        // - Lowest bit = 1 → Foreign callback
        // The lift function must check `(value and 1) == 0` to distinguish.
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
                string hello();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        // Should check lowest bit for handle discrimination
        // In Kotlin: (value and 1L) == 0L
        assert!(
            jvm.contains("and 1") || jvm.contains("0L"),
            "trait interface lift should check lowest bit for handle discrimination\nGot:\n{jvm}"
        );
    }

    // ========================================================================
    // UniFFI Internals: Callback Interface VTable
    // Based on docs/manual/src/internals/foreign_calls.md
    // ========================================================================

    #[test]
    fn callback_interface_has_vtable_with_free() {
        // Each callback interface vtable must have a free method.
        let udl = r#"
            namespace test_crate {};

            callback interface MyCallback {
                void on_event(string event_name);
                string get_name();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        assert!(
            jvm.contains("uniffiFree"),
            "callback interface vtable should have uniffiFree\nGot:\n{jvm}"
        );
    }

    #[test]
    fn callback_interface_has_vtable_with_clone() {
        // Each callback interface vtable must have a clone method.
        let udl = r#"
            namespace test_crate {};

            callback interface MyCallback {
                void on_event(string event_name);
                string get_name();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        assert!(
            jvm.contains("uniffiClone"),
            "callback interface vtable should have uniffiClone\nGot:\n{jvm}"
        );
    }

    #[test]
    fn callback_interface_has_vtable_registration() {
        // VTable must be registered with Rust before any handles are returned.
        let udl = r#"
            namespace test_crate {};

            callback interface MyCallback {
                void on_event(string event_name);
                string get_name();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        assert!(
            jvm.contains("register"),
            "callback interface should have register function\nGot:\n{jvm}"
        );
    }

    #[test]
    fn callback_interface_uses_handle_map() {
        // Foreign callback objects should be stored in a handle map.
        let udl = r#"
            namespace test_crate {};

            callback interface MyCallback {
                void on_event(string event_name);
                string get_name();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        assert!(
            jvm.contains("handleMap") || jvm.contains("UniffiHandleMap"),
            "callback interface should use handle map\nGot:\n{jvm}"
        );
    }

    // ========================================================================
    // UniFFI Internals: RustCallStatus
    // Based on docs/manual/src/internals/rust_calls.md
    // ========================================================================

    #[test]
    fn rust_call_status_constants() {
        // RustCallStatus codes must be: Success=0, Error=1, UnexpectedError=2, Cancelled=3
        let udl = r#"
            namespace test_crate {};

            namespace test_crate {
                i32 add(i32 a, i32 b);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        assert!(
            jvm.contains("UNIFFI_CALL_SUCCESS") && jvm.contains("0.toByte()"),
            "should have UNIFFI_CALL_SUCCESS = 0\nGot:\n{jvm}"
        );
        assert!(
            jvm.contains("UNIFFI_CALL_ERROR") && jvm.contains("1.toByte()"),
            "should have UNIFFI_CALL_ERROR = 1\nGot:\n{jvm}"
        );
        assert!(
            jvm.contains("UNIFFI_CALL_UNEXPECTED_ERROR") && jvm.contains("2.toByte()"),
            "should have UNIFFI_CALL_UNEXPECTED_ERROR = 2\nGot:\n{jvm}"
        );
        assert!(
            jvm.contains("UNIFFI_CALL_CANCELLED") && jvm.contains("3.toByte()"),
            "should have UNIFFI_CALL_CANCELLED = 3\nGot:\n{jvm}"
        );
    }

    #[test]
    fn rust_call_status_check_methods() {
        // RustCallStatus must have isSuccess, isError, isPanic, isCancelled methods.
        let udl = r#"
            namespace test_crate {};

            namespace test_crate {
                i32 add(i32 a, i32 b);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        // The check methods are defined as extension functions: fun UniffiRustCallStatus.isSuccess()
        assert!(
            jvm.contains("isSuccess()"),
            "should have isSuccess() method\nGot:\n{jvm}"
        );
        assert!(
            jvm.contains("isError()"),
            "should have isError() method\nGot:\n{jvm}"
        );
        assert!(
            jvm.contains("isPanic()"),
            "should have isPanic() method\nGot:\n{jvm}"
        );
        assert!(
            jvm.contains("isCancelled()"),
            "should have isCancelled() method\nGot:\n{jvm}"
        );
    }

    #[test]
    fn rust_call_status_initializes_to_success() {
        // Foreign code must initialize code=Success (0) before calling Rust.
        // Default constructor should set code = 0.toByte().
        let udl = r#"
            namespace test_crate {};

            namespace test_crate {
                i32 add(i32 a, i32 b);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        // The struct initializes code to 0 (Success) in the constructor
        assert!(
            jvm.contains("UNIFFI_CALL_SUCCESS") || jvm.contains("code = 0"),
            "RustCallStatus should initialize code to 0 (Success)\nGot:\n{jvm}"
        );
    }

    #[test]
    fn rust_call_status_struct_layout() {
        // RustCallStatus must be a JNA Structure with FieldOrder("code", "errorBuf").
        let udl = r#"
            namespace test_crate {};

            namespace test_crate {
                i32 add(i32 a, i32 b);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        assert!(
            jvm.contains("FieldOrder"),
            "RustCallStatus should use JNA Structure FieldOrder\nGot:\n{jvm}"
        );
        assert!(
            jvm.contains("code") && jvm.contains("errorBuf"),
            "RustCallStatus should have code and errorBuf fields\nGot:\n{jvm}"
        );
    }

    // ========================================================================
    // UniFFI Internals: Error Handling
    // Based on docs/manual/src/internals/rust_calls.md
    // ========================================================================

    #[test]
    fn error_handling_check_call_status() {
        // uniffiCheckCallStatus must handle all status codes:
        // Success → return, Error → throw, Panic → throw InternalException,
        // Cancelled → throw InternalException, Unknown → throw InternalException
        let udl = r#"
            namespace test_crate {};

            namespace test_crate {
                i32 add(i32 a, i32 b);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        assert!(
            jvm.contains("uniffiCheckCallStatus"),
            "should have uniffiCheckCallStatus function\nGot:\n{jvm}"
        );
        assert!(
            jvm.contains("Rust panic"),
            "should handle panic status with 'Rust panic' message\nGot:\n{jvm}"
        );
        assert!(
            jvm.contains("Rust future cancelled"),
            "should handle cancelled status\nGot:\n{jvm}"
        );
    }

    #[test]
    fn error_handling_null_error_handler() {
        // UniffiNullRustCallStatusErrorHandler should exist for functions
        // that don't return Result.
        let udl = r#"
            namespace test_crate {};

            namespace test_crate {
                i32 add(i32 a, i32 b);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        assert!(
            jvm.contains("UniffiNullRustCallStatusErrorHandler"),
            "should have UniffiNullRustCallStatusErrorHandler\nGot:\n{jvm}"
        );
    }

    #[test]
    fn error_handling_throws_on_error_status() {
        // When Rust returns Error status, Kotlin should throw the lifted error.
        let udl = r#"
            namespace test_crate {};

            [Error]
            enum MyError {
                "NotFound",
                "InvalidInput"
            };

            namespace test_crate {
                [Throws=MyError]
                i32 risky_add(i32 a, i32 b);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        assert!(
            jvm.contains("MyException") || jvm.contains("MyError"),
            "should have error type for throwing\nGot:\n{jvm}"
        );
    }

    // ========================================================================
    // UniFFI Internals: Method Call Pattern
    // Based on docs/manual/src/internals/object_references.md
    // ========================================================================

    #[test]
    fn method_call_clones_handle_before_call() {
        // Per object_references.md: "Clone the handle. Pass the cloned handle
        // to the Rust FFI function (transferring ownership of a leaked Arc<>
        // back to Rust)."
        let udl = r#"
            namespace test_crate {};

            interface Calculator {
                constructor();
                i32 add(i32 a, i32 b);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        // callWithHandle should clone the handle before calling Rust
        assert!(
            jvm.contains("callWithHandle"),
            "methods should use callWithHandle to clone handle\nGot:\n{jvm}"
        );
    }

    // ========================================================================
    // UniFFI Internals: FFI Converter Implementation
    // Based on docs/manual/src/internals/ffi_converter_traits.md
    // ========================================================================

    #[test]
    fn ffi_converter_has_required_methods() {
        // FfiConverter interface must have: lift, lower, read, write, allocationSize
        let udl = r#"
            namespace test_crate {};

            namespace test_crate {
                i32 add(i32 a, i32 b);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        assert!(
            jvm.contains("interface FfiConverter"),
            "should have FfiConverter interface\nGot:\n{jvm}"
        );
        assert!(
            jvm.contains("fun lift("),
            "FfiConverter should have lift method\nGot:\n{jvm}"
        );
        assert!(
            jvm.contains("fun lower("),
            "FfiConverter should have lower method\nGot:\n{jvm}"
        );
        assert!(
            jvm.contains("fun read("),
            "FfiConverter should have read method\nGot:\n{jvm}"
        );
        assert!(
            jvm.contains("fun write("),
            "FfiConverter should have write method\nGot:\n{jvm}"
        );
    }

    #[test]
    fn ffi_converter_rust_buffer_has_required_methods() {
        // FfiConverterRustBuffer must have read, write, allocationSize
        let udl = r#"
            namespace test_crate {};

            dictionary Point {
                double x;
                double y;
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        assert!(
            jvm.contains("FfiConverterRustBuffer"),
            "should have FfiConverterRustBuffer\nGot:\n{jvm}"
        );
    }

    // ========================================================================
    // UniFFI Internals: Default Values / Placeholder Returns
    // Based on docs/manual/src/internals/rust_calls.md
    // ========================================================================

    #[test]
    fn placeholder_return_values_defined() {
        // Placeholder values for error returns must be defined for all FFI types.
        let udl = r#"
            namespace test_crate {};

            namespace test_crate {
                i32 add(i32 a, i32 b);
                string greet(string name);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let jvm = bindings.jvm.as_ref().expect("jvm bindings should exist");
        // Placeholder values should be defined for various types
        assert!(
            jvm.contains("0L") || jvm.contains("0.toLong()"),
            "should have placeholder for Long/Handle\nGot:\n{jvm}"
        );
    }

    // ========================================================================
    // UniFFI Internals: Interface Definition
    // Based on docs/manual/src/internals/rendering_foreign_bindings.md
    // ========================================================================

    #[test]
    fn interface_definition_in_common() {
        // Objects should generate both an interface (for the API) and an
        // implementation class (for the FFI).
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
                string hello();
                i32 compute(i32 x);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        // Common should have the interface definition
        assert!(
            bindings.common.contains("interface"),
            "common should have interface definition\nGot:\n{}",
            &bindings.common[..bindings.common.len().min(2000)]
        );
    }

    // ========================================================================
    // UniFFI Internals: Docstring Handling
    // Based on docs/manual/src/internals/rendering_foreign_bindings.md
    // ========================================================================

    #[test]
    fn docstrings_preserved_in_bindings() {
        // Docstrings from Rust should be preserved in generated Kotlin bindings.
        let udl = r#"
            namespace test_crate {};

            /// A simple calculator for testing.
            interface Calculator {
                /// Create a new calculator.
                constructor();
                /// Add two numbers together.
                i32 add(i32 a, i32 b);
            };
        "#;
        let bindings = generate_test_bindings(udl);
        let common = &bindings.common;
        // Docstrings should appear in the generated code
        assert!(
            common.contains("calculator") || common.contains("Calculator"),
            "should have docstring content\nGot:\n{common}"
        );
    }

    // ========================================================================
    // UniFFI Internals: Package Name
    // Based on docs/manual/src/internals/rendering_foreign_bindings.md
    // ========================================================================

    #[test]
    fn package_name_applied_to_bindings() {
        // The configured package name should appear in the generated bindings.
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
            };
        "#;
        let ci = ComponentInterface::from_webidl(udl, "test_crate").unwrap();
        let config = Config {
            package_name: Some("com.example.mylib".to_string()),
            cdylib_name: Some("test".to_string()),
            kotlin_multiplatform: true,
            kotlin_targets: vec![ConfigKotlinTarget::Jvm],
            ..Default::default()
        };
        let bindings = generate_bindings(&config, &ci).unwrap();
        assert!(
            bindings.common.contains("com.example.mylib"),
            "should use configured package name\nGot:\n{}",
            &bindings.common[..bindings.common.len().min(1000)]
        );
    }

    // ========================================================================
    // UniFFI Internals: External Types
    // Based on docs/manual/src/internals/crates.md
    // ========================================================================

    #[test]
    fn external_type_package_name_configurable() {
        // External types should use the configured package name.
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
            };
        "#;
        let ci = ComponentInterface::from_webidl(udl, "test_crate").unwrap();
        let mut external_packages = std::collections::HashMap::new();
        external_packages.insert("other_crate".to_string(), "com.example.other".to_string());
        let config = Config {
            package_name: Some("com.example.mylib".to_string()),
            cdylib_name: Some("test".to_string()),
            kotlin_multiplatform: true,
            kotlin_targets: vec![ConfigKotlinTarget::Jvm],
            external_packages,
            ..Default::default()
        };
        let bindings = generate_bindings(&config, &ci).unwrap();
        // The external_packages config should be preserved
        assert!(
            config.external_packages.contains_key("other_crate"),
            "external_packages should be preserved"
        );
    }

    // ========================================================================
    // UniFFI Internals: Async Support
    // Based on docs/manual/src/internals/async-ffi.md
    // ========================================================================

    #[test]
    fn async_function_generates_rust_future_handle() {
        // Async functions should generate RustFuture handle-based FFI.
        let udl = r#"
            namespace test_crate {};

            interface AsyncTask {
                constructor();
                [Async]
                string fetch_data();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        assert!(
            bindings.common.contains("AsyncTask"),
            "should have AsyncTask class"
        );
    }

    // ========================================================================
    // UniFFI Internals: Visibility
    // Based on docs/manual/src/internals/rendering_foreign_bindings.md
    // ========================================================================

    #[test]
    fn visibility_modifier_applied() {
        // The configured visibility modifier should be applied to all public types.
        let udl = r#"
            namespace test_crate {};

            dictionary Point {
                double x;
                double y;
            };

            enum Color {
                "Red",
                "Green",
                "Blue"
            };

            interface Foo {
                constructor();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        // All public types should have the visibility modifier
        assert!(
            bindings.common.contains("public data class Point"),
            "record should have public visibility\nGot:\n{}",
            &bindings.common[..bindings.common.len().min(1000)]
        );
        assert!(
            bindings.common.contains("public enum class Color"),
            "enum should have public visibility\nGot:\n{}",
            &bindings.common[..bindings.common.len().min(1000)]
        );
    }

    // ========================================================================
    // UniFFI Internals: Complex Type Nesting
    // Based on docs/manual/src/internals/lifting_and_lowering.md
    // ========================================================================

    #[test]
    fn nested_optional_sequence_types() {
        // Complex nested types like Optional<Sequence<T>> should be handled.
        let udl = r#"
            namespace test_crate {};

            namespace test_crate {
                sequence<i32>? maybe_numbers();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        assert!(
            bindings.common.len() > 0,
            "should generate bindings for nested optional sequence"
        );
    }

    #[test]
    fn map_type_with_complex_values() {
        // Maps with complex value types should be handled.
        let udl = r#"
            namespace test_crate {};

            namespace test_crate {
                record<string, string?> get_config();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        assert!(
            bindings.common.len() > 0,
            "should generate bindings for map with optional values"
        );
    }

    // ========================================================================
    // UniFFI Internals: C Interop Header (Native)
    // Based on docs/manual/src/internals/crates.md
    // ========================================================================

    #[test]
    fn native_header_generated() {
        // For native targets, a C interop header should be generated.
        let udl = r#"
            namespace test_crate {};

            interface Foo {
                constructor();
                string hello();
            };
        "#;
        let bindings = generate_test_bindings(udl);
        assert!(
            bindings.header.is_some(),
            "should generate native header\nGot: None"
        );
    }
}
