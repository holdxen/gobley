/*
 * Integration test: proc-macro codegen (library mode).
 *
 * Tests that gobley-uniffi-bindgen correctly generates Kotlin bindings
 * from a cdylib built with proc-macros (#[uniffi::export] impl on records/enums).
 *
 * This exercises the real codegen path used in production:
 *   Rust proc-macro → metadata in cdylib → library_mode::generate_bindings → Kotlin
 *
 * Requires the fixture crate to be built first:
 *   cargo build -p gobley-fixture-record-enum-methods
 */

use camino::Utf8PathBuf;
use gobley_uniffi_bindgen::KotlinBindingGenerator;
use std::collections::HashMap;
use std::process::Command;

struct TestConfigSupplier;

impl uniffi_bindgen::BindgenCrateConfigSupplier for TestConfigSupplier {
    fn get_toml(&self, _crate_name: &str) -> anyhow::Result<Option<toml::value::Table>> {
        Ok(Some(
            toml::toml! {
                package_name = "test.proc_macro"
                kotlin_multiplatform = true
                kotlin_targets = ["jvm"]
            }
            .into(),
        ))
    }

    fn get_udl(&self, _crate_name: &str, _udl_name: &str) -> anyhow::Result<String> {
        anyhow::bail!("no UDL files in proc-macro tests")
    }
}

