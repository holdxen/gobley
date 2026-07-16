#!/usr/bin/env python3
"""
Gobley runtime integration test script.

Builds a Rust cdylib, generates Kotlin bindings via gobley-uniffi-bindgen,
creates a temporary Kotlin Gradle project, and runs runtime tests to verify
the generated bindings actually work.

Supports both KMP (kotlin_multiplatform = true) and non-KMP modes, so the
extension-function code path can be exercised alongside the inline path.

Usage:
    python3 test_runtime.py [--fixture PATH] [--bindgen PATH] [--keep] [--kmp]
    python3 test_runtime.py --kmp   # test KMP extension-function path

Requirements:
    - Rust toolchain (cargo)
    - JDK 17+
    - Gradle wrapper available in the gobley project
"""

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

# ── TOML parsing: prefer stdlib tomllib (3.11+), then tomli, then manual ──
try:
    import tomllib as _toml_lib
except ImportError:
    try:
        import tomli as _toml_lib
    except ImportError:
        _toml_lib = None


def parse_cargo_toml(path: Path) -> dict:
    """Parse a Cargo.toml file using tomllib if available, else manual fallback."""
    if _toml_lib is not None:
        with open(path, "rb") as f:
            return _toml_lib.load(f)
    # ── Manual fallback (handles the common flat + [lib] structure) ──
    result: dict = {}
    section = result
    with open(path) as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            if line.startswith("[") and line.endswith("]"):
                sec_name = line[1:-1]
                d = result
                for part in sec_name.split("."):
                    d = d.setdefault(part, {})
                section = d
                continue
            if "=" in line:
                key, _, val = line.partition("=")
                key = key.strip()
                val = val.strip().strip('"')
                section[key] = val
    return result


def run(cmd, cwd=None, check=True, capture=False):
    """Run a command and optionally capture output."""
    display = " ".join(cmd) if isinstance(cmd, list) else cmd
    print(f"  $ {display}")
    result = subprocess.run(
        cmd, cwd=cwd, shell=isinstance(cmd, str),
        capture_output=capture, text=True,
    )
    if check and result.returncode != 0:
        print(f"  FAILED (exit {result.returncode})")
        if capture:
            print(result.stdout)
            print(result.stderr)
        sys.exit(1)
    return result


def find_project_root():
    """Find the gobley project root (contains Cargo.toml with workspace)."""
    candidates = [
        Path(__file__).parent.parent,
        Path(__file__).resolve().parent.parent,
        Path.cwd(),
    ]
    for p in candidates:
        if (p / "Cargo.toml").exists():
            with open(p / "Cargo.toml") as f:
                if "gobley-uniffi-bindgen" in f.read():
                    return p.resolve()
    print("ERROR: Cannot find gobley project root. Run from the gobley directory.")
    sys.exit(1)


def build_bindgen(project_root):
    """Build gobley-uniffi-bindgen binary."""
    print("\n[1/6] Building gobley-uniffi-bindgen...")
    run(["cargo", "build", "-p", "gobley-uniffi-bindgen"], cwd=project_root)
    exe = ".exe" if sys.platform == "win32" else ""
    bindgen = project_root / "target" / "debug" / f"gobley-uniffi-bindgen{exe}"
    if not bindgen.exists():
        print(f"ERROR: Bindgen binary not found at {bindgen}")
        sys.exit(1)
    print(f"  -> {bindgen}")
    return bindgen


def build_fixture(project_root, fixture_crate, fixture_path):
    """Build the fixture Rust crate to produce a cdylib."""
    print(f"\n[2/6] Building fixture crate '{fixture_crate}'...")
    run(["cargo", "build", "-p", fixture_crate], cwd=project_root)

    cargo_toml = parse_cargo_toml(fixture_path / "Cargo.toml")
    lib_name = cargo_toml.get("lib", {}).get("name")
    if not lib_name:
        lib_name = fixture_crate.replace("-", "_")

    target_dir = project_root / "target" / "debug"
    prefix = "" if sys.platform == "win32" else "lib"
    exts = {"darwin": ".dylib", "linux": ".so", "win32": ".dll"}
    ext = exts.get(sys.platform, ".dylib")

    cdylib = target_dir / f"{prefix}{lib_name}{ext}"
    if not cdylib.exists():
        print(f"ERROR: cdylib not found at {cdylib}")
        sys.exit(1)
    print(f"  -> {cdylib}")
    return cdylib


def generate_bindings(bindgen, cdylib, crate_path, config_toml, out_dir, kmp=False):
    """Generate Kotlin bindings using gobley-uniffi-bindgen."""
    mode = "KMP" if kmp else "non-KMP"
    print(f"\n[3/6] Generating Kotlin bindings ({mode} mode)...")
    shutil.rmtree(out_dir, ignore_errors=True)
    out_dir.mkdir(parents=True, exist_ok=True)

    cmd = [
        str(bindgen),
        "--library",
        "--out-dir", str(out_dir),
        "--config", str(config_toml),
        "--crate-paths", f"test={crate_path}",
        str(cdylib),
    ]
    run(cmd)

    kt_files = list(out_dir.rglob("*.kt"))
    print(f"  -> {len(kt_files)} Kotlin file(s) generated")
    for f in kt_files:
        print(f"     {f.relative_to(out_dir)}")
    return kt_files


