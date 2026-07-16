
{%- import "macros.kt" as kt %}

{%- for type_ in ci.iter_local_types() %}
{%- let type_name = type_|type_name(ci) %}
{%- let ffi_converter_name = type_|ffi_converter_name %}
{%- let canonical_type_name = type_|canonical_name %}
{%- let contains_object_references = ci.item_contains_object_references(type_) %}

{#
 # Map `Type` instances to an include statement for that type.
 #
 # There is a companion match in `KotlinCodeOracle::create_code_type()` which performs a similar function for the
 # Rust code.
 #
 #   - When adding additional types here, make sure to also add a match arm to that function.
 #   - To keep things manageable, let's try to limit ourselves to these 2 mega-matches
 #}

{%- match type_ %}

{%- when Type::Object { module_path, name, imp } %}
{% include "ObjectTemplate.kt" %}

{%- when Type::Record { name, module_path } %}
{%- if config.kotlin_multiplatform %}
{%- let rec = ci.get_record_definition(name).unwrap() %}
{%- let has_stub_methods = !rec.methods().is_empty() %}
{%- let stub_trait_methods = rec.uniffi_trait_methods() %}
{%- let has_stub_traits = stub_trait_methods.display_fmt.is_some() || stub_trait_methods.debug_fmt.is_some() || stub_trait_methods.eq_eq.is_some() || stub_trait_methods.hash_hash.is_some() || stub_trait_methods.ord_cmp.is_some() %}
{%- if has_stub_methods || has_stub_traits %}
{%- for meth in rec.methods() %}
{%- call kt::func_extension_stub(type_name, meth, 0) -%}{%- endcall %}
{%- endfor %}
{%- call kt::uniffi_trait_impls_stub(type_name, stub_trait_methods) -%}{%- endcall %}
{%- endif %}
{%- endif %}

{%- when Type::Enum { name, module_path } %}
{%- if config.kotlin_multiplatform %}
{%- let e = ci.get_enum_definition(name).unwrap() %}
{%- let has_stub_methods = !e.methods().is_empty() %}
{%- let stub_trait_methods = e.uniffi_trait_methods() %}
{%- let has_stub_traits = stub_trait_methods.display_fmt.is_some() || stub_trait_methods.debug_fmt.is_some() || stub_trait_methods.eq_eq.is_some() || stub_trait_methods.hash_hash.is_some() || stub_trait_methods.ord_cmp.is_some() %}
{%- if has_stub_methods || has_stub_traits %}
{%- for meth in e.methods() %}
{%- call kt::func_extension_stub_enum(type_name, meth, 0) -%}{%- endcall %}
{%- endfor %}
{%- call kt::uniffi_trait_impls_stub(type_name, stub_trait_methods) -%}{%- endcall %}
{%- endif %}
{%- endif %}

{%- else %}
{%- endmatch %}
{%- endfor %}
