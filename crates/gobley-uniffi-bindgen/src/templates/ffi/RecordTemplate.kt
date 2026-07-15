
{%- let rec = ci.get_record_definition(name).unwrap() %}
{%- let has_methods = !rec.methods().is_empty() %}
{%- let use_extension = config.kotlin_multiplatform && has_methods %}

{%- if use_extension %}
{%- for meth in rec.methods() %}
{%- call kt::func_extension_with_body(type_name, meth, 0) -%}{%- endcall %}
{%- endfor %}
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