# ── Constants ──────────────────────────────────────────────────────────────
PACKAGE_NAME = "io.gobley.test"
KOTLIN_VERSION = "2.4.0"
JNA_VERSION = "5.18.1"
ATOMICFU_VERSION = "0.26.1"

# ── Non-KMP build.gradle.kts ───────────────────────────────────────────────
BUILD_GRADLE_KTS = """plugins {
    kotlin("jvm") version "__KOTLIN_VERSION__"
    application
}
application {
    mainClass.set("__PACKAGE_NAME__.MainKt")
}
repositories {
    mavenCentral()
}
dependencies {
    implementation("net.java.dev.jna:jna:__JNA_VERSION__")
    implementation("org.jetbrains.kotlinx:atomicfu:__ATOMICFU_VERSION__")
}
tasks.withType<JavaExec> {
    systemProperty("jna.library.path",
        System.getProperty("cdylib.path") ?: "${projectDir}/lib")
    systemProperty("test.kmp",
        System.getProperty("test.kmp", "false"))
}
"""

# ── KMP build.gradle.kts ──────────────────────────────────────────────────
# Note: `application` plugin is incompatible with KMP. We use a custom JavaExec.
BUILD_GRADLE_KTS_KMP = """plugins {
    kotlin("multiplatform") version "__KOTLIN_VERSION__"
}
repositories {
    mavenCentral()
}
kotlin {
    jvm()
    sourceSets {
        commonMain.dependencies {
            implementation("net.java.dev.jna:jna:__JNA_VERSION__")
            implementation("org.jetbrains.kotlinx:atomicfu:__ATOMICFU_VERSION__")
        }
    }
}
// application plugin is incompatible with KMP; use a custom JavaExec task.
tasks.register<JavaExec>("runTest") {
    dependsOn("jvmJar")
    classpath = files(tasks.named("jvmJar"), configurations["jvmRuntimeClasspath"])
    mainClass.set("__PACKAGE_NAME__.MainKt")
    systemProperty("jna.library.path",
        System.getProperty("cdylib.path") ?: "${projectDir}/lib")
    systemProperty("test.kmp",
        System.getProperty("test.kmp", "false"))
}
"""


def create_kotlin_project(tmp_dir, generated_bindings_dir, cdylib_path,
                          gradlew_path, kmp=False):
    """Create a minimal Kotlin Gradle project for runtime testing."""
    mode = "KMP" if kmp else "non-KMP"
    print(f"\n[4/6] Creating Kotlin test project ({mode} mode)...")

    tmp_dir.mkdir(parents=True, exist_ok=True)

    # settings.gradle.kts
    (tmp_dir / "settings.gradle.kts").write_text(
        'rootProject.name = "gobley-runtime-test"\n'
    )

    # build.gradle.kts
    template = BUILD_GRADLE_KTS_KMP if kmp else BUILD_GRADLE_KTS
    build_content = (template
        .replace("__KOTLIN_VERSION__", KOTLIN_VERSION)
        .replace("__PACKAGE_NAME__", PACKAGE_NAME)
        .replace("__JNA_VERSION__", JNA_VERSION)
        .replace("__ATOMICFU_VERSION__", ATOMICFU_VERSION)
    )
    (tmp_dir / "build.gradle.kts").write_text(build_content)

    # Copy Gradle wrapper
    shutil.copy2(gradlew_path, tmp_dir / "gradlew")
    wrapper_dir = tmp_dir / "gradle" / "wrapper"
    wrapper_dir.mkdir(parents=True, exist_ok=True)
    gradle_root = gradlew_path.parent
    shutil.copy2(gradle_root / "gradle" / "wrapper" / "gradle-wrapper.jar", wrapper_dir)
    shutil.copy2(gradle_root / "gradle" / "wrapper" / "gradle-wrapper.properties", wrapper_dir)
    os.chmod(tmp_dir / "gradlew", 0o755)

    # Copy generated bindings into the correct source-set directory
    if kmp:
        # KMP: bindgen outputs commonMain/ and jvmMain/ subdirectories
        src_base = tmp_dir / "src"
        for kt_file in generated_bindings_dir.rglob("*.kt"):
            rel = kt_file.relative_to(generated_bindings_dir)
            # rel is like "commonMain/kotlin/.../*.kt" or "jvmMain/kotlin/.../*.kt"
            dest = src_base / rel
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(kt_file, dest)
    else:
        # Non-KMP: bindgen outputs main/kotlin/...
        src_dir = tmp_dir / "src" / "main" / "kotlin"
        src_dir.mkdir(parents=True, exist_ok=True)
        for kt_file in generated_bindings_dir.rglob("*.kt"):
            rel = kt_file.relative_to(generated_bindings_dir)
            dest = src_dir / rel
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(kt_file, dest)

    # Copy cdylib to lib/ as fallback (primary path via -Dcdylib.path)
    lib_dir = tmp_dir / "lib"
    lib_dir.mkdir(exist_ok=True)
    shutil.copy2(cdylib_path, lib_dir / cdylib_path.name)

    # Write Main.kt into the JVM source set
    if kmp:
        main_kt_dir = tmp_dir / "src" / "jvmMain" / "kotlin"
    else:
        main_kt_dir = tmp_dir / "src" / "main" / "kotlin"
    main_kt_dir.mkdir(parents=True, exist_ok=True)
    pkg_dir = main_kt_dir / "io" / "gobley" / "test"
    pkg_dir.mkdir(parents=True, exist_ok=True)
    (pkg_dir / "Main.kt").write_text(MAIN_KT)

    print(f"  -> {tmp_dir}")
    return tmp_dir


