
{#
// Kotlin's `enum class` construct doesn't support variants with associated data,
// but is a little nicer for consumers than its `sealed class` enum pattern.
// So, we switch here, using `enum class` for enums with no associated data
// and `sealed class` for the general case.
#}

{%- let should_generate_serializable = config.generate_serializable() && e|serializable_enum(ci) -%}
{%- let has_methods = !e.methods().is_empty() -%}
{%- let uniffi_trait_methods = e.uniffi_trait_methods() -%}
{%- let has_trait_methods = uniffi_trait_methods.display_fmt.is_some() || uniffi_trait_methods.debug_fmt.is_some() || uniffi_trait_methods.eq_eq.is_some() || uniffi_trait_methods.hash_hash.is_some() || uniffi_trait_methods.ord_cmp.is_some() -%}
{%- let inline_methods = !config.kotlin_multiplatform && (has_methods || has_trait_methods) -%}
{%- let has_display = uniffi_trait_methods.display_fmt.is_some() || uniffi_trait_methods.debug_fmt.is_some() -%}
{%- let has_eq = uniffi_trait_methods.eq_eq.is_some() -%}
{%- let has_hash = uniffi_trait_methods.hash_hash.is_some() -%}
{%- let has_cmp = uniffi_trait_methods.ord_cmp.is_some() -%}

{%- if e.is_flat() %}

{%- call kt::docstring(e, 0) %}{% endcall %}
{% match e.variant_discr_type() %}
{% when None %}
{% if should_generate_serializable %}@kotlinx.serialization.Serializable{% endif %}
{{ visibility() }}enum class {{ type_name }} {
    {% for variant in e.variants() -%}
    {%- call kt::docstring(variant, 4) %}{% endcall %}
    {{ variant|variant_name(config) }}{% if loop.last %};{% else %},{% endif %}
    {%- endfor %}

    {%- if inline_methods %}
    {%- for meth in e.methods() %}
    {%- call kt::func_decl_with_body_enum("", meth, 4) -%}{%- endcall %}
    {%- endfor %}
    {%- call kt::uniffi_trait_impls(type_name, uniffi_trait_methods, 4, false) -%}{%- endcall %}
    {%- endif %}

    {{ visibility() }}companion object
}
{% when Some(variant_discr_type) %}
{% if should_generate_serializable %}@kotlinx.serialization.Serializable{% endif %}
{{ visibility() }}enum class {{ type_name }}(public val value: {{ variant_discr_type|type_name(ci) }}) {
    {% for variant in e.variants() -%}
    {%- call kt::docstring(variant, 4) %}{% endcall %}
    {{ variant|variant_name(config) }}({{ e|variant_discr_literal(loop.index0) }}){% if loop.last %};{% else %},{% endif %}
    {%- endfor %}

    {%- if inline_methods %}
    {%- for meth in e.methods() %}
    {%- call kt::func_decl_with_body_enum("", meth, 4) -%}{%- endcall %}
    {%- endfor %}
    {%- call kt::uniffi_trait_impls(type_name, uniffi_trait_methods, 4, false) -%}{%- endcall %}
    {%- endif %}

    {{ visibility() }}companion object
}
{% endmatch %}
{% else %}

{%- call kt::docstring(e, 0) %}{% endcall %}
{% if should_generate_serializable %}@kotlinx.serialization.Serializable{% endif %}
{{ visibility() }}sealed class {{ type_name }}{% if contains_object_references %}: Disposable {% endif %} {
    {% for variant in e.variants() -%}
    {%- let variant_type_name = variant|variant_type_name(ci) -%}
    {%- let should_generate_variant_serializable = config.generate_serializable() && variant|serializable_enum_variant(ci) -%}
    {%- call kt::docstring(variant, 4) %}{% endcall %}
    {%- if !variant.has_fields() %}
    {% if should_generate_variant_serializable %}@kotlinx.serialization.Serializable{% endif %}
    {{ visibility() }}{% if config.use_data_objects() %}data {% endif %}object {{ variant_type_name }} : {{ type_name }}() {% if contains_object_references %} {
        override fun destroy(): Unit = Unit
    }
    {% endif %}
    {% else -%}
    {%- let should_generate_equals_hash_code = variant|should_generate_equals_hash_code_enum_variant -%}
    {% if should_generate_variant_serializable %}@kotlinx.serialization.Serializable{% endif %}
    {{ visibility() }}data class {{ variant_type_name }}(
        {%- for field in variant.fields() -%}
        {%- call kt::docstring(field, 8) %}{% endcall %}
        val {% call kt::field_name(field, loop.index) %}{% endcall %}: {{ field|type_name(ci) }},
        {%- endfor %}
    ) : {{ type_name }}() {
        {%- if should_generate_equals_hash_code -%}
        {%- call kt::generate_equals_hash_code(variant, variant_type_name, 8) -%}{%- endcall %}
        {%- endif -%}
        {%- if contains_object_references %}
        override fun destroy() {
            {%- if variant.has_fields() -%}
            {%- call kt::destroy_fields(variant, 12) -%}{%- endcall %}
            {%- else %}
            // Nothing to destroy
            {%- endif %}
        }
        {%- endif %}
    }
    {%- endif %}
    {% endfor %}

    {%- if inline_methods %}
    {%- for meth in e.methods() %}
    {%- call kt::func_decl_with_body_enum("", meth, 4) -%}{%- endcall %}
    {%- endfor %}
    {%- call kt::uniffi_trait_impls(type_name, uniffi_trait_methods, 4, false) -%}{%- endcall %}
    {%- endif %}

    {{ visibility() }}companion object
}

{% endif %}

{%- if config.kotlin_multiplatform && (has_methods || has_trait_methods) %}
{%- for meth in e.methods() %}
{%- call kt::func_extension_decl_enum(type_name, meth, 0) -%}{%- endcall %}
{%- endfor %}
{%- if has_display %}
{{ visibility() }}expect fun {{ type_name }}.toString(): String
{%- endif %}
{%- if has_eq %}
{{ visibility() }}expect fun {{ type_name }}.equals(other: Any?): Boolean
{%- endif %}
{%- if has_hash %}
{{ visibility() }}expect fun {{ type_name }}.hashCode(): Int
{%- endif %}
{%- if has_cmp %}
{{ visibility() }}expect operator fun {{ type_name }}.compareTo(other: {{ type_name }}): Int
{%- endif %}
{%- endif %}
