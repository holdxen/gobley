
{%- let rec = ci.get_record_definition(name).unwrap() -%}
{%- let should_generate_equals_hash_code = rec|should_generate_equals_hash_code_record -%}
{%- let should_generate_serializable = config.generate_serializable() && rec|serializable_record(ci) -%}
{%- let has_methods = !rec.methods().is_empty() -%}
{%- let uniffi_trait_methods = rec.uniffi_trait_methods() -%}
{%- let has_trait_methods = uniffi_trait_methods.display_fmt.is_some() || uniffi_trait_methods.debug_fmt.is_some() || uniffi_trait_methods.eq_eq.is_some() || uniffi_trait_methods.hash_hash.is_some() || uniffi_trait_methods.ord_cmp.is_some() -%}
{%- let comparable = !config.kotlin_multiplatform && uniffi_trait_methods.ord_cmp.is_some() -%}
{%- let inline_methods = !config.kotlin_multiplatform && (has_methods || has_trait_methods) -%}
{%- let has_display = uniffi_trait_methods.display_fmt.is_some() || uniffi_trait_methods.debug_fmt.is_some() -%}
{%- let has_eq = uniffi_trait_methods.eq_eq.is_some() -%}
{%- let has_hash = uniffi_trait_methods.hash_hash.is_some() -%}
{%- let has_cmp = uniffi_trait_methods.ord_cmp.is_some() -%}

{%- if rec.has_fields() %}
{%- call kt::docstring(rec, 0) %}{% endcall %}
{% if should_generate_serializable %}@kotlinx.serialization.Serializable{% endif %}
{{ visibility() }}data class {{ type_name }} (
    {%- for field in rec.fields() %}
    {%- call kt::docstring(field, 4) %}{% endcall %}
    {% if config.is_record_immutable(rec.name()) %}val{% else %}var{% endif %} {{ field.name()|var_name }}: {{ field|type_name(ci) -}}
    {%- match field.default_value() %}
        {%- when Some with(literal) %} = {{ literal|render_literal(field, ci, config) }}
        {%- else %}
    {%- endmatch -%}
    {% if !loop.last %}, {% endif %}
    {%- endfor %}
) {% if comparable && contains_object_references %}: Disposable, Comparable<{{ type_name }}> {% elif contains_object_references %}: Disposable {% elif comparable %}: Comparable<{{ type_name }}> {% endif %}{
    {%- if should_generate_equals_hash_code -%}
    {%- call kt::generate_equals_hash_code(rec, type_name, 4) -%}{%- endcall %}
    {%- endif -%}
    {%- if contains_object_references %}
    override fun destroy() {
        {%- call kt::destroy_fields(rec, 8) %}{% endcall %}
    }
    {%- endif %}
    {%- if inline_methods %}
    {%- for meth in rec.methods() %}
    {%- call kt::func_decl_with_body("", meth, 4) -%}{%- endcall %}
    {%- endfor %}
    {%- call kt::uniffi_trait_impls(type_name, uniffi_trait_methods, 4, false) -%}{%- endcall %}
    {%- endif %}
    {{ visibility() }}companion object
}
{%- else -%}
{%- call kt::docstring(rec, 0) %}{% endcall %}
{{ visibility() }}{% if config.use_data_objects() %}data {% endif %}object {{ type_name }}
{%- if inline_methods %} {
    {%- for meth in rec.methods() %}
    {%- call kt::func_decl_with_body("", meth, 4) -%}{%- endcall %}
    {%- endfor %}
    {%- call kt::uniffi_trait_impls(type_name, uniffi_trait_methods, 4, false) -%}{%- endcall %}
    {{ visibility() }}companion object
}
{%- endif %}
{%- endif %}

{%- if config.kotlin_multiplatform && (has_methods || has_trait_methods) %}
{%- for meth in rec.methods() %}
{%- call kt::func_extension_decl(type_name, meth, 0) -%}{%- endcall %}
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
