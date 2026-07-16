
{#
// Template to call into rust. Used in several places.
// Variable names in `arg_list` should match up with arg lists
// passed to rust via `arg_list_lowered`
#}

{%- macro check_rust_buffer_length(length) -%}
    require({{ length }} <= Int.MAX_VALUE) {
        val length = {{ length }}
        "cannot handle RustBuffer longer than Int.MAX_VALUE bytes: length is $length"
    }
{%- endmacro %}

{%- macro to_ffi_call(func, indent) -%}
                        {%- match func.self_type() %}
                        {%- when Some(Type::Object { .. }) %}
                        callWithHandle {
{{ " "|repeat(indent) }}    {% call to_raw_ffi_call(func, indent + 4) %}{% endcall %}
{{ " "|repeat(indent) }}{{ '}' }}
                        {%- else -%}
                        {%- call to_raw_ffi_call(func, indent) -%}{% endcall %}{%- endmatch -%}
{%- endmacro %}

{%- macro to_raw_ffi_call(func, indent) -%}
                        {%- match func.throws_type() -%}
                        {%- when Some(e) -%}
                        uniffiRustCallWithError({{ e|type_name(ci) }}ErrorHandler)
                        {%- else -%}
                        uniffiRustCall
                        {%- endmatch %} { uniffiRustCallStatus ->
{{ " "|repeat(indent) }}    UniffiLib.{{ func.ffi_func().name() }}(
                                {%- match func.self_type() %}
                                {%- when Some(Type::Object { .. }) %}
{{ " "|repeat(indent) }}        it,
                                {%- when Some(t) %}
{{ " "|repeat(indent) }}        {{- t|lower_fn }}(this),
                                {%- when None %}
                                {%- endmatch -%}
                                {%- call arg_list_lowered(func, indent + 8) %}{% endcall %}
{{ " "|repeat(indent) }}        uniffiRustCallStatus,
{{ " "|repeat(indent) }}    )
                        {%- if let Some(return_type) = func.ffi_func().return_type() -%}
                        {%-     if return_type|need_non_null_assertion %}
{{ " "|repeat(indent) }}{{ '}' }}!!
                        {%-     else %}
{{ " "|repeat(indent) }}{{ '}' }}
                        {%-     endif -%}
                        {%- else %}
{{ " "|repeat(indent) }}{{ '}' }}
                        {%- endif %}
{%- endmacro -%}

{%- macro func_decl(func_decl, callable, indent, is_decl_override) %}
                        {%- call docstring(callable, indent) -%}{%- endcall %}
                        {%- match callable.throws_type() -%}
                        {%-     when Some(throwable) %}
{{ " "|repeat(indent) }}@Throws({{ throwable|type_name(ci) }}::class {%- if callable.is_async() -%}, kotlin.coroutines.cancellation.CancellationException::class{%- endif -%})
                        {%-     else -%}
                        {%- endmatch %}
{{ " "|repeat(indent) }}{{ visibility() }}{% if func_decl.len() != 0 -%}{{ func_decl }} {% endif -%}
                        {%- if callable.is_async() -%}suspend {% endif -%}
                        fun {{ callable.name()|fn_name }}(
                            {%- call arg_list(callable, is_decl_override || callable.self_type().is_none()) -%}{%- endcall %}
                        )
                        {%- match callable.return_type() -%}
                        {%-     when Some(return_type) %}: {{ return_type|type_name(ci) -}}
                        {%-     else -%}
                        {%- endmatch -%}
{% endmacro %}

{%- macro func_decl_with_body(func_decl, callable, indent) %}
                        {%- call docstring(callable, indent) -%}{%- endcall %}
                        {%- match callable.throws_type() -%}
                        {%-     when Some(throwable) %}
{{ " "|repeat(indent) }}@Throws({{ throwable|type_name(ci) }}::class {%- if callable.is_async() -%}, kotlin.coroutines.cancellation.CancellationException::class{%- endif -%})
                        {%-     else -%}
                        {%- endmatch %}
{{ " "|repeat(indent) }}{{ visibility() }}{% if func_decl.len() != 0 -%}{{ func_decl }} {% endif -%}
                        {%- if callable.is_async() -%}suspend {% endif -%}
                        fun {{ callable.name()|fn_name }}(
                            {%- call arg_list(callable, false) -%}{%- endcall %}
                        )
                        {%- match callable.return_type() -%}
                        {%-     when Some(return_type) %}: {{ return_type|type_name(ci) -}}
                        {%-     else %}
                        {%- endmatch %} {
                            {%- if callable.is_async() %}
{{ " "|repeat(indent) }}    return {% call call_async(callable, indent + 4) -%}{%- endcall %}
                            {%- else %}
                            {%- match callable.return_type() -%}
                            {%-     when Some(return_type) %}
{{ " "|repeat(indent) }}    return {{ return_type|lift_fn }}({%- call to_ffi_call(callable, indent + 4) -%}{%- endcall %})
                            {%-     else %}
{{ " "|repeat(indent) }}    {% call to_ffi_call(callable, indent + 4) -%}{%- endcall %}
                            {%- endmatch %}
                            {%- endif %}
{{ " "|repeat(indent) }}{{ '}' }}
{% endmacro %}

{%- macro func_decl_with_body_enum(func_decl, callable, indent) %}
                        {%- call docstring(callable, indent) -%}{%- endcall %}
                        {%- match callable.throws_type() -%}
                        {%-     when Some(throwable) %}
{{ " "|repeat(indent) }}@Throws({{ throwable|type_name(ci) }}::class {%- if callable.is_async() -%}, kotlin.coroutines.cancellation.CancellationException::class{%- endif -%})
                        {%-     else -%}
                        {%- endmatch %}
{{ " "|repeat(indent) }}{{ visibility() }}{% if func_decl.len() != 0 -%}{{ func_decl }} {% endif -%}
                        {%- if callable.is_async() -%}suspend {% endif -%}
                        fun {{ callable.name()|enum_fn_name }}(
                            {%- call arg_list(callable, false) -%}{%- endcall %}
                        )
                        {%- match callable.return_type() -%}
                        {%-     when Some(return_type) %}: {{ return_type|type_name(ci) -}}
                        {%-     else %}
                        {%- endmatch %} {
                            {%- if callable.is_async() %}
{{ " "|repeat(indent) }}    return {% call call_async(callable, indent + 4) -%}{%- endcall %}
                            {%- else %}
                            {%- match callable.return_type() -%}
                            {%-     when Some(return_type) %}
{{ " "|repeat(indent) }}    return {{ return_type|lift_fn }}({%- call to_ffi_call(callable, indent + 4) -%}{%- endcall %})
                            {%-     else %}
{{ " "|repeat(indent) }}    {% call to_ffi_call(callable, indent + 4) -%}{%- endcall %}
                            {%- endmatch %}
                            {%- endif %}
{{ " "|repeat(indent) }}{{ '}' }}
{% endmacro %}

{%- macro func_extension_with_body(receiver_type, callable, indent) %}
                        {%- call docstring(callable, indent) -%}{%- endcall %}
                        {%- match callable.throws_type() -%}
                        {%-     when Some(throwable) %}
{{ " "|repeat(indent) }}@Throws({{ throwable|type_name(ci) }}::class {%- if callable.is_async() -%}, kotlin.coroutines.cancellation.CancellationException::class{%- endif -%})
                        {%-     else -%}
                        {%- endmatch %}
{{ " "|repeat(indent) }}{{ visibility() }}{%- if callable.is_async() -%}suspend {% endif -%}
                        fun {{ receiver_type }}.{{ callable.name()|fn_name }}(
                            {%- call arg_list(callable, false) -%}{%- endcall %}
                        )
                        {%- match callable.return_type() -%}
                        {%-     when Some(return_type) %}: {{ return_type|type_name(ci) -}}
                        {%-     else %}
                        {%- endmatch %} {
                            {%- if callable.is_async() %}
{{ " "|repeat(indent) }}    return {% call call_async(callable, indent + 4) -%}{%- endcall %}
                            {%- else %}
                            {%- match callable.return_type() -%}
                            {%-     when Some(return_type) %}
{{ " "|repeat(indent) }}    return {{ return_type|lift_fn }}({%- call to_ffi_call(callable, indent + 4) -%}{%- endcall %})
                            {%-     else %}
{{ " "|repeat(indent) }}    {% call to_ffi_call(callable, indent + 4) -%}{%- endcall %}
                            {%- endmatch %}
                            {%- endif %}
{{ " "|repeat(indent) }}{{ '}' }}
{% endmacro %}

{%- macro func_extension_with_body_enum(receiver_type, callable, indent) %}
                        {%- call docstring(callable, indent) -%}{%- endcall %}
                        {%- match callable.throws_type() -%}
                        {%-     when Some(throwable) %}
{{ " "|repeat(indent) }}@Throws({{ throwable|type_name(ci) }}::class {%- if callable.is_async() -%}, kotlin.coroutines.cancellation.CancellationException::class{%- endif -%})
                        {%-     else -%}
                        {%- endmatch %}
{{ " "|repeat(indent) }}{{ visibility() }}{%- if callable.is_async() -%}suspend {% endif -%}
                        fun {{ receiver_type }}.{{ callable.name()|enum_fn_name }}(
                            {%- call arg_list(callable, false) -%}{%- endcall %}
                        )
                        {%- match callable.return_type() -%}
                        {%-     when Some(return_type) %}: {{ return_type|type_name(ci) -}}
                        {%-     else %}
                        {%- endmatch %} {
                            {%- if callable.is_async() %}
{{ " "|repeat(indent) }}    return {% call call_async(callable, indent + 4) -%}{%- endcall %}
                            {%- else %}
                            {%- match callable.return_type() -%}
                            {%-     when Some(return_type) %}
{{ " "|repeat(indent) }}    return {{ return_type|lift_fn }}({%- call to_ffi_call(callable, indent + 4) -%}{%- endcall %})
                            {%-     else %}
{{ " "|repeat(indent) }}    {% call to_ffi_call(callable, indent + 4) -%}{%- endcall %}
                            {%- endmatch %}
                            {%- endif %}
{{ " "|repeat(indent) }}{{ '}' }}
{% endmacro %}

{%- macro func_decl_with_stub(func_decl, callable, indent) %}
                        {%- call docstring(callable, indent) %}{% endcall %}
{{ " "|repeat(indent) }}{{ visibility() }}{% if func_decl.len() != 0 -%}{{ func_decl }} {% endif -%}
                        {%- if callable.is_async() -%}suspend {% endif -%}
                        fun {{ callable.name()|fn_name }}(
                            {%- call arg_list(callable, false) -%}{%- endcall %}
                        )
                        {%- match callable.return_type() -%}
                        {%-     when Some(return_type) %}: {{ return_type|type_name(ci) -}}
                        {%-     else -%}
                        {%- endmatch %} {
{{ " "|repeat(indent) }}    TODO()
{{ " "|repeat(indent) }}{{ '}' }}
{% endmacro %}

{%- macro call_async(callable, indent) -%}
                        uniffiRustCallAsync(
                            {%- match callable.self_type() %}
                            {%- when Some(Type::Object { .. }) %}
{{ " "|repeat(indent) }}    callWithHandle { handle ->
{{ " "|repeat(indent) }}        UniffiLib.{{ callable.ffi_func().name() }}(
{{ " "|repeat(indent) }}            handle,
                                    {%- call arg_list_lowered(callable, indent + 12) %}{% endcall %}
{{ " "|repeat(indent) }}        )
{{ " "|repeat(indent) }}    },
                            {%- else %}
{{ " "|repeat(indent) }}    UniffiLib.{{ callable.ffi_func().name() }}(
                                {%- call arg_list_lowered(callable, indent + 8) %}{% endcall %}
{{ " "|repeat(indent) }}    ),
                            {%- endmatch %}
{{ " "|repeat(indent) }}    {{ callable|async_poll(ci) }},
{{ " "|repeat(indent) }}    {{ callable|async_complete(ci) }},
{{ " "|repeat(indent) }}    {{ callable|async_free(ci) }},
{{ " "|repeat(indent) }}    {{ callable|async_cancel(ci) }},
{{ " "|repeat(indent) }}    // lift function
                            {%- match callable.return_type() -%}
                            {%- when Some(return_type) -%}
                            {%- if return_type|as_ffi_type|ref|need_non_null_assertion %}
{{ " "|repeat(indent) }}    { {{ return_type|lift_fn }}(it!!) },
                            {%- else %}
{{ " "|repeat(indent) }}    { {{ return_type|lift_fn }}(it) },
                            {%- endif -%}
                            {%- when None %}
{{ " "|repeat(indent) }}    { Unit },
{{ " "|repeat(indent) }}    {% endmatch %}
{{ " "|repeat(indent) }}    // Error FFI converter
                            {%- match callable.throws_type() -%}
                            {%- when Some(e) %}
{{ " "|repeat(indent) }}    {{ e|type_name(ci) }}ErrorHandler,
                            {%- when None %}
{{ " "|repeat(indent) }}    UniffiNullRustCallStatusErrorHandler,
                            {%- endmatch %}
{{ " "|repeat(indent) }})
{%- endmacro %}

{%- macro arg_list_lowered(func, indent) -%}
                        {%- for arg in func.arguments() %}
{{ " "|repeat(indent) }}{{ arg|lower_fn }}({{ arg.name()|var_name }}),
                        {%- endfor -%}
{%- endmacro -%}

{#-
// Arglist as used in kotlin declarations of methods, functions and constructors.
// If is_decl, then default values be specified.
// Note the var_name and type_name filters.
-#}

{% macro arg_list(func, is_decl) %}
{%- for arg in func.arguments() -%}
        {{ arg.name()|var_name }}: {{ arg|type_name(ci) }}
{%-     if is_decl %}
{%-         match arg.default_value() %}
{%-             when Some with(literal) %} = {{ literal|render_literal(arg, ci, config) }}
{%-             else %}
{%-         endmatch %}
{%-     endif %}
{%-     if !loop.last %}, {% endif -%}
{%- endfor %}
{%- endmacro %}

{#-
// Arglist as used in the UniffiLib function declarations.
// Note unfiltered name but ffi_type_name filters.
-#}
{%- macro arg_list_ffi_decl(func, indent) -%}
                        {%- for arg in func.arguments() %}
{{ " "|repeat(indent) }}{{ arg.name()|var_name }}: {{ arg.type_().borrow()|ffi_type_name_by_value(ci) }},
                        {%- endfor -%}
                        {%- if func.has_rust_call_status_arg() %}
{{ " "|repeat(indent) }}uniffiCallStatus: UniffiRustCallStatus,
                        {%- endif -%}
{%- endmacro -%}

{%- macro arg_list_ffi_call_native(func) %}
    {%- for arg in func.arguments() -%}
        {%- if let Some(callback) = arg.type_().borrow()|ffi_as_callback(ci) -%}
        {%- if callback|ffi_callback_needs_casting_native %}
        {{ arg.name()|var_name }} as {{ci.namespace()}}.cinterop.{{ arg.type_().borrow()|ffi_type_name_for_ffi_struct(ci) }},
        {%- else %}
        {{ arg.name()|var_name }},
        {%- endif -%}
        {%- else %}
        {{ arg.name()|var_name }}{{- arg.type_().borrow()|ffi_cast_to_local_rust_buffer_if_needed(ci) -}},
        {%- endif -%}
    {%- endfor -%}
    {%- if func.has_rust_call_status_arg() %}
        uniffiCallStatus,
    {%- endif -%}
{%- endmacro -%}

{% macro field_name(field, field_num) %}
{%- if field.name().is_empty() -%}
v{{- field_num -}}
{%- else -%}
{{ field.name()|var_name }}
{%- endif -%}
{%- endmacro %}

{% macro field_name_unquoted(field, field_num) %}
{%- if field.name().is_empty() -%}
v{{- field_num -}}
{%- else -%}
{{ field.name()|var_name|unquote }}
{%- endif -%}
{%- endmacro %}

{#- Macro for destroying fields -#}
{%- macro destroy_fields(member, indent) %}
{{ " "|repeat(indent) }}Disposable.destroy(
                            {%- for field in member.fields() %}
{{ " "|repeat(indent) }}    this.{%- call field_name(field, loop.index) -%}{%- endcall %},
                            {%- endfor %}
{{ " "|repeat(indent) }})
{%- endmacro -%}

{#- Macro for generating equals() ans hashCode() -#}
{%- macro generate_equals_hash_code(data_class, type_name, indent) %}
{{ " "|repeat(indent) }}override fun equals(other: Any?): Boolean {
{{ " "|repeat(indent) }}    if (this === other) return true
{{ " "|repeat(indent) }}    if (other == null || this::class != other::class) return false

{{ " "|repeat(indent) }}    other as {{ type_name }}

                            {%- if data_class.fields().len() == 1 -%}
                            {%-     for field in data_class.fields() %}
                            {%-         match field|as_data_class_field_type -%}
                            {%-             when DataClassFieldType::Bytes %}
{{ " "|repeat(indent) }}    return {% call field_name(field, loop.index) %}{% endcall %}.contentEquals(other.{% call field_name(field, loop.index) %}{% endcall %})
                            {%-             when DataClassFieldType::NullableBytes %}
{{ " "|repeat(indent) }}    if ({% call field_name(field, loop.index) %}{% endcall %} != null) {
{{ " "|repeat(indent) }}        if (other.{% call field_name(field, loop.index) %}{% endcall %} == null) return false
{{ " "|repeat(indent) }}        if (!{% call field_name(field, loop.index) %}{% endcall %}.contentEquals(other.{% call field_name(field, loop.index) %}{% endcall %})) return false
{{ " "|repeat(indent) }}    }

{{ " "|repeat(indent) }}    return true
                            {%-             else %}
{{ " "|repeat(indent) }}    return {% call field_name(field, loop.index) %}{% endcall %} == other.{% call field_name(field, loop.index) %}{% endcall %}
                            {%-         endmatch -%}
                            {%-     endfor -%}
                            {%- else -%}
                            {%-     for field in data_class.fields() -%}
                            {%-         match field|as_data_class_field_type -%}
                            {%-             when DataClassFieldType::Bytes %}
{{ " "|repeat(indent) }}    if (!{% call field_name(field, loop.index) %}{% endcall %}.contentEquals(other.{% call field_name(field, loop.index) %}{% endcall %})) return false
                            {%-             when DataClassFieldType::NullableBytes %}
{{ " "|repeat(indent) }}    if ({% call field_name(field, loop.index) %}{% endcall %} != null) {
{{ " "|repeat(indent) }}        if (other.{% call field_name(field, loop.index) %}{% endcall %} == null) return false
{{ " "|repeat(indent) }}        if (!{% call field_name(field, loop.index) %}{% endcall %}.contentEquals(other.{% call field_name(field, loop.index) %}{% endcall %})) return false
{{ " "|repeat(indent) }}    }
                            {%-             else %}
{{ " "|repeat(indent) }}    if ({% call field_name(field, loop.index) %}{% endcall %} != other.{% call field_name(field, loop.index) %}{% endcall %}) return false
                            {%-         endmatch -%}
                            {%-     endfor %}

{{ " "|repeat(indent) }}    return true
                            {%- endif %}
{{ " "|repeat(indent) }}{{ '}' }}
{{ " "|repeat(indent) }}override fun hashCode(): Int {
                            {%- for field in data_class.fields() -%}
                            {%-     if loop.first -%}
                            {%-         if data_class.fields().len() == 1 %}
{{ " "|repeat(indent) }}    return{{ ' ' }}
                            {%-         else %}
{{ " "|repeat(indent) }}    var result ={{ ' ' }}
                            {%-         endif -%}
                            {%-     else %}
{{ " "|repeat(indent) }}    result = 31 * result +{{ ' ' }}
                            {%-     endif -%}
                            {%-     match field|as_data_class_field_type -%}
                            {%-         when DataClassFieldType::Bytes -%}
                            {% call field_name(field, loop.index) %}{% endcall %}.contentHashCode()
                            {%-         when DataClassFieldType::NullableBytes -%}
                            ({% call field_name(field, loop.index) %}{% endcall %}?.contentHashCode() ?: 0)
                            {%-         when DataClassFieldType::NonNullableNonBytes -%}
                            {% call field_name(field, loop.index) %}{% endcall %}.hashCode()
                            {%-         when DataClassFieldType::NullableNonBytes -%}
                            ({% call field_name(field, loop.index) %}{% endcall %}?.hashCode() ?: 0)
                            {%-     endmatch -%}
                            {%- endfor -%}
                            {%- if data_class.fields().len() > 1 %}
{{ " "|repeat(indent) }}    return result
                            {%- endif %}
{{ " "|repeat(indent) }}{{ '}' }}
{%- endmacro -%}

{%- macro docstring_value(maybe_docstring, indent_spaces) %}
{%- match maybe_docstring %}
{%- when Some(docstring) %}
{{ docstring|docstring(indent_spaces) }}
{%- else %}
{%- endmatch %}
{%- endmacro %}

{%- macro docstring(defn, indent_spaces) %}
{%- call docstring_value(defn.docstring(), indent_spaces) %}{% endcall %}
{%- endmacro %}

{#- Macro for generating uniffi trait method implementations (Display/Eq/Hash/Ord) -#}
{#- `type_name`: the Kotlin type name of the record/enum -#}
{#- `uniffi_trait_methods`: the UniffiTraitMethods from rec.uniffi_trait_methods() -#}
{#- `indent`: indentation level for inline (non-KMP) mode -#}
{#- `use_extension`: when true, generate as top-level extension functions (KMP mode) -#}
{%- macro uniffi_trait_impls(type_name, uniffi_trait_methods, indent, use_extension) %}
{%- if let Some(fmt) = uniffi_trait_methods.display_fmt.or(uniffi_trait_methods.debug_fmt.clone()) %}
{%- if use_extension %}
{{ visibility() }}fun {{ type_name }}.toString(): String {
    return {{ fmt.return_type().unwrap()|lift_fn }}({% call to_ffi_call(fmt, 4) %}{% endcall %})
}
{%- else %}
{{ " "|repeat(indent) }}override fun toString(): String {
{{ " "|repeat(indent) }}    return {{ fmt.return_type().unwrap()|lift_fn }}({% call to_ffi_call(fmt, indent + 4) %}{% endcall %})
{{ " "|repeat(indent) }}}
{%- endif %}
{%- endif %}
{%- if let Some(eq) = uniffi_trait_methods.eq_eq %}
{%- if use_extension %}
{{ visibility() }}fun {{ type_name }}.equals(other: Any?): Boolean {
    if (this === other) return true
    if (other !is {{ type_name }}) return false
    return {{ eq.return_type().unwrap()|lift_fn }}({% call to_ffi_call(eq, 4) %}{% endcall %})
}
{%- else %}
{{ " "|repeat(indent) }}override fun equals(other: Any?): Boolean {
{{ " "|repeat(indent) }}    if (this === other) return true
{{ " "|repeat(indent) }}    if (other !is {{ type_name }}) return false
{{ " "|repeat(indent) }}    return {{ eq.return_type().unwrap()|lift_fn }}({% call to_ffi_call(eq, indent + 4) %}{% endcall %})
{{ " "|repeat(indent) }}}
{%- endif %}
{%- endif %}
{%- if let Some(hash) = uniffi_trait_methods.hash_hash %}
{%- if use_extension %}
{{ visibility() }}fun {{ type_name }}.hashCode(): Int {
    return {{ hash.return_type().unwrap()|lift_fn }}({%- call to_ffi_call(hash, 4) %}{% endcall %}).toInt()
}
{%- else %}
{{ " "|repeat(indent) }}override fun hashCode(): Int {
{{ " "|repeat(indent) }}    return {{ hash.return_type().unwrap()|lift_fn }}({%- call to_ffi_call(hash, indent + 4) %}{% endcall %}).toInt()
{{ " "|repeat(indent) }}}
{%- endif %}
{%- endif %}
{%- if let Some(cmp) = uniffi_trait_methods.ord_cmp %}
{%- if use_extension %}
{{ visibility() }}operator fun {{ type_name }}.compareTo(other: {{ type_name }}): Int {
    return {{ cmp.return_type().unwrap()|lift_fn }}({%- call to_ffi_call(cmp, 4) %}{% endcall %}).toInt()
}
{%- else %}
{{ " "|repeat(indent) }}override operator fun compareTo(other: {{ type_name }}): Int {
{{ " "|repeat(indent) }}    return {{ cmp.return_type().unwrap()|lift_fn }}({%- call to_ffi_call(cmp, indent + 4) %}{% endcall %}).toInt()
{{ " "|repeat(indent) }}}
{%- endif %}
{%- endif %}
{%- endmacro %}
