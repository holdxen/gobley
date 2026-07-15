# 对 Gobley 库的深度理解

## 这个库要解决什么问题

UniFFI 是 Mozilla 开发的 Rust FFI 框架，能从 Rust 代码生成各语言的绑定。但 UniFFI 的官方 Kotlin 后端只面向 Kotlin/JVM 单平台——它生成一个完整的 `.kt` 文件，直接通过 JNA 调用 Rust cdylib。

Gobley 要解决的核心问题是：**让 UniFFI 生成的绑定能在 Kotlin Multiplatform（KMP）项目中使用**，即同一份 Rust 代码的绑定能同时跑在 Android、JVM 桌面、和 Kotlin/Native（iOS 等）上。

这不是简单的「改改模板」就行。KMP 的 source set 结构要求把代码拆成 `commonMain`（平台无关）和 `jvmMain`/`nativeMain` 等平台 source set，而 UniFFI 原生的单文件输出完全不适应这种结构。

## 核心设计思路：expect/actual 拆分

Gobley 最关键的设计决策是把 UniFFI 的绑定输出拆成两层：

- **commonMain**：只包含声明，不含任何平台特定调用。使用 Kotlin 的 `expect` 关键字标记。
- **平台 source set（jvmMain/nativeMain/androidMain）**：包含 `actual` 实现，引用平台特定的 FFI 机制（JVM 用 JNA，Native 用 C interop）。

这个拆分的分界线就是 **FFI 调用**。凡是调用 `UniffiLib.xxx()` 的代码必须出现在平台 source set 中，因为 `UniffiLib` 的定义方式因平台而异：
- JVM/Android：`internal object UniffiLib : Library`（JNA）
- Native：通过 C interop `.def` 文件链接

这个设计直接决定了模板的组织方式——同一个类型（如 Object）需要两个模板：`common/ObjectTemplate.kt` 生成 `expect` 声明，`ffi/ObjectTemplate.kt` 生成 `actual` 实现。

## 模板系统的精妙与脆弱

### 精妙之处

Gobley 用 Askama（Rust 的 Jinja2 模板引擎）实现了模板系统。模板分为 5 个目录：

```
common/     → commonMain（expect 声明，无 FFI）
android+jvm/ → JVM/Android 平台（JNA 实现）
native/      → Kotlin/Native 平台（C interop 实现）
ffi/         → FFI 层共享代码（被平台模板 include）
stub/        → 不支持平台的桩实现
```

`ffi/` 目录是个巧妙的抽象——它包含的模板（如 `ffi/RecordTemplate.kt`、`ffi/ObjectTemplate.kt`）被 `android+jvm/Types.kt` 和 `native/Types.kt` 共同 include，避免了 JVM 和 Native 之间的代码重复。

宏系统（`macros.kt`）提供了三个关键宏：
- `func_decl` — 只生成函数签名（用于 common 的 expect 声明）
- `func_decl_with_body` — 生成含 FFI 调用的函数体（用于平台 actual 实现）
- `func_decl_with_stub` — 生成 `TODO()` 桩（用于 stub source set）

这套宏系统是 expect/actual 拆分的基石——同一个方法签名，在 common 中用 `func_decl` 生成声明，在平台中用 `func_decl_with_body` 生成实现。

### 脆弱之处

模板系统的脆弱性在于 **common 模板中绝不能出现 FFI 调用**，但模板本身没有机制阻止这件事。我们刚刚修复的 Record 方法 bug 和尚未修复的 Enum 方法 bug，根源都是同一个错误：在 `common/` 模板中误用了 `func_decl_with_body`（会生成 `UniffiLib.xxx()` 调用），而 `commonMain` 看不到 `UniffiLib`。

这种错误不会在 Rust 编译期被发现（模板只是字符串生成），也不会在 bindgen 单元测试中被发现（测试只检查生成的字符串是否包含某些内容，不检查 Kotlin 语义），只有当用户在真实 KMP 项目中编译时才会暴露。

## Object 的完整 expect/actual vs Record 的扩展函数方案

### Object 类型：完整的 expect/actual

Object（`#[derive(uniffi::Object)]`）是引用类型，需要 handle 管理和生命周期控制。Gobley 为它生成了完整的 expect/actual 结构：

- **common**：`expect open class Foo` + `interface FooInterface`（方法声明）
- **平台**：`actual open class Foo`（含 handle 字段、引用计数、cleaner、方法体）

这个模式能工作是因为 Kotlin 允许 `expect class` 和 `actual class`。Object 是 class，可以这样做。

### Record 类型：不能用 expect/actual