# ── Test runner (shared by both modes) ─────────────────────────────────────
MAIN_KT = r'''package io.gobley.test

import io.gobley.test.*

fun main() {
    var passed = 0
    var failed = 0

    fun test(name: String, block: () -> Unit) {
        try {
            block()
            println("  PASS: $name")
            passed++
        } catch (e: Exception) {
            println("  FAIL: $name: ${e.message}")
            failed++
        }
    }

    fun assert(condition: Boolean, message: String) {
        if (!condition) throw AssertionError(message)
    }

    val isKmp = System.getProperty("test.kmp") == "true"
    println("=== Gobley Runtime Tests (${if (isKmp) "KMP" else "non-KMP"}) ===\n")

    // ─── Record methods ───
    println("Record methods:")
    test("Point.distanceTo") {
        val p1 = createPoint(0.0, 0.0)
        val p2 = createPoint(3.0, 4.0)
        assert(p1.distanceTo(p2) == 5.0, "Expected 5.0")
    }
    test("Point.toStringDebug") {
        assert(createPoint(1.5, 2.5).toStringDebug() == "(1.5, 2.5)")
    }
    test("Vector2D.length") {
        assert(Vector2D(3.0, 4.0).length() == 5.0)
    }
    test("Vector2D.scale") {
        val v = Vector2D(1.0, 2.0).scale(3.0)
        assert(v.dx == 3.0 && v.dy == 6.0)
    }

    // ─── Enum methods ───
    println("\nEnum methods:")
    test("Direction.opposite") {
        assert(Direction.NORTH.opposite() == Direction.SOUTH)
    }
    // 'name' is a Kotlin reserved enum method; bindgen renames it to 'rustName'
    test("Direction.rustName (renamed from 'name')") {
        assert(Direction.NORTH.rustName() == "North")
    }

    // ─── Sealed enum methods ───
    println("\nSealed enum methods:")
    test("ApiResponse.isSuccess (true)") {
        assert(ApiResponse.Success("ok", 200u).isSuccess())
    }
    test("ApiResponse.isSuccess (false)") {
        assert(!ApiResponse.Error("fail").isSuccess())
    }
    test("ApiResponse.statusCode") {
        assert(ApiResponse.Success("ok", 200u).statusCode() == 200u)
    }

    // ─── Display trait (toString) ───
    println("\nDisplay trait (toString):")
    // Color: Kotlin's default Enum.toString() returns the variant name (e.g. "RED").
    //   - non-KMP: override toString() returns Rust-side Display "Red"
    //   - KMP: extension toString() is shadowed by Enum.toString(), returns "RED"
    test("Color.RED.toString") {
        if (isKmp) {
            assert(Color.RED.toString() == "RED",
                "In KMP, extension is shadowed by Enum.toString(); got: ${Color.RED.toString()}")
        } else {
            assert(Color.RED.toString() == "Red", "Expected 'Red' from Rust-side Display")
        }
    }
    test("Color.GREEN.toString") {
        if (isKmp) {
            assert(Color.GREEN.toString() == "GREEN",
                "In KMP, extension is shadowed by Enum.toString(); got: ${Color.GREEN.toString()}")
        } else {
            assert(Color.GREEN.toString() == "Green", "Expected 'Green' from Rust-side Display")
        }
    }
    // UserProfile: data class default toString() returns "UserProfile(name=Alice, age=25)",
    // which differs from the Rust-side Display "Alice (age: 25)".
    //   - non-KMP: toString() is an override → returns Rust-side value
    //   - KMP: toString() is an extension function shadowed by Any.toString()
    test("UserProfile.toString (Rust-side Display)") {
        if (isKmp) {
            // Extension is shadowed; Any.toString() is called instead.
            val result = UserProfile("Alice", 25u).toString()
            assert(result != "Alice (age: 25)",
                "In KMP mode toString() extension is shadowed by Any.toString(); got: $result")
        } else {
            assert(UserProfile("Alice", 25u).toString() == "Alice (age: 25)",
                "Expected 'Alice (age: 25)'")
        }
    }

    // ─── Eq trait (equals) ───
    println("\nEq trait (equals):")
    // UserProfile is a data class, so Kotlin's auto-generated equals() compares
    // all constructor properties (name, age) — same as Rust-side Eq.
    //   - non-KMP: override equals() from Rust-side Eq (coincidentally same result)
    //   - KMP: extension equals() is shadowed, but data class equals() gives
    //     the same structural equality result.
    // Note: if Rust-side Eq had custom logic (e.g. ignoring a field), KMP mode
    // would silently use the data class's equals() instead, producing different results.
    test("UserProfile.equals (equal)") {
        val u1 = UserProfile("Alice", 25u)
        val u2 = UserProfile("Alice", 25u)
        assert(u1 == u2, "Equal profiles should be equal (structural equality via data class)")
    }
    test("UserProfile.equals (not equal)") {
        val u1 = UserProfile("Alice", 25u)
        val u2 = UserProfile("Bob", 30u)
        assert(u1 != u2, "Different profiles should not be equal")
    }

    // ─── Ord trait (compareTo) ───
    // compareTo as operator extension works in both modes because
    // Any does not have a compareTo member to shadow it.
    println("\nOrd trait (compareTo):")
    test("UserProfile compareTo (age ordering)") {
        val u1 = UserProfile("Alice", 25u)
        val u2 = UserProfile("Bob", 30u)
        assert(u1 < u2, "Alice (25) should be less than Bob (30)")
    }
    test("UserProfile compareTo (same age, name ordering)") {
        val u1 = UserProfile("Alice", 25u)
        val u2 = UserProfile("Bob", 25u)
        assert(u1 < u2, "Alice should be less than Bob when same age")
    }
    test("UserProfile compareTo (equal)") {
        val u1 = UserProfile("Alice", 25u)
        val u2 = UserProfile("Alice", 25u)
        assert(u1.compareTo(u2) == 0, "Equal profiles should have compareTo == 0")
    }

    // ─── Renamed types ───
    println("\nRenamed types:")
    test("RenamedPoint") {
        val p = RenamedPoint(1.0, 2.0)
        assert(p.x == 1.0 && p.y == 2.0)
    }
    test("RenamedStatus") {
        assert(RenamedStatus.ACTIVE == RenamedStatus.ACTIVE)
    }
    test("Config renamed fields") {
        val c = Config("test", 42)
        assert(c.configName == "test" && c.configValue == 42)
    }
    test("calculateSum") {
        assert(calculateSum(3, 4) == 7)
    }

    // ─── Object methods ───
    println("\nObject methods:")
    test("Calculator.add") {
        assert(Calculator(10.0).add(5.0) == 15.0)
    }
    test("Calculator.getValue") {
        val c = Calculator(10.0)
        c.add(5.0)
        assert(c.getValue() == 15.0)
    }
    test("InternalCalc.compute (renamed)") {
        assert(InternalCalc(10.0).compute(5.0) == 15.0)
    }
    test("InternalCalc.result (renamed)") {
        val c = InternalCalc(10.0)
        c.compute(5.0)
        assert(c.result() == 15.0)
    }

    // ─── uniffiIsDestroyed ───
    println("\nuniffiIsDestroyed:")
    test("uniffiIsDestroyed false for new object") {
        val calc = Calculator(10.0)
        assert(!calc.uniffiIsDestroyed, "New object should not be destroyed")
        calc.close()
    }
    test("uniffiIsDestroyed true after close") {
        val calc = Calculator(10.0)
        calc.close()
        assert(calc.uniffiIsDestroyed, "Object should be destroyed after close()")
    }

    // ─── Trait: Rust-only ───
    println("\nTrait: Rust-only:")
    test("Logger.log + Logger.level") {
        val logger = getLogger()
        logger.log("test message")
        assert(logger.level() == 1u, "Expected level 1")
    }

    // ─── Trait: explicit Rust-only ───
    println("\nTrait: explicit Rust-only:")
    test("Formatter.format") {
        assert(getFormatter().format("hello") == """{"data": "hello"}""")
    }

    // ─── Trait: Rust + foreign ───
    println("\nTrait: Rust + foreign:")
    test("EventHandler from Rust") {
        assert(processEvent(getEventHandler(), "click") == "handled: click")
    }
    test("EventHandler from Kotlin (callback)") {
        var onEventCalled = false
        var receivedData = ""
        val handler = object : EventHandler {
            override fun onEvent(eventName: String, data: String) {
                onEventCalled = true
                receivedData = data
            }
            override fun shouldHandle(eventName: String) = eventName == "important"
        }
        // 'important' should be handled
        assert(processEvent(handler, "important") == "handled: important")
        assert(onEventCalled, "onEvent should have been called for 'important'")
        assert(receivedData == "processed",
            "onEvent data should be 'processed', got: $receivedData")
        // 'trivial' should be skipped
        onEventCalled = false
        assert(processEvent(handler, "trivial") == "skipped: trivial")
        assert(!onEventCalled, "onEvent should NOT have been called for 'trivial'")
    }

    // ─── Trait: foreign-only (callback) ───
    println("\nTrait: foreign-only (callback):")
    test("DataStore Kotlin implementation") {
        val store = object : DataStore {
            private val m = mutableMapOf<String, String>()
            override fun get(key: String) = m[key]
            override fun set(key: String, value: String) { m[key] = value }
            override fun hasKey(key: String) = m.containsKey(key)
        }
        store.set("name", "Alice")
        assert(useDataStore(store, "name") == "Alice")
    }
    test("DataStore hasKey") {
        val store = object : DataStore {
            private val m = mutableMapOf<String, String>()
            override fun get(key: String) = m[key]
            override fun set(key: String, value: String) { m[key] = value }
            override fun hasKey(key: String) = m.containsKey(key)
        }
        store.set("k", "v")
        assert(store.hasKey("k") && !store.hasKey("x"))
    }

    // ─── Top-level functions ───
    println("\nTop-level functions:")
    test("createPoint") {
        val p = createPoint(1.0, 2.0)
        assert(p.x == 1.0 && p.y == 2.0)
    }
    test("pointDistance") {
        assert(pointDistance(createPoint(0.0, 0.0), createPoint(3.0, 4.0)) == 5.0)
    }

    // ─── Summary ───
    println("\n=== Results: $passed passed, $failed failed ===")
    if (failed > 0) {
        System.exit(1)
    }
}
'''