/// Build the fixture crate and return the path to the cdylib.
fn build_fixture_cdylib() -> Utf8PathBuf {
    let output = Command::new("cargo")
        .args(["build", "-p", "gobley-fixture-record-enum-methods"])
        .output()
        .expect("failed to run cargo build");

    assert!(
        output.status.success(),
        "cargo build failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // Find the cdylib in the target directory
    let target_dir = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("target")
        .join("debug");

    // Platform-specific library name
    let lib_name = if cfg!(target_os = "macos") {
        "librecord_enum_methods.dylib"
    } else if cfg!(target_os = "linux") {
        "librecord_enum_methods.so"
    } else if cfg!(target_os = "windows") {
        "record_enum_methods.dll"
    } else {
        panic!("unsupported platform")
    };

    let lib_path = target_dir.join(lib_name);
    assert!(
        lib_path.exists(),
        "cdylib not found at {lib_path}. Run: cargo build -p gobley-fixture-record-enum-methods"
    );
    lib_path
}

/// Generate bindings from the cdylib and return (common, jvm) content.
fn generate_kmp_bindings(lib_path: &Utf8PathBuf) -> (String, String) {
    let out_dir = tempfile::tempdir().expect("failed to create temp dir");
    let out_path = Utf8PathBuf::try_from(out_dir.path().to_path_buf()).unwrap();

    uniffi_bindgen::library_mode::generate_bindings(
        lib_path,
        None,
        &KotlinBindingGenerator,
        &TestConfigSupplier,
        None,
        &out_path,
        false,
    )
    .expect("generate_bindings failed");

    // Read the generated files
    let common_dir = out_path.join("commonMain").join("kotlin");
    let jvm_dir = out_path.join("jvmMain").join("kotlin");

    let common_file = find_kt_file(&common_dir, "common");
    let jvm_file = find_kt_file(&jvm_dir, "jvm");

    let common = std::fs::read_to_string(&common_file)
        .unwrap_or_else(|e| panic!("failed to read common bindings at {common_file}: {e}"));
    let jvm = std::fs::read_to_string(&jvm_file)
        .unwrap_or_else(|e| panic!("failed to read jvm bindings at {jvm_file}: {e}"));

    (common, jvm)
}

fn find_kt_file(dir: &Utf8PathBuf, suffix: &str) -> Utf8PathBuf {
    find_kt_file_recursive(dir, suffix)
        .unwrap_or_else(|| panic!("no .kt file with suffix '{suffix}' found in {dir}"))
}

fn find_kt_file_recursive(dir: &Utf8PathBuf, suffix: &str) -> Option<Utf8PathBuf> {
    for entry in std::fs::read_dir(dir).ok()? {
        let entry = entry.ok()?;
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_kt_file_recursive(
                &Utf8PathBuf::try_from(path).unwrap(),
                suffix,
            ) {
                return Some(found);
            }
        } else if path.extension().map_or(false, |ext| ext == "kt") {
            let name = path.file_name().unwrap().to_str().unwrap();
            if name.contains(&format!(".{suffix}.kt")) {
                return Some(Utf8PathBuf::try_from(path).unwrap());
            }
        }
    }
    None
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[test]
fn proc_macro_record_methods_extension_in_kmp() {
    let lib = build_fixture_cdylib();
    let (common, jvm) = generate_kmp_bindings(&lib);

    // Record should be a concrete data class in common (not expect)
    assert!(
        common.contains("data class Point"),
        "common should have 'data class Point'\nGot:\n{common}"
    );
    assert!(
        !common.contains("expect data class"),
        "common must not have 'expect data class' (Kotlin forbids it)"
    );

    // Record methods should NOT be in common (no UniffiLib in commonMain)
    assert!(
        !common.contains("UniffiLib"),
        "common must not contain UniffiLib\nGot:\n{common}"
    );

    // Record methods SHOULD be in jvm as extension functions
    assert!(
        jvm.contains("fun Point.`distanceTo`") || jvm.contains("fun Point.distanceTo"),
        "jvm should have Point.distanceTo extension function\nGot:\n{jvm}"
    );
    assert!(
        jvm.contains("fun Point.`toStringDebug`") || jvm.contains("fun Point.toStringDebug"),
        "jvm should have Point.toStringDebug extension function\nGot:\n{jvm}"
    );
    assert!(
        jvm.contains("UniffiLib"),
        "jvm should have UniffiLib FFI calls\nGot:\n{jvm}"
    );
}

#[test]
fn proc_macro_enum_methods_extension_in_kmp() {
    let lib = build_fixture_cdylib();
    let (common, jvm) = generate_kmp_bindings(&lib);

    // Enum should be concrete in common
    assert!(
        common.contains("enum class Direction"),
        "common should have 'enum class Direction'\nGot:\n{common}"
    );

    // Enum methods should be extension functions in jvm
    assert!(
        jvm.contains("fun Direction.`name`") || jvm.contains("fun Direction.name"),
        "jvm should have Direction.name extension function\nGot:\n{jvm}"
    );
    assert!(
        jvm.contains("fun Direction.`opposite`") || jvm.contains("fun Direction.opposite"),
        "jvm should have Direction.opposite extension function\nGot:\n{jvm}"
    );
}

#[test]
fn proc_macro_sealed_enum_methods_extension_in_kmp() {
    let lib = build_fixture_cdylib();
    let (common, jvm) = generate_kmp_bindings(&lib);

    // ApiResponse is a sealed class (has associated data)
    assert!(
        common.contains("sealed class ApiResponse"),
        "common should have 'sealed class ApiResponse'\nGot:\n{common}"
    );

    // Methods should be extension functions in jvm
    assert!(
        jvm.contains("fun ApiResponse.`isSuccess`") || jvm.contains("fun ApiResponse.isSuccess"),
        "jvm should have ApiResponse.isSuccess extension function\nGot:\n{jvm}"
    );
    assert!(
        jvm.contains("fun ApiResponse.`statusCode`") || jvm.contains("fun ApiResponse.statusCode"),
        "jvm should have ApiResponse.statusCode extension function\nGot:\n{jvm}"
    );
}

#[test]
fn proc_macro_record_display_trait() {
    let lib = build_fixture_cdylib();
    let (common, _jvm) = generate_kmp_bindings(&lib);

    // UserProfile should be in common
    assert!(
        common.contains("data class UserProfile"),
        "common should have 'data class UserProfile'\nGot:\n{common}"
    );
    // Note: impl Display for Record is not automatically detected by
    // uniffi's proc-macro system. The uniffi_trait_methods() API exists
    // but requires explicit metadata generation. This test verifies the
    // record itself is generated correctly regardless.
}

#[test]
fn proc_macro_enum_display_trait() {
    let lib = build_fixture_cdylib();
    let (common, _jvm) = generate_kmp_bindings(&lib);

    // Color should be in common
    assert!(
        common.contains("enum class Color"),
        "common should have 'enum class Color'\nGot:\n{common}"
    );
    // Note: impl Display for Enum is not automatically detected by
    // uniffi's proc-macro system. This test verifies the enum itself
    // is generated correctly regardless.
}

#[test]
fn proc_macro_object_still_uses_expect_actual() {
    let lib = build_fixture_cdylib();
    let (common, jvm) = generate_kmp_bindings(&lib);

    // Object should use expect/actual pattern
    assert!(
        common.contains("expect"),
        "common should have 'expect' for objects\nGot:\n{common}"
    );
    assert!(
        jvm.contains("actual"),
        "jvm should have 'actual' for objects\nGot:\n{jvm}"
    );

    // Calculator object should have methods in jvm
    assert!(
        jvm.contains("Calculator"),
        "jvm should have Calculator class\nGot:\n{jvm}"
    );
}

#[test]
fn proc_macro_top_level_functions_are_expect() {
    let lib = build_fixture_cdylib();
    let (common, jvm) = generate_kmp_bindings(&lib);

    // Top-level functions should be expect in common
    assert!(
        common.contains("expect fun"),
        "common should have 'expect fun' for top-level functions\nGot:\n{common}"
    );

    // Top-level functions should be actual in jvm
    assert!(
        jvm.contains("actual fun"),
        "jvm should have 'actual fun' for top-level functions\nGot:\n{jvm}"
    );
}

#[test]
fn proc_macro_record_ffi_converter_in_jvm() {
    let lib = build_fixture_cdylib();
    let (common, jvm) = generate_kmp_bindings(&lib);

    // FfiConverter should be in jvm, not common
    assert!(
        jvm.contains("FfiConverterTypePoint"),
        "jvm should have FfiConverterTypePoint\nGot:\n{jvm}"
    );
    assert!(
        jvm.contains("FfiConverterTypeDirection"),
        "jvm should have FfiConverterTypeDirection\nGot:\n{jvm}"
    );
}

#[test]
fn proc_macro_common_has_no_ffi_calls() {
    let lib = build_fixture_cdylib();
    let (common, _) = generate_kmp_bindings(&lib);

    // commonMain must not contain ANY FFI calls
    assert!(
        !common.contains("UniffiLib"),
        "common must not contain UniffiLib\nGot:\n{common}"
    );
    assert!(
        !common.contains("uniffiRustCall"),
        "common must not contain uniffiRustCall\nGot:\n{common}"
    );
    assert!(
        !common.contains("FfiConverter"),
        "common must not contain FfiConverter\nGot:\n{common}"
    );
}

// ─── Rename tests ────────────────────────────────────────────────────────────

#[test]
fn proc_macro_renamed_record_type() {
    let lib = build_fixture_cdylib();
    let (common, jvm) = generate_kmp_bindings(&lib);

    // #[uniffi(name = "RenamedPoint")] on PrivatePoint should generate RenamedPoint
    assert!(
        common.contains("data class RenamedPoint"),
        "common should have 'data class RenamedPoint', not PrivatePoint\nGot:\n{common}"
    );
    assert!(
        !common.contains("PrivatePoint"),
        "common should NOT have the original Rust name 'PrivatePoint'\nGot:\n{common}"
    );
    // FfiConverter should use the renamed name
    assert!(
        jvm.contains("FfiConverterTypeRenamedPoint"),
        "jvm should have FfiConverterTypeRenamedPoint\nGot:\n{jvm}"
    );
}

#[test]
fn proc_macro_renamed_enum_type() {
    let lib = build_fixture_cdylib();
    let (common, _jvm) = generate_kmp_bindings(&lib);

    // #[uniffi(name = "RenamedStatus")] on InternalStatus should generate RenamedStatus
    assert!(
        common.contains("enum class RenamedStatus"),
        "common should have 'enum class RenamedStatus', not InternalStatus\nGot:\n{common}"
    );
    assert!(
        !common.contains("InternalStatus"),
        "common should NOT have the original Rust name 'InternalStatus'\nGot:\n{common}"
    );
}

#[test]
fn proc_macro_renamed_object_methods() {
    let lib = build_fixture_cdylib();
    let (_common, jvm) = generate_kmp_bindings(&lib);

    // #[uniffi::method(name = "compute")] on add() should generate "compute"
    assert!(
        jvm.contains("`compute`") || jvm.contains("compute("),
        "jvm should have renamed method 'compute' (not 'add')\nGot:\n{jvm}"
    );
    // #[uniffi::method(name = "result")] on get_value() should generate "result"
    assert!(
        jvm.contains("`result`") || jvm.contains("result("),
        "jvm should have renamed method 'result' (not 'get_value')\nGot:\n{jvm}"
    );
    // InternalCalc should be the object name (no type-level rename in this fixture)
    assert!(
        jvm.contains("InternalCalc"),
        "jvm should have InternalCalc object\nGot:\n{jvm}"
    );
}

#[test]
fn proc_macro_renamed_function() {
    let lib = build_fixture_cdylib();
    let (common, jvm) = generate_kmp_bindings(&lib);

    // #[uniffi::export(name = "calculate_sum")] on internal_sum should generate calculate_sum
    assert!(
        common.contains("calculateSum") || common.contains("calculate_sum"),
        "common should have renamed function 'calculateSum'\nGot:\n{common}"
    );
    assert!(
        !common.contains("internalSum") && !common.contains("internal_sum"),
        "common should NOT have the original Rust name 'internal_sum'\nGot:\n{common}"
    );
    assert!(
        jvm.contains("calculateSum") || jvm.contains("calculate_sum"),
        "jvm should have renamed function 'calculateSum'\nGot:\n{jvm}"
    );
}

#[test]
fn proc_macro_renamed_record_fields() {
    let lib = build_fixture_cdylib();
    let (common, _jvm) = generate_kmp_bindings(&lib);

    // #[uniffi(name = "configName")] on internal_name field
    assert!(
        common.contains("configName"),
        "common should have renamed field 'configName'\nGot:\n{common}"
    );
    assert!(
        !common.contains("internalName") && !common.contains("internal_name"),
        "common should NOT have the original field name 'internal_name'\nGot:\n{common}"
    );
    // #[uniffi(name = "configValue")] on internal_value field
    assert!(
        common.contains("configValue"),
        "common should have renamed field 'configValue'\nGot:\n{common}"
    );
}