Record（`#[derive(uniffi::Record)]`）是值类型，映射为 Kotlin 的 `data class`。我们最初尝试用 `expect data class`，但 Kotlin 编译器直接报错：

```
Modifier 'expect' is incompatible with 'data'.
Expected class constructor cannot have a property parameter.
```

Kotlin 语言层面禁止了 `expect data class`——data class 的主构造参数是它定义的一部分，不能只声明不实现。

最终采用的方案是 **扩展函数**：
- **common**：具体的 `data class RecordData(...)`（无方法，纯数据）
- **平台**：`fun RecordData.debug(): String { UniffiLib.xxx() }`（扩展函数，含 FFI 调用）

这个方案优雅地绕过了 Kotlin 的限制。data class 留在 commonMain 不需要 expect/actual，方法作为扩展函数放到平台 source set，用户调用时 `recordData.debug()` 的语法体验与成员方法完全一致。

### Enum 类型：同样的困境，尚未修复

Enum（`#[derive(uniffi::Enum)]`）的情况与 Record 类似。`enum class` 和 `sealed class` 都不能是 `expect` 的（Kotlin 限制）。但 `common/EnumTemplate.kt` 目前还在用 `func_decl_with_body` 生成方法体——这跟修复前的 Record bug 一模一样。需要同样的扩展函数修复。

## Gradle 插件：让一切「just work」

如果没有 Gradle 插件，用户需要手动执行：编译 Rust → 运行 bindgen → 把生成的文件放到正确的 source set → 配置 JNA/C interop。Gobley 的 Gradle 插件把这整个流程自动化了。

### 配置合并的巧妙设计

`MergeUniffiConfigTask` 做了一件不太显眼但很重要的事：它把用户的 `uniffi.toml`（TOML 格式，有 `[bindings.kotlin]` 段）和 Gradle 属性（如 `kotlin_multiplatform`、`kotlin_targets`）合并成一个**扁平的 TOML 文件**（没有 section header），然后传给 bindgen。

这个设计意味着 bindgen 不需要知道 Gradle 的存在——它只读一个普通的 TOML 配置文件。Gradle 侧和 bindgen 侧通过这个合并后的 TOML 解耦。`Config.kt`（Gradle 侧）和 `Config` struct（Rust 侧，`mod.rs:100`）通过 `@SerialName` 注解保持字段名同步。

### bindgen 安装的灵活性

`UniFfiExtension` 提供了 4 种 bindgen 来源：

```kotlin
bindgenFromRegistry("gobley-uniffi-bindgen", "0.3.8")  // crates.io
bindgenFromPath(layout.projectDirectory)                // 本地路径
bindgenFromGitBranch("https://github.com/.../gobley.git", "main")  // Git 分支
bindgenFromGitTag("https://github.com/.../gobley.git", "v0.3.8")   // Git tag
bindgenFromGitRevision("https://github.com/.../gobley.git", "abc123")  // Git commit
```

这个设计让用户可以轻松使用自己 fork 的 bindgen——正是 SVNexus 项目的做法（`bindgenFromGitBranch("https://github.com/holdxen/gobley.git", "main")`）。

### Source set 集成

`UniFfiPlugin.kt:419` 把生成的绑定文件注册到对应的 Kotlin source set：

```kotlin
with(kotlinExtensionDelegate.sourceSets.commonMain) {
    kotlin.srcDir(commonBindingsDirectory)  // commonMain/kotlin/
}
with(kotlinExtensionDelegate.sourceSets.jvmMain) {
    kotlin.srcDir(jvmBindingsDirectory)     // jvmMain/kotlin/
}
```

对于 Native，还需要额外处理 C interop：生成的 `.h` 头文件被注册到 `cinterop` 定义中，让 Kotlin/Native 编译器知道 Rust 导出的 C 符号。

## Object 的 handle 管理机制

Gobley 生成的 Object 实现了一套完整的 handle 生命周期管理（`ffi/ObjectTemplate.kt`），这是整个项目中最精密的部分：

```
Kotlin Object 实例
    │
    ├── handle: Long          ── Rust 端的 Object 指针
    ├── cleanable: Cleanable? ── 绑定到 Cleaner 的清理回调
    ├── callCounter: AtomicLong ── 引用计数（初始值 1）
    │
    ├── callWithHandle { block ->
    │     // CAS 循环递增 callCounter，防止并发销毁
    │     // 执行 block(uniffiCloneHandle())
    │     // 递减 callCounter，若为 0 则触发 clean
    │ }
    │
    ├── uniffiCloneHandle()  ── 调用 Rust 的 clone 函数，返回新 handle
    │
    └── destroy()            ── 标记销毁，递减计数，最终调用 Rust free
```