def run_tests(project_dir, cdylib_path, kmp=False):
    """Run the Kotlin runtime tests."""
    print("\n[5/6] Running Kotlin runtime tests...")
    kmp_flag = "true" if kmp else "false"
    # KMP uses custom 'runTest' task; non-KMP uses 'run' (application plugin)
    gradle_task = "runTest" if kmp else "run"
    result = run(
        ["./gradlew", gradle_task,
         f"-Dcdylib.path={cdylib_path.parent}",
         f"-Dtest.kmp={kmp_flag}"],
        cwd=project_dir, check=False, capture=True
    )
    print(result.stdout)
    if result.stderr:
        print(result.stderr)
    return result.returncode == 0


def main():
    parser = argparse.ArgumentParser(description="Gobley runtime integration test")
    parser.add_argument("--fixture", default="tests/uniffi/record-enum-methods",
                        help="Path to fixture crate (relative to project root)")
    parser.add_argument("--bindgen", default=None,
                        help="Path to pre-built bindgen binary")
    parser.add_argument("--keep", action="store_true",
                        help="Keep temporary project after test")
    parser.add_argument("--kmp", action="store_true",
                        help="Test KMP mode (extension functions) instead of non-KMP (inline)")
    parser.add_argument("--project-root", default=None,
                        help="Gobley project root (auto-detected if not set)")
    args = parser.parse_args()

    project_root = Path(args.project_root) if args.project_root else find_project_root()
    print(f"Project root: {project_root}")
    print(f"Mode: {'KMP' if args.kmp else 'non-KMP'}")

    fixture_crate = args.fixture
    fixture_path = project_root / fixture_crate
    if not fixture_path.exists():
        print(f"ERROR: Fixture path not found: {fixture_path}")
        sys.exit(1)

    # Read crate name from Cargo.toml
    cargo_toml = parse_cargo_toml(fixture_path / "Cargo.toml")
    fixture_name = cargo_toml.get("package", {}).get("name")
    if not fixture_name:
        # Fallback: first 'name' in file (manual parse may nest differently)
        with open(fixture_path / "Cargo.toml") as f:
            for line in f:
                if line.strip().startswith("name"):
                    fixture_name = line.split("=")[1].strip().strip('"')
                    break
    if not fixture_name:
        print("ERROR: Cannot find crate name in Cargo.toml")
        sys.exit(1)

    gradlew = project_root / "gradlew"
    if not gradlew.exists():
        print(f"ERROR: gradlew not found at {gradlew}")
        sys.exit(1)

    # Step 1: Build or locate bindgen
    if args.bindgen:
        bindgen = Path(args.bindgen)
        if not bindgen.exists():
            print(f"ERROR: Bindgen not found at {bindgen}")
            sys.exit(1)
        print(f"\n[1/6] Using pre-built bindgen: {bindgen}")
    else:
        bindgen = build_bindgen(project_root)

    # Step 2: Build fixture
    cdylib = build_fixture(project_root, fixture_name, fixture_path)

    # Step 3: Generate bindings
    tmp = Path(tempfile.mkdtemp(prefix="gobley-test-"))
    bindings_dir = tmp / "bindings"
    flat_config = tmp / "config.toml"
    if args.kmp:
        flat_config.write_text(
            f'package_name = "{PACKAGE_NAME}"\n'
            'kotlin_multiplatform = true\n'
            'kotlin_targets = ["jvm"]\n'
        )
    else:
        flat_config.write_text(
            f'package_name = "{PACKAGE_NAME}"\n'
            'kotlin_multiplatform = false\n'
            'kotlin_targets = ["jvm"]\n'
        )
    generate_bindings(bindgen, cdylib, fixture_path, flat_config,
                      bindings_dir, kmp=args.kmp)

    # Step 4: Create Kotlin project
    project_dir = create_kotlin_project(
        tmp / "project", bindings_dir, cdylib, gradlew, kmp=args.kmp
    )

    # Step 5: Run tests
    success = run_tests(project_dir, cdylib, kmp=args.kmp)

    # Step 6: Cleanup
    if args.keep:
        print(f"\n[6/6] Project kept at: {project_dir}")
    else:
        print(f"\n[6/6] Cleaning up {tmp}...")
        shutil.rmtree(tmp, ignore_errors=True)

    if success:
        print("\n✅ All runtime tests passed!")
        return 0
    else:
        print("\n❌ Some runtime tests failed!")
        return 1


