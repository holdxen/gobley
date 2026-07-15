
{#
// Kotlin's `enum class` construct doesn't support variants with associated data,
// but is a little nicer for consumers than its `sealed class` enum pattern.
// So, we switch here, using `enum class` for enums with no associated data
// and `sealed class` for the general case.
#}

{%- let has_methods = !e.methods().is_empty() %}
{%- let uniffi_trait_methods = e.uniffi_trait_methods() %}
{%- let has_trait_methods = uniffi_trait_methods.display_fmt.is_some() || uniffi_trait_methods.debug_fmt.is_some() || uniffi_trait_methods.eq_eq.is_some() || uniffi_trait_methods.hash_hash.is_some() || uniffi_trait_methods.ord_cmp.is_some() %}
{%- let use_extension = config.kotlin_multiplatform && (has_methods || has_trait_methods) %}

{%- if use_extension %}
{%- for meth in e.methods() %}
{%- call kt::func_extension_with_body_enum(type_name, meth, 0) -%}{%- endcall %}
{%- endfor %}
{%- call kt::uniffi_trait_impls(type_name, uniffi_trait_methods, 0, true) -%}{%- endcall %}
{%- endif %}

{%- if e.is_flat() %}

{{ visibility() }}object {{ e|ffi_converter_name }}: FfiConverterRustBuffer<{{ type_name }}> {
    override fun read(buf: ByteBuffer): {{ type_name }} = try {
        {%- if config.use_enum_entries() %}
        {{ type_name }}.entries[buf.getInt() - 1]
        {%- else %}
        {{ type_name }}.values()[buf.getInt() - 1]
        {%- endif %}
    } catch (e: IndexOutOfBoundsException) {
        throw RuntimeException("invalid enum value, something is very wrong!!", e)
    }

    override fun allocationSize(value: {{ type_name }}): ULong = 4UL

    override fun write(value: {{ type_name }}, buf: ByteBuffer) {
        buf.putInt(value.ordinal + 1)
    }
}

{%- else %}

{{ visibility() }}object {{ e|ffi_converter_name }} : FfiConverterRustBuffer<{{ type_name }}>{
    override fun read(buf: ByteBuffer): {{ type_name }} {
        return when(buf.getInt()) {
            {%- for variant in e.variants() %}
            {{ loop.index }} -> {{ type_name }}.{{ variant|variant_type_name(ci) }}{% if variant.has_fields() %}(
                {% for field in variant.fields() -%}
                {{ field|read_fn(ci) }}(buf),
                {% endfor -%}
            ){%- endif -%}
            {%- endfor %}
            else -> throw RuntimeException("invalid enum value, something is very wrong!!")
        }
    }

    override fun allocationSize(value: {{ type_name }}): ULong = when(value) {
        {%- for variant in e.variants() %}
        is {{ type_name }}.{{ variant|variant_type_name(ci) }} -> {
            // Add the size for the Int that specifies the variant plus the size needed for all fields
            (
                4UL
                {%- for field in variant.fields() %}
                + {{ field|allocation_size_fn }}(value.{%- call kt::field_name(field, loop.index) -%}{%- endcall %})
                {%- endfor %}
            )
        }
        {%- endfor %}
    }

    override fun write(value: {{ type_name }}, buf: ByteBuffer) {
        when(value) {
            {%- for variant in e.variants() %}
            is {{ type_name }}.{{ variant|variant_type_name(ci) }} -> {
                buf.putInt({{ loop.index }})
                {%- for field in variant.fields() %}
                {{ field|write_fn(ci) }}(value.{%- call kt::field_name(field, loop.index) -%}{%- endcall %}, buf)
                {%- endfor %}
                Unit
            }
            {%- endfor %}
        }.let { /* this makes the `when` an expression, which ensures it is exhaustive */ }
    }
}

{%- endif %}
