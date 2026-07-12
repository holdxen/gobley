
{%- call kt::docstring_value(interface_docstring, 0) %}{% endcall %}
{{ visibility() }}interface {{ interface_name }} {
    {% for meth in methods.iter() -%}
    {%- call kt::func_decl("", meth, 4, true) %}{% endcall %}
    {% endfor %}
    {{ visibility() }}companion object
}