if __name__ == "__main__":
    sys.exit(main())
#!/usr/bin/env python3
"""
Gobley runtime integration test script.

Builds a Rust cdylib, generates Kotlin bindings via gobley-uniffi-bindgen,
creates a temporary Kotlin/JVM Gradle project, and runs runtime tests to verify
the generated bindings actually work.

Usage:
    python3 test_runtime.py [--fixture PATH] [--bindgen PATH] [--keep]

Requirements:
    - Rust toolchain (cargo)
    - JDK 17+
    - Gradle wrapper available in the gobley project
"""

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


def run(cmd, cwd=None, check=True, capture=False):
    """Run a command and optionally capture output."""
    print(f"  $ {' '.join(cmd)}" if isinstance(cmd, list) else f"  $ {cmd}")
    result = subprocess.run(
        cmd, cwd=cwd, shell=isinstance(cmd, str),
        capture_output=capture, text=True
    )
    if check and result.returncode != 0:
        print(f"  FAILED (exit {result.returncode})")
        if capture:
            print(result.stdout)
            print(result.stderr)
        sys.exit(1)
    return result


def find_project_root():
    """Find the gobley project root (contains Cargo.toml with workspace)."""
    candidates = [
        Path(__file__).parent,
        Path(__file__).resolve().parent,
        Path.cwd(),
    ]
    for p in candidates:
        if (p / "Cargo.toml").exists():
            with open(p / "Cargo.toml") as f:
                if "gobley-uniffi-bindgen" in f.read():
                    return p.resolve()
    print("ERROR: Cannot find gobley project root. Run from the gobley directory.")
    sys.exit(1)


