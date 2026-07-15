# Gobley 代码架构文档

## 1. 项目概述

Gobley 是一组库和工具，用于通过 [UniFFI](https://github.com/mozilla/uniffi-rs) 将 Rust 与 Kotlin Multiplatform 混合使用。项目从 [UniFFI Kotlin Multiplatform bindings](https://gitlab.com/trixnity/uniffi-kotlin-multiplatform-bindings) fork 而来。

支持平台：Android、Kotlin/JVM、Kotlin/Native。WASM 尚未支持。

## 2. 顶层目录结构

```
gobley/
├── crates/                      # Rust crates（绑定生成器 + WASM 转换器）
│   ├── gobley-uniffi-bindgen/   # Kotlin Multiplatform 绑定生成器（核心）
│   └── gobley-wasm-transformer/ # WASM 模块转换器（实验性，用于 Kotlin/JS）
├── build-logic/                 # Gradle 插件（Kotlin DSL）
│   ├── gobley-gradle/           # 基础设施插件（宿主信息、Rust 目标、Android 适配）
│   ├── gobley-gradle-cargo/     # Cargo 构建插件（编译 Rust、管理交叉编译）
│   ├── gobley-gradle-rust/      # Rust 工具链插件（rustup、目标管理）
│   ├── gobley-gradle-uniffi/    # UniFFI 绑定生成插件（配置合并、bindgen 安装、代码生成）
│   ├── gobley-gradle-build/     # 构建辅助插件
│   └── conventions/             # Gradle 约定脚本（测试配置）
├── tests/                       # 测试套件
│   ├── gradle/                  # Gradle 集成测试
│   └── uniffi/                  # UniFFI 功能测试（20+ 测试 crate）
├── examples/                    # 示例项目
├── Cargo.toml                   # Rust workspace 根配置
├── settings.gradle.kts          # Gradle 根设置
└── build.gradle.kts             # Gradle 根构建脚本
```

## 3. Rust Crates 架构

### 3.1 `gobley-uniffi-bindgen`（绑定生成器）

这是项目的核心，负责从 Rust 编译产物（cdylib）生成 Kotlin 绑定代码。

**入口点：**
- `src/main.rs` — CLI 入口，支持 `--library` 模式（从 cdylib 提取元数据）和 UDL 模式
- `src/lib.rs` — `KotlinBindingGenerator` 实现 `uniffi_bindgen::BindingGenerator` trait

**代码生成管线：**

```
cdylib / UDL
    │
    ▼
uniffi_bindgen::library_mode::generate_bindings()
    │  (从 cdylib 提取元数据 + 解析 UDL)
    ▼
ComponentInterface (uniffi_bindgen 提供)
    │  (包含 records, enums, objects, functions, callback interfaces)
    ▼
generate_bindings(config, ci)  ──  gen_kotlin_multiplatform/mod.rs:280
    │
    ├──► CommonKotlinWrapper   ──► commonMain 源集（expect 声明）
    ├──► AndroidJvmKotlinWrapper ──► androidMain / jvmMain 源集（actual 实现）
    ├──► NativeKotlinWrapper   ──► nativeMain 源集（actual 实现）
    ├──► StubKotlinWrapper     ──► stub 源集（TODO() 桩）
    └──► HeadersKotlinWrapper  ──► Kotlin/Native C interop 头文件（.h）
```

**模板系统（Askama）：**

模板使用 [Askama](https://github.com/djc/askama)（Jinja2 风格的 Rust 模板引擎），位于 `src/templates/`：

| 目录 | 用途 | 输出位置 |
|---|---|---|
| `common/` | 公共声明（expect 类/函数） | `commonMain/kotlin/` |
| `android+jvm/` | JVM/Android 平台实现 | `jvmMain/kotlin/` 或 `androidMain/kotlin/` |
| `native/` | Kotlin/Native 平台实现 | `nativeMain/kotlin/` |
| `ffi/` | FFI 层代码（FfiConverter、FFI Helper） | 各平台 source set |
| `stub/` | 桩实现（用于不支持的平台） | `stubMain/kotlin/` |
| `headers/` | C interop 头文件 | `nativeInterop/cinterop/` |
| `macros.kt` | 共享宏（func_decl、func_decl_with_body 等） | 被各模板 include |

**关键模板文件：**

| 模板 | 功能 |
|---|---|
| `common/Types.kt` | 类型分发器，遍历所有类型并 include 对应模板 |
| `common/RecordTemplate.kt` | Record（data class）生成 |
| `common/EnumTemplate.kt` | Enum（enum class / sealed class）生成 |
| `common/ObjectTemplate.kt` | Object（interface + expect class）生成 |
| `common/TopLevelFunctionTemplate.kt` | 顶层函数生成 |
| `ffi/ObjectTemplate.kt` | Object 的 actual 实现（含 handle 管理、FFI 调用） |
| `ffi/RecordTemplate.kt` | Record 的 FFI 层（FfiConverter + 扩展函数） |
| `ffi/EnumTemplate.kt` | Enum 的 FFI 层（FfiConverter） |
| `android+jvm/NamespaceLibraryTemplate.kt` | UniffiLib 接口定义（JNA） |

**CodeType 系统：**

每种 UniFFI 类型（Record、Enum、Object、CallbackInterface、Custom、primitives、compounds）都有对应的 `CodeType` 实现，位于 `src/gen_kotlin_multiplatform/`：

- `record.rs` — `RecordCodeType`
- `enum_.rs` — `EnumCodeType`
- `object.rs` — `ObjectCodeType`
- `callback_interface.rs` — `CallbackInterfaceCodeType`
- `custom.rs` — `CustomCodeType`
- `primitives.rs` — 基本类型（Int32, String, Boolean 等）
- `compounds.rs` — 复合类型（Optional, Sequence, Map, Set）
- `variant.rs` — Enum 变体
- `miscellany.rs` — 杂项类型

**KotlinCodeOracle：**

`KotlinCodeOracle`（`mod.rs:619`）是名称转换中心，负责将 Rust 名称转换为 Kotlin 惯用名称：
- `class_name` — UpperCamelCase
- `fn_name` — lowerCamelCase（带反引号转义关键字）
- `var_name` — lowerCamelCase（带反引号）
- `enum_variant_name` — ShoutySnakeCase 或 PascalCase（可配置）

**Config（`mod.rs:100`）：**

配置与 Gradle 插件的 `Config.kt`（`build-logic/gobley-gradle-uniffi/src/main/kotlin/Config.kt`）通过 TOML 序列化同步。关键字段：
- `kotlin_multiplatform` — 是否启用 KMP 模式（影响 expect/actual 生成）
- `kotlin_targets` — 目标平台列表（jvm, android, native, stub）
- `generate_immutable_records` — Record 使用 `val` 还是 `var`
- `generate_serializable_types` — 是否生成 `@Serializable` 注解
- `use_pascal_case_enum_class` — Enum 变量命名风格

### 3.2 `gobley-wasm-transformer`（WASM 转换器）

实验性 crate，用于将 Rust 编译的 WASM 模块转换为 Kotlin/JS 可用的形式。

- 使用 `walrus` 库解析和修改 WASM 模块
- 注入栈指针 shim 和函数导入
- 处理 `wasm_bindgen` 生成的 JS 模块
- 通过 Askama 模板 `templates/js.kt` 生成 Kotlin/JS 绑定

## 4. Gradle 插件架构

### 4.1 插件依赖关系

```
用户项目
    │
    ├── apply("dev.gobley.cargo")    ── CargoPlugin
    ├── apply("dev.gobley.uniffi")   ── UniFfiPlugin
    │
    ▼
gobley-gradle (基础库)
    │
    ├── GobleyHost           ── 当前宿主平台信息
    ├── RustTarget           ── Rust 交叉编译目标
    ├── Variant              ── Debug/Release 变体
    └── Kotlin/Android 适配层
```

### 4.2 `gobley-gradle-cargo`（Cargo 构建插件）

**核心职责：** 管理 Rust 项目的 Cargo 构建。

**DSL 层（`dsl/`）：**
- `CargoExtension` — 顶层扩展，用户配置入口
- `CargoBuild` / `CargoBuildVariant` — 构建配置（目标平台、profile、feature）
  - `CargoJvmBuild` — JVM 构建（embedRustLibrary）
  - `CargoAndroidBuild` — Android 构建（多个 ABI）
  - `CargoNativeBuild` — Kotlin/Native 构建
  - `CargoWasmBuild` — WASM 构建
- `CargoBinaryCrateSource` — bindgen 二进制 crate 来源（registry / git / path）

**Task 层（`tasks/`）：**
- `CargoBuildTask` — 执行 `cargo build`
- `CargoCheckTask` — 执行 `cargo check`
- `CargoCleanTask` — 执行 `cargo clean`
- `CargoInstallTask` — 执行 `cargo install`（用于安装 bindgen）
- `RustUpTargetAddTask` — 添加交叉编译目标
- `FindDynamicLibrariesTask` — 查找动态库依赖
- `TransformWasmTask` — WASM 转换

### 4.3 `gobley-gradle-uniffi`（UniFFI 绑定生成插件）

**核心职责：** 协调绑定生成流程。

**工作流程（`UniFfiPlugin.kt`）：**

```
1. mergeUniffiConfig     ── 合并 uniffi.toml + Gradle 配置 → 合并后的 TOML
2. installUniffiBindgen  ── cargo install gobley-uniffi-bindgen（从 git/registry/path）
3. cargoBuild            ── 编译 Rust cdylib
4. buildUniffiBindings   ── 运行 bindgen 生成 Kotlin 绑定
5. Kotlin 编译            ── 编译生成的绑定 + 用户 Kotlin 代码
```

**Task 层：**
- `MergeUniffiConfigTask` — 合并用户 `uniffi.toml` 与 Gradle 属性，输出扁平 TOML
- `InstallUniffiBindgenTask` — 通过 `cargo install --git` 安装 bindgen 二进制
- `BuildUniffiBindingsTask` — 调用 bindgen CLI 生成绑定
- `GenerateUniffiProguardRulesTask` — 生成 Android ProGuard 规则

**Source Set 集成（`UniFfiPlugin.kt:419`）：**
- `commonMain` ← `commonMain/kotlin/`
- `jvmMain` ← `jvmMain/kotlin/`
- `androidMain` ← `androidMain/kotlin/`
- `nativeMain` ← `nativeMain/kotlin/`
- Native C interop ← `nativeInterop/cinterop/`

### 4.4 `gobley-gradle`（基础库）

提供跨插件共享的基础设施：

- `GobleyHost` — 当前宿主平台检测（OS、架构、Rust target）
- `RustTarget` 体系 — Rust 交叉编译目标抽象（`rust/targets/`）
  - `RustAndroidTarget` — Android ABI
  - `RustAppleMobileTarget` — iOS / watchOS / tvOS
  - `RustDesktopTarget` — 桌面平台
  - `RustJvmTarget` — JVM 目标
  - `RustNativeTarget` — Kotlin/Native 目标
  - `RustWasmTarget` — WASM 目标
- `GobleyKotlinExtensionDelegate` — Kotlin Gradle Plugin 适配层
  - 支持 KMP、Android、JVM 三种模式
- `GobleyAndroidExtensionDelegate` — Android Gradle Plugin 适配层
- 工具类：`CommandTask`、`GloballyLockedTask`、`PathList` 等

## 5. KMP expect/actual 模式

Gobley 的核心设计是利用 Kotlin Multiplatform 的 expect/actual 机制分离平台无关声明与平台特定实现：

| Source Set | 内容 | 示例 |
|---|---|---|
| `commonMain` | `expect` 声明（无 FFI 调用） | `expect fun add(...): Int` |
| `jvmMain` | `actual` 实现（JNA 调用 `UniffiLib`） | `actual fun add(...) = UniffiLib.xxx(...)` |
| `nativeMain` | `actual` 实现（C interop） | `actual fun add(...) = UniffiLib.xxx(...)` |

**Object 类型**使用完整的 expect/actual 模式：
- common: `expect open class Foo` + `interface FooInterface`（方法声明）
- ffi: `actual open class Foo`（方法体含 `UniffiLib` 调用、handle 管理）

**Record 类型**（修复后）：
- common: 具体 `data class`（无方法）
- ffi: 扩展函数 `fun RecordData.debug()` + `FfiConverter`

**Enum 类型**：
- common: 具体 `enum class` / `sealed class`（**当前有 bug，见下文**）

## 6. 测试体系

测试通过 Gradle 属性控制是否包含（`settings.gradle.kts`）：

| 属性 | 包含的测试 |
|---|---|
| `gobley.projects.gradleTests` | Gradle 集成测试（android-linking, cargo-only, jvm-only, js-only） |
| `gobley.projects.uniffiTests` | UniFFI 功能测试（20+ crate） |
| `gobley.projects.uniffiTests.extTypes` | 外部类型测试 |
| `gobley.projects.uniffiTests.futures` | 异步 Future 测试 |
| `gobley.projects.examples.*` | 示例项目 |

关键测试 crate：
- `coverall` — 覆盖性测试（所有 UniFFI 特性）
- `proc-macro` — proc-macro 模式测试
- `trait-record-enum` — Record/Enum 带 trait 方法测试
- `ext-types` — 跨 crate 外部类型测试
- `futures` — 异步函数测试

## 7. 已发现的 Bug 和缺失

### 7.1 [BUG] Enum 方法在 KMP 模式下会编译失败

**严重程度：** 高（与已修复的 Record 方法 bug 同类）

**位置：** `crates/gobley-uniffi-bindgen/src/templates/common/EnumTemplate.kt:24,38,85`

**问题：** `common/EnumTemplate.kt` 使用 `func_decl_with_body` 生成 enum 方法，该宏会生成包含 `UniffiLib.xxx()` FFI 调用的方法体。在 KMP 模式下，`commonMain` 看不到 `UniffiLib`（它是 `jvmMain`/`nativeMain` 中的平台特定声明），会导致编译失败。

**复现场景：** 定义带有方法的 enum：
```rust
#[derive(uniffi::Enum)]
enum Color { Red, Green, Blue }

#[uniffi::export]
impl Color {
    fn name(&self) -> String { format!("{:?}", self) }
}
```
在 KMP 项目中编译会失败，错误为 `Unresolved reference: UniffiLib`。

**对比 Record 修复：** Record 方法已修复（commit `479cd26`），使用扩展函数方案：common 中保持纯 `data class`，平台模板生成扩展函数 `fun Color.name(): String { UniffiLib.xxx() }`。Enum 需要同样的修复。

**修复方向：**
1. `common/EnumTemplate.kt`：用 `!config.kotlin_multiplatform` 守卫，KMP 模式下不生成方法体
2. `ffi/EnumTemplate.kt`：KMP 模式下生成扩展函数

### 7.2 [缺失] Record 的 `uniffi_trait_methods` 未生成

**严重程度：** 中

**位置：** `crates/gobley-uniffi-bindgen/src/templates/common/RecordTemplate.kt` 和 `ffi/RecordTemplate.kt`

**问题：** 上游 uniffi 0.32 的 `RecordTemplate.kt` 会为实现了 Rust `Display`、`Eq`、`Hash`、`Ord` trait 的 Record 生成对应的 Kotlin 方法：
- `Display` → `override fun toString(): String`
- `Eq` → `override fun equals(other: Any?): Boolean`
- `Hash` → `override fun hashCode(): Int`
- `Ord` → `override fun compareTo(other: T): Int` + `Comparable<T>` 接口

Gobley 的模板完全没有使用 `rec.uniffi_trait_methods()` API，也不包含 `uniffi_trait_impls` 宏。这意味着 Rust 端的 `impl Display for MyRecord` 不会反映到 Kotlin 端。

**对比：** Object 类型已正确支持 `uniffi_traits()`（`ffi/ObjectTemplate.kt:138`），但 Record 和 Enum 未支持。

### 7.3 [缺失] `uniffi_trait_impls` 宏缺失

**严重程度：** 中

**位置：** `crates/gobley-uniffi-bindgen/src/templates/macros.kt`

**问题：** 上游 `macros.kt` 包含 `uniffi_trait_impls` 宏（`macros.kt:188`），用于从 `UniffiTraitMethods` 生成 `toString()`、`equals()`、`hashCode()`、`compareTo()`。Gobley 的 `macros.kt` 没有这个宏。

### 7.4 [代码质量] `lib.rs` 中的 `unwrap()` 调用

**严重程度：** 低

**位置：** `crates/gobley-uniffi-bindgen/src/lib.rs:113,114,121,133,135,136`

**问题：** `write_bindings_target` 和 `write_cinterop` 函数中使用了 `unwrap()` 处理文件 I/O，在磁盘空间不足或权限不足时会 panic，而非返回有意义的错误。

### 7.5 [代码质量] `header_escape_name` 死代码

**严重程度：** 低

**位置：** `crates/gobley-uniffi-bindgen/src/gen_kotlin_multiplatform/mod.rs:1391`

**问题：** `header_escape_name` 函数已定义但从未被任何模板使用，产生 `dead_code` 编译警告。可能是 headers 模板重构后遗留的。

### 7.6 [已修复] Record 方法在 KMP 模式下不生成

**状态：** 已在 commit `479cd26` 修复

**历史：** commit `81623a4` 发现 record 方法体在 `commonMain` 中会编译失败（`UniffiLib` 不可见），直接删除了方法生成但未在平台模板补回，导致 record 方法完全丢失。commit `479cd26` 使用扩展函数方案修复：common 中保持纯 data class，平台模板生成 `fun RecordData.debug()` 扩展函数。

## 8. 构建流程图

```
用户 Gradle 构建
    │
    ├─ mergeUniffiConfig
    │    └─ 合并 uniffi.toml + Gradle 属性 → 合并后的 TOML
    │
    ├─ installUniffiBindgen
    │    └─ cargo install gobley-uniffi-bindgen --git <source>
    │
    ├─ cargoBuild<target><variant>
    │    └─ cargo build --target <rust-target> → cdylib
    │
    ├─ buildUniffiBindings
    │    └─ gobley-uniffi-bindgen --library <cdylib> --config <merged.toml>
    │        │
    │        ├─ 从 cdylib 提取元数据 (macro_metadata)
    │        ├─ 解析 UDL（如有）
    │        ├─ 构建 ComponentInterface
    │        └─ generate_bindings()
    │             ├─ commonMain/kotlin/*.kt  (expect 声明)
    │             ├─ jvmMain/kotlin/*.kt     (actual 实现 + UniffiLib)
    │             ├─ androidMain/kotlin/*.kt (actual 实现)
    │             ├─ nativeMain/kotlin/*.kt  (actual 实现)
    │             └─ nativeInterop/cinterop/*.h (C interop 头文件)
    │
    └─ compileKotlinJvm / compileKotlinNative
         └─ 编译生成的绑定 + 用户 Kotlin 代码
```