这套机制确保了：
1. **线程安全**：用 CAS（compare-and-set）循环避免并发问题
2. **不会提前释放**：方法调用期间 handle 不会被释放
3. **自动清理**：通过 `Cleaner`（JVM）或手动 `close()` 回收 Rust 端内存
4. **防重入**：`wasDestroyed` 标志防止多次销毁

## UniFFI 0.29 → 0.32 升级的阵痛

这个 fork 的 Git 历史清晰地展示了 uniffi 0.29→0.32 升级带来的挑战：

```
d47f41c chore: upgrade uniffi from 0.29.5 to 0.32.0
29ab478 fix: Pointer→Long migration, Type::Box/Set mapping
742d511 fix: expect/actual constructor mismatch, VTable uniffiClone
b0bbc88 fix: support HashSet<T> and NoHandle for uniffi 0.32
5c2342a fix: SetTemplate.kt uses sumOf for allocationSize
29924a8 feat: add record/enum methods support (uniffi 0.32 feature)
81623a4 fix: record methods should not be in common source set  ← 删了方法但没补回
479cd26 fix: use extension functions for record methods in KMP  ← 最终修复
```

uniffi 0.32 引入了几个新特性，Gobley 需要适配：
- **Record 方法**：`#[uniffi::export] impl MyRecord { fn method(&self) }`
- **Enum 方法**：`#[uniffi::export] impl MyEnum { fn method(&self) }`
- **`uniffi_trait_methods`**：Rust 端的 `Display`/`Eq`/`Hash`/`Ord` trait 实现映射到 Kotlin
- **Pointer → Long**：FFI handle 类型从 `Pointer` 改为 `Long`
- **`Type::Box`/`Type::Set`**：新的类型变体

Record 方法的修复历程最能说明 KMP 适配的难度：第一次尝试把方法直接放 common（编译失败），第二次尝试删掉方法（功能丢失），第三次用 expect data class（Kotlin 不允许），最终用扩展函数才正确解决。每一步都需要同时理解 UniFFI 的元数据模型、Kotlin 的语言限制、和 KMP 的 source set 结构。

## 代码量分布的启示

| 部分 | 行数 | 说明 |
|---|---|---|
| Kotlin 模板 | ~4,170 | 绑定生成的核心逻辑 |
| Rust 代码 | ~3,819 | bindgen 框架 + CodeType + CodeOracle |
| Gradle 插件 | ~8,375 | 构建自动化 + DSL + Task |

Gradle 插件的代码量是模板和 Rust 代码的两倍多。这反映了一个现实：**让绑定生成器在各种项目中可用，所需的基础设施代码远比绑定生成本身多**。Cargo 构建管理、交叉编译、配置合并、source set 集成、动态库查找、ProGuard 规则生成——每一项都是独立的工程挑战。

## 我认为的优雅之处

1. **expect/actual 分界线**：用「是否包含 FFI 调用」作为 common 和平台的分界线，清晰且可操作。

2. **扩展函数绕过 Kotlin 限制**：Record 方法用扩展函数而非 expect/actual 解决，是一个简洁的工程妥协。

3. **配置合并解耦**：Gradle 侧和 bindgen 侧通过合并的 TOML 文件解耦，bindgen 不依赖 Gradle。

4. **`ffi/` 共享模板**：JVM 和 Native 的 FFI 层代码大量复用，通过 `ffi/` 目录共享。

5. **bindgen 安装灵活性**：4 种来源（registry/path/git/tag/revision），让 fork 和定制变得自然。

## 我认为的脆弱之处

1. **模板中无 FFI 调用检查**：common 模板误用 `func_decl_with_body` 不会被任何工具发现，直到用户编译。Record 和 Enum 都踩了这个坑。

2. **uniffi_trait_methods 缺失**：上游 0.32 已支持将 Rust trait 实现映射到 Kotlin，但 Gobley 完全没实现。Object 有 `uniffi_traits()` 支持，Record 和 Enum 没有。

3. **bindgen 版本与 Gradle 插件版本独立**：用户可以混用不同版本的 Gradle 插件和 bindgen（如 SVNexus 用 0.3.7 的 Gradle 插件 + fork 的 bindgen），如果两者对配置格式的理解不一致，会产生难以调试的问题。

4. **`lib.rs` 中的 `unwrap()`**：文件 I/O 用 `unwrap()` 处理错误，在磁盘异常时会 panic 而非返回有意义的错误。

5. **测试不验证 Kotlin 语义**：bindgen 的 81 个单元测试只检查生成的字符串是否包含特定内容，不验证生成的 Kotlin 代码能否编译。这导致 expect/actual 不匹配、KMP 限制违反等问题只能在用户项目中暴露。