def build_bindgen(project_root):
    """Build gobley-uniffi-bindgen binary."""
    print("\n[1/6] Building gobley-uniffi-bindgen...")
    run(["cargo", "build", "-p", "gobley-uniffi-bindgen"], cwd=project_root)
    bindgen = project_root / "target" / "debug" / "gobley-uniffi-bindgen"
    if not bindgen.exists():
        print(f"ERROR: Bindgen binary not found at {bindgen}")
        sys.exit(1)
    print(f"  -> {bindgen}")
    return bindgen


def build_fixture(project_root, fixture_crate, fixture_path):
    """Build the fixture Rust crate to produce a cdylib."""
    print(f"\n[2/6] Building fixture crate '{fixture_crate}'...")
    run(["cargo", "build", "-p", fixture_crate], cwd=project_root)

    # Read the actual library name from Cargo.toml [lib] name
    cargo_toml = fixture_path / "Cargo.toml"
    lib_name = None
    in_lib_section = False
    with open(cargo_toml) as f:
        for line in f:
            line = line.strip()
            if line == "[lib]":
                in_lib_section = True
            elif line.startswith("[") and in_lib_section:
                break
            elif in_lib_section and line.startswith("name"):
                lib_name = line.split("=")[1].strip().strip('"')
                break
    if not lib_name:
        # Fallback: use crate name with hyphens replaced by underscores
        lib_name = fixture_crate.replace("-", "_")

    # Find the cdylib
    target_dir = project_root / "target" / "debug"
    lib_prefixes = {"darwin": "lib", "linux": "lib", "win32": ""}
    lib_exts = {"darwin": ".dylib", "linux": ".so", "win32": ".dll"}
    prefix = lib_prefixes.get(sys.platform, "lib")
    ext = lib_exts.get(sys.platform, ".dylib")

    cdylib = target_dir / f"{prefix}{lib_name}{ext}"
    if not cdylib.exists():
        print(f"ERROR: cdylib not found at {cdylib}")
        sys.exit(1)
    print(f"  -> {cdylib}")
    return cdylib


def generate_bindings(bindgen, cdylib, crate_path, config_toml, out_dir):
    """Generate Kotlin bindings using gobley-uniffi-bindgen."""
    print("\n[3/6] Generating Kotlin bindings...")
    shutil.rmtree(out_dir, ignore_errors=True)
    out_dir.mkdir(parents=True, exist_ok=True)

    cmd = [
        str(bindgen),
        "--library",
        "--out-dir", str(out_dir),
        "--config", str(config_toml),
        "--crate-paths", f"test={crate_path}",
        str(cdylib),
    ]
    run(cmd)

    # List generated files
    kt_files = list(out_dir.rglob("*.kt"))
    print(f"  -> {len(kt_files)} Kotlin file(s) generated")
    for f in kt_files:
        print(f"     {f.relative_to(out_dir)}")

    return kt_files


def create_kotlin_project(tmp_dir, generated_bindings_dir, cdylib_path, gradlew_path):
    """Create a minimal Kotlin/JVM Gradle project for runtime testing."""
    print("\n[4/6] Creating Kotlin test project...")

    # Create project directory
    tmp_dir.mkdir(parents=True, exist_ok=True)

    # settings.gradle.kts
    (tmp_dir / "settings.gradle.kts").write_text('rootProject.name = "gobley-runtime-test"\n')

    # build.gradle.kts
    (tmp_dir / "build.gradle.kts").write_text("""plugins {
    kotlin("jvm") version "2.0.21"
    application
}
application {
    mainClass.set("MainKt")
}
repositories {
    mavenCentral()
}
dependencies {
    implementation("net.java.dev.jna:jna:5.16.0")
    implementation("org.jetbrains.kotlinx:atomicfu:0.26.1")
}
tasks.withType<JavaExec> {
    systemProperty("jna.library.path", System.getProperty("cdylib.path") ?: "${projectDir}/lib")
}
""")

    # Copy gradlew
    shutil.copy2(gradlew_path, tmp_dir / "gradlew")
    wrapper_dir = tmp_dir / "gradle" / "wrapper"
    wrapper_dir.mkdir(parents=True, exist_ok=True)
    gradle_root = gradlew_path.parent
    shutil.copy2(gradle_root / "gradle" / "wrapper" / "gradle-wrapper.jar", wrapper_dir)
    shutil.copy2(gradle_root / "gradle" / "wrapper" / "gradle-wrapper.properties", wrapper_dir)
    os.chmod(tmp_dir / "gradlew", 0o755)

    # Copy generated bindings
    src_dir = tmp_dir / "src" / "main" / "kotlin"
    src_dir.mkdir(parents=True, exist_ok=True)
    # Copy all .kt files preserving directory structure
    for kt_file in generated_bindings_dir.rglob("*.kt"):
        rel = kt_file.relative_to(generated_bindings_dir)
        dest = src_dir / rel
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(kt_file, dest)

    # Copy cdylib
    lib_dir = tmp_dir / "lib"
    lib_dir.mkdir(exist_ok=True)
    shutil.copy2(cdylib_path, lib_dir / cdylib_path.name)

    # Write Main.kt
    (src_dir / "Main.kt").write_text(MAIN_KT)

    print(f"  -> {tmp_dir}")
    return tmp_dir


