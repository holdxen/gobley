
{%- let rec = ci.get_record_definition(name).unwrap() %}
{%- let should_generate_equals_hash_code = rec|should_generate_equals_hash_code_record %}
{%- let has_methods = !rec.methods().is_empty() %}
{%- let use_expect_actual = config.kotlin_multiplatform && has_methods %}
{%- let actual = self.actual_keyword() %}

{%- if use_expect_actual %}
{%- if rec.has_fields() %}
{{ visibility() }}actual data class {{ type_name }} (
    {%- for field in rec.fields() %}
    {% if config.generate_immutable_records() %}actual val{% else %}actual var{% endif %} {{ field.name()|var_name }}: {{ field|type_name(ci) -}}
    {%- match field.default_value() %}
        {%- when Some with(literal) %} = {{ literal|render_literal(field, ci, config) }}
        {%- else %}
    {%- endmatch -%}
    {% if !loop.last %}, {% endif %}
    {%- endfor %}
) {% if contains_object_references %}: Disposable {% endif %}{
    {%- if should_generate_equals_hash_code -%}
    {%- call kt::generate_equals_hash_code(rec, type_name, 4) -%}{%- endcall %}
    {%- endif -%}
    {%- if contains_object_references %}
    override fun destroy() {
        {%- call kt::destroy_fields(rec, 8) %}{% endcall %}
    }
    {%- endif %}
    {%- for meth in rec.methods() %}
    {%- call kt::func_decl_with_body(actual, meth, 4) -%}{%- endcall %}
    {%- endfor %}
    {{ visibility() }}actual companion object
}
{%- else %}
{{ visibility() }}actual object {{ type_name }} {
    {%- for meth in rec.methods() %}
    {%- call kt::func_decl_with_body(actual, meth, 4) -%}{%- endcall %}
    {%- endfor %}
    {{ visibility() }}actual companion object
}
{%- endif %}
{%- endif %}

{{ visibility() }}object {{ rec|ffi_converter_name }}: FfiConverterRustBuffer<{{ type_name }}> {
    override fun read(buf: ByteBuffer): {{ type_name }} {
        {%- if rec.has_fields() %}
        return {{ type_name }}(
        {%- for field in rec.fields() %}
            {{ field|read_fn(ci) }}(buf),
        {%- endfor %}
        )
        {%- else %}
        return {{ type_name }}
        {%- endif %}
    }

    override fun allocationSize(value: {{ type_name }}): ULong = {%- if rec.has_fields() %} (
        {%- for field in rec.fields() %}
            {{ field|allocation_size_fn }}(value.{{ field.name()|var_name }}){% if !loop.last %} +{% endif %}
        {%- endfor %}
    ) {%- else %} 0UL {%- endif %}

    override fun write(value: {{ type_name }}, buf: ByteBuffer) {
        {%- for field in rec.fields() %}
        {{ field|write_fn(ci) }}(value.{{ field.name()|var_name }}, buf)
        {%- endfor %}
    }
}
