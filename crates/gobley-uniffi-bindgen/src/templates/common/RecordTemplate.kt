
{%- let rec = ci.get_record_definition(name).unwrap() -%}
{%- let should_generate_equals_hash_code = rec|should_generate_equals_hash_code_record -%}
{%- let should_generate_serializable = config.generate_serializable() && rec|serializable_record(ci) -%}
{%- let has_methods = !rec.methods().is_empty() -%}
{%- let use_expect_actual = config.kotlin_multiplatform && has_methods -%}

{%- if rec.has_fields() %}
{%- call kt::docstring(rec, 0) %}{% endcall %}
{% if should_generate_serializable && !use_expect_actual %}@kotlinx.serialization.Serializable{% endif %}
{{ visibility() }}{% if use_expect_actual %}expect {% endif %}data class {{ type_name }} (
    {%- for field in rec.fields() %}
    {%- call kt::docstring(field, 4) %}{% endcall %}
    {% if config.generate_immutable_records() %}val{% else %}var{% endif %} {{ field.name()|var_name }}: {{ field|type_name(ci) -}}
    {%- match field.default_value() %}
        {%- when Some with(literal) %} = {{ literal|render_literal(field, ci, config) }}
        {%- else %}
    {%- endmatch -%}
    {% if !loop.last %}, {% endif %}
    {%- endfor %}
) {% if contains_object_references %}: Disposable {% endif %}{
    {%- if use_expect_actual %}
    {%- for meth in rec.methods() %}
    {%- call kt::func_decl("", meth, 4, false) -%}{%- endcall %}
    {%- endfor %}
    {%- else %}
    {%- if should_generate_equals_hash_code -%}
    {%- call kt::generate_equals_hash_code(rec, type_name, 4) -%}{%- endcall %}
    {%- endif -%}
    {%- if contains_object_references %}
    override fun destroy() {
        {%- call kt::destroy_fields(rec, 8) %}{% endcall %}
    }
    {%- endif %}
    {%- for meth in rec.methods() %}
    {%- call kt::func_decl_with_body("", meth, 4) -%}{%- endcall %}
    {%- endfor %}
    {%- endif %}
    {{ visibility() }}{% if use_expect_actual %}expect {% endif %}companion object
}
{%- else -%}
{%- call kt::docstring(rec, 0) %}{% endcall %}
{{ visibility() }}{% if use_expect_actual %}expect {% endif %}{% if !use_expect_actual && config.use_data_objects() %}data {% endif %}object {{ type_name }}
{%- if use_expect_actual %} {
    {%- for meth in rec.methods() %}
    {%- call kt::func_decl("", meth, 4, false) -%}{%- endcall %}
    {%- endfor %}
    {{ visibility() }}expect companion object
}
{%- endif %}
{%- endif %}