MAIN_KT = r'''import io.github.holdxen.svnexus.rust.*

fun main() {
    var passed = 0
    var failed = 0

    fun test(name: String, block: () -> Unit) {
        try {
            block()
            println("  PASS: $name")
            passed++
        } catch (e: Exception) {
            println("  FAIL: $name: ${e.message}")
            failed++
        }
    }

    fun assert(condition: Boolean, message: String) {
        if (!condition) throw AssertionError(message)
    }

    println("=== Gobley Runtime Tests ===\n")

    // ─── Record methods ───
    println("Record methods:")
    test("Point.distanceTo") {
        val p1 = createPoint(0.0, 0.0)
        val p2 = createPoint(3.0, 4.0)
        assert(p1.distanceTo(p2) == 5.0, "Expected 5.0")
    }
    test("Point.toStringDebug") {
        assert(createPoint(1.5, 2.5).toStringDebug() == "(1.5, 2.5)")
    }
    test("Vector2D.length") {
        assert(Vector2D(3.0, 4.0).length() == 5.0)
    }
    test("Vector2D.scale") {
        val v = Vector2D(1.0, 2.0).scale(3.0)
        assert(v.dx == 3.0 && v.dy == 6.0)
    }

    // ─── Enum methods ───
    println("\nEnum methods:")
    test("Direction.opposite") {
        assert(Direction.NORTH.opposite() == Direction.SOUTH)
    }
    // Verify enum method name conflict fix: 'name' was renamed to 'rustName'
    // by fn_name() filter to avoid conflict with Kotlin's built-in Enum.name
    test("Direction.rustName (renamed from 'name')") {
        assert(Direction.NORTH.rustName() == "North")
    }

    // ─── Sealed enum methods ───
    println("\nSealed enum methods:")
    test("ApiResponse.isSuccess (true)") {
        assert(ApiResponse.Success("ok", 200u).isSuccess())
    }
    test("ApiResponse.isSuccess (false)") {
        assert(!ApiResponse.Error("fail").isSuccess())
    }
    test("ApiResponse.statusCode") {
        assert(ApiResponse.Success("ok", 200u).statusCode() == 200u)
    }

    // ─── Renamed types ───
    println("\nRenamed types:")
    test("RenamedPoint") {
        val p = RenamedPoint(1.0, 2.0)
        assert(p.x == 1.0 && p.y == 2.0)
    }
    test("RenamedStatus") {
        assert(RenamedStatus.ACTIVE == RenamedStatus.ACTIVE)
    }
    test("Config renamed fields") {
        val c = Config("test", 42)
        assert(c.configName == "test" && c.configValue == 42)
    }
    test("calculateSum") {
        assert(calculateSum(3, 4) == 7)
    }

    // ─── Object methods ───
    println("\nObject methods:")
    test("Calculator.add") {
        assert(Calculator(10.0).add(5.0) == 15.0)
    }
    test("Calculator.getValue") {
        val c = Calculator(10.0)
        c.add(5.0)
        assert(c.getValue() == 15.0)
    }
    test("InternalCalc.compute (renamed)") {
        assert(InternalCalc(10.0).compute(5.0) == 15.0)
    }
    test("InternalCalc.result (renamed)") {
        val c = InternalCalc(10.0)
        c.compute(5.0)
        assert(c.result() == 15.0)
    }

    // ─── Ord trait / compareTo ───
    println("\nOrd trait (compareTo):")
    test("UserProfile compareTo (age ordering)") {
        val u1 = UserProfile("Alice", 25u)
        val u2 = UserProfile("Bob", 30u)
        assert(u1 < u2, "Alice (25) should be less than Bob (30)")
    }
    test("UserProfile compareTo (same age, name ordering)") {
        val u1 = UserProfile("Alice", 25u)
        val u2 = UserProfile("Bob", 25u)
        assert(u1 < u2, "Alice should be less than Bob when same age")
    }
    test("UserProfile compareTo (equal)") {
        val u1 = UserProfile("Alice", 25u)
        val u2 = UserProfile("Alice", 25u)
        assert(u1.compareTo(u2) == 0, "Equal profiles should have compareTo == 0")
    }

    // ─── uniffiIsDestroyed ───
    println("\nuniffiIsDestroyed:")
    test("uniffiIsDestroyed false for new object") {
        val calc = Calculator(10.0)
        assert(!calc.uniffiIsDestroyed, "New object should not be destroyed")
        calc.close()
    }
    test("uniffiIsDestroyed true after close") {
        val calc = Calculator(10.0)
        calc.close()
        assert(calc.uniffiIsDestroyed, "Object should be destroyed after close()")
    }

    // ─── Trait: Rust-only ───
    println("\nTrait: Rust-only:")
    test("Logger.log") { getLogger().log("test") }
    test("Logger.level") { assert(getLogger().level() == 1u) }

    // ─── Trait: explicit Rust-only ───
    println("\nTrait: explicit Rust-only:")
    test("Formatter.format") {
        assert(getFormatter().format("hello") == """{"data": "hello"}""")
    }

    // ─── Trait: Rust + foreign ───
    println("\nTrait: Rust + foreign:")
    test("EventHandler from Rust") {
        assert(processEvent(getEventHandler(), "click") == "handled: click")
    }
    test("EventHandler from Kotlin (callback)") {
        val handler = object : EventHandler {
            override fun onEvent(eventName: String, data: String) {}
            override fun shouldHandle(eventName: String) = eventName == "important"
        }
        assert(processEvent(handler, "important") == "handled: important")
        assert(processEvent(handler, "trivial") == "skipped: trivial")
    }

    // ─── Trait: foreign-only (callback) ───
    println("\nTrait: foreign-only (callback):")
    test("DataStore Kotlin implementation") {
        val store = object : DataStore {
            private val m = mutableMapOf<String, String>()
            override fun get(key: String) = m[key]
            override fun set(key: String, value: String) { m[key] = value }
            override fun hasKey(key: String) = m.containsKey(key)
        }
        store.set("name", "Alice")
        assert(useDataStore(store, "name") == "Alice")
    }
    test("DataStore hasKey") {
        val store = object : DataStore {
            private val m = mutableMapOf<String, String>()
            override fun get(key: String) = m[key]
            override fun set(key: String, value: String) { m[key] = value }
            override fun hasKey(key: String) = m.containsKey(key)
        }
        store.set("k", "v")
        assert(store.hasKey("k") && !store.hasKey("x"))
    }

    // ─── Top-level functions ───
    println("\nTop-level functions:")
    test("createPoint") {
        val p = createPoint(1.0, 2.0)
        assert(p.x == 1.0 && p.y == 2.0)
    }
    test("pointDistance") {
        assert(pointDistance(createPoint(0.0, 0.0), createPoint(3.0, 4.0)) == 5.0)
    }

    // ─── Summary ───
    println("\n=== Results: $passed passed, $failed failed ===")
    if (failed > 0) {
        System.exit(1)
    }
}
'''


def run_tests(project_dir, cdylib_path):
    """Run the Kotlin runtime tests."""
    print("\n[5/6] Running Kotlin runtime tests...")
    result = run(
        ["./gradlew", "run", f"-Dcdylib.path={cdylib_path.parent}"],
        cwd=project_dir, check=False, capture=True
    )
    print(result.stdout)
    if result.stderr:
        print(result.stderr)
    return result.returncode == 0


def main():
    parser = argparse.ArgumentParser(description="Gobley runtime integration test")
    parser.add_argument("--fixture", default="tests/uniffi/record-enum-methods",
                        help="Path to fixture crate (relative to project root)")
    parser.add_argument("--bindgen", default=None,
                        help="Path to pre-built bindgen binary")
    parser.add_argument("--keep", action="store_true",
                        help="Keep temporary project after test")
    parser.add_argument("--project-root", default=None,
                        help="Gobley project root (auto-detected if not set)")
    args = parser.parse_args()

    project_root = Path(args.project_root) if args.project_root else find_project_root()
    print(f"Project root: {project_root}")

    fixture_crate = args.fixture
    fixture_path = project_root / fixture_crate
    if not fixture_path.exists():
        print(f"ERROR: Fixture path not found: {fixture_path}")
        sys.exit(1)

    # Read crate name from Cargo.toml
    with open(fixture_path / "Cargo.toml") as f:
        for line in f:
            if line.strip().startswith("name"):
                fixture_name = line.split("=")[1].strip().strip('"')
                break
        else:
            print("ERROR: Cannot find crate name in Cargo.toml")
            sys.exit(1)

    gradlew = project_root / "gradlew"
    if not gradlew.exists():
        print(f"ERROR: gradlew not found at {gradlew}")
        sys.exit(1)

    # Step 1: Build or locate bindgen
    if args.bindgen:
        bindgen = Path(args.bindgen)
        if not bindgen.exists():
            print(f"ERROR: Bindgen not found at {bindgen}")
            sys.exit(1)
        print(f"\n[1/6] Using pre-built bindgen: {bindgen}")
    else:
        bindgen = build_bindgen(project_root)

    # Step 2: Build fixture
    cdylib = build_fixture(project_root, fixture_name, fixture_path)

    # Step 3: Generate bindings
    tmp = Path(tempfile.mkdtemp(prefix="gobley-test-"))
    bindings_dir = tmp / "bindings"
    flat_config = tmp / "config.toml"
    flat_config.write_text(
        'package_name = "io.github.holdxen.svnexus.rust"\n'
        'kotlin_multiplatform = false\n'
        'kotlin_targets = ["jvm"]\n'
    )
    generate_bindings(bindgen, cdylib, fixture_path, flat_config, bindings_dir)

    # Step 4: Create Kotlin project
    project_dir = create_kotlin_project(tmp / "project", bindings_dir, cdylib, gradlew)

    # Step 5: Run tests
    success = run_tests(project_dir, cdylib)

    # Step 6: Cleanup
    if args.keep:
        print(f"\n[6/6] Project kept at: {project_dir}")
    else:
        print(f"\n[6/6] Cleaning up {tmp}...")
        shutil.rmtree(tmp, ignore_errors=True)

    if success:
        print("\n✅ All runtime tests passed!")
        return 0
    else:
        print("\n❌ Some runtime tests failed!")
        return 1


if __name__ == "__main__":
    sys.exit(main())
