use std::collections::HashMap;

use crate::generate::cpp_type::CppType;
use crate::generate::cs_type::CORDL_ACCESSOR_FIELD_PREFIX;
use crate::generate::members::CppLine;
use crate::generate::members::CppNestedUnion;

use brocolib::global_metadata::Il2CppFieldDefinition;
use brocolib::runtime_metadata::Il2CppType;
use itertools::Itertools;
use log::warn;

use std::sync::Arc;

use brocolib::runtime_metadata::Il2CppTypeEnum;

use brocolib::global_metadata::TypeDefinitionIndex;

use super::context_collection::CppContextCollection;
use super::cpp_type::CORDL_METHOD_HELPER_NAMESPACE;
use super::cpp_type_tag::CppTypeTag;
use super::cs_type::CSType;
use super::members::CppFieldDecl;
use super::members::CppFieldImpl;
use super::members::CppInclude;
use super::members::CppMember;
use super::members::CppMethodDecl;
use super::members::CppMethodImpl;
use super::members::CppNestedStruct;
use super::members::CppNonMember;
use super::members::CppParam;
use super::members::CppPropertyDecl;
use super::members::CppStaticAssert;
use super::members::CppTemplate;
use super::metadata::Metadata;
use super::type_extensions::Il2CppTypeEnumExtensions;
use super::type_extensions::TypeDefinitionExtensions;
use super::type_extensions::TypeExtentions;
use super::writer::Writable;

#[derive(Clone, Debug)]
pub struct FieldInfo<'a> {
    pub cpp_field: CppFieldDecl,
    pub field: &'a Il2CppFieldDefinition,
    pub field_type: &'a Il2CppType,
    pub is_constant: bool,
    pub is_static: bool,
    pub is_pointer: bool,

    pub offset: Option<u32>,
    pub size: usize,
}

pub struct FieldInfoSet<'a> {
    fields: Vec<Vec<FieldInfo<'a>>>,
    size: u32,
    offset: u32,
}

impl<'a> FieldInfoSet<'a> {
    fn max(&self) -> u32 {
        self.size + self.offset
    }
}

pub fn handle_static_fields(
    cpp_type: &mut CppType,
    fields: &[FieldInfo],
    metadata: &Metadata,
    tdi: TypeDefinitionIndex,
) {
    let t = CppType::get_type_definition(metadata, tdi);

    // if no fields, skip
    if t.field_count == 0 {
        return;
    }

    // we want only static fields
    // we ignore constants
    for field_info in fields.iter().filter(|f| f.is_static && !f.is_constant) {
        let f_type = field_info.field_type;
        let f_name = field_info.field.name(metadata.metadata);
        let f_offset = field_info.offset.unwrap_or(u32::MAX);
        let f_size = field_info.size;
        let field_ty_cpp_name = &field_info.cpp_field.field_ty;

        // non const field
        // instance field access on ref types is special
        // ref type instance fields are specially named because the field getters are supposed to be used
        let f_cpp_name = field_info.cpp_field.cpp_name.clone();

        let klass_resolver = cpp_type.classof_cpp_name();

        let getter_call =
                format!("return {CORDL_METHOD_HELPER_NAMESPACE}::getStaticField<{field_ty_cpp_name}, \"{f_name}\", {klass_resolver}>();");

        let setter_var_name = "value";
        let setter_call =
                format!("{CORDL_METHOD_HELPER_NAMESPACE}::setStaticField<{field_ty_cpp_name}, \"{f_name}\", {klass_resolver}>(std::forward<{field_ty_cpp_name}>({setter_var_name}));");

        // don't get a template that has no names
        let useful_template =
            cpp_type
                .cpp_template
                .clone()
                .and_then(|t| match t.names.is_empty() {
                    true => None,
                    false => Some(t),
                });

        let getter_name = format!("getStaticF_{}", f_cpp_name);
        let setter_name = format!("setStaticF_{}", f_cpp_name);

        let get_return_type = field_ty_cpp_name.clone();

        let getter_decl = CppMethodDecl {
            cpp_name: getter_name.clone(),
            instance: false,
            return_type: get_return_type,

            brief: None,
            body: None, // TODO:
            // Const if instance for now
            is_const: false,
            is_constexpr: !f_type.is_static() || f_type.is_constant(),
            is_inline: true,
            is_virtual: false,
            is_operator: false,
            is_no_except: false, // TODO:
            parameters: vec![],
            prefix_modifiers: vec![],
            suffix_modifiers: vec![],
            template: None,
        };

        let setter_decl = CppMethodDecl {
            cpp_name: setter_name,
            instance: false,
            return_type: "void".to_string(),

            brief: None,
            body: None,      //TODO:
            is_const: false, // TODO: readonly fields?
            is_constexpr: !f_type.is_static() || f_type.is_constant(),
            is_inline: true,
            is_virtual: false,
            is_operator: false,
            is_no_except: false, // TODO:
            parameters: vec![CppParam {
                def_value: None,
                modifiers: "".to_string(),
                name: setter_var_name.to_string(),
                ty: field_ty_cpp_name.clone(),
            }],
            prefix_modifiers: vec![],
            suffix_modifiers: vec![],
            template: None,
        };

        let getter_impl = CppMethodImpl {
            body: vec![Arc::new(CppLine::make(getter_call.clone()))],
            declaring_cpp_full_name: cpp_type.cpp_name_components.remove_pointer().combine_all(),
            template: useful_template.clone(),

            ..getter_decl.clone().into()
        };

        let setter_impl = CppMethodImpl {
            body: vec![Arc::new(CppLine::make(setter_call))],
            declaring_cpp_full_name: cpp_type.cpp_name_components.remove_pointer().combine_all(),
            template: useful_template.clone(),

            ..setter_decl.clone().into()
        };

        // instance fields on a ref type should declare a cpp property

        let prop_decl = CppPropertyDecl {
            cpp_name: f_cpp_name,
            prop_ty: field_ty_cpp_name.clone(),
            instance: !f_type.is_static() && !f_type.is_constant(),
            getter: getter_decl.cpp_name.clone().into(),
            setter: setter_decl.cpp_name.clone().into(),
            indexable: false,
            brief_comment: Some(format!(
                "Field {f_name}, offset 0x{f_offset:x}, size 0x{f_size:x} "
            )),
        };

        // only push accessors if declaring ref type, or if static field
        cpp_type
            .declarations
            .push(CppMember::Property(prop_decl).into());

        // decl
        cpp_type
            .declarations
            .push(CppMember::MethodDecl(setter_decl).into());

        cpp_type
            .declarations
            .push(CppMember::MethodDecl(getter_decl).into());

        // impl
        cpp_type
            .implementations
            .push(CppMember::MethodImpl(setter_impl).into());

        cpp_type
            .implementations
            .push(CppMember::MethodImpl(getter_impl).into());
    }
}

pub(crate) fn handle_const_fields(
    cpp_type: &mut CppType,
    fields: &[FieldInfo],
    ctx_collection: &CppContextCollection,
    metadata: &Metadata,
    tdi: TypeDefinitionIndex,
) {
    let t = CppType::get_type_definition(metadata, tdi);

    // if no fields, skip
    if t.field_count == 0 {
        return;
    }

    let declaring_cpp_template = if cpp_type
        .cpp_template
        .as_ref()
        .is_some_and(|t| !t.names.is_empty())
    {
        cpp_type.cpp_template.clone()
    } else {
        None
    };

    for field_info in fields.iter().filter(|f| f.is_constant) {
        let f_type = field_info.field_type;
        let f_name = field_info.field.name(metadata.metadata);
        let f_offset = field_info.offset.unwrap_or(u32::MAX);
        let f_size = field_info.size;

        let def_value = field_info.cpp_field.value.as_ref();

        let def_value = def_value.expect("Constant with no default value?");

        match f_type.ty.is_primitive_builtin() {
            false => {
                // other type
                let field_decl = CppFieldDecl {
                    instance: false,
                    readonly: f_type.is_constant(),
                    value: None,
                    const_expr: false,
                    brief_comment: Some(format!("Field {f_name} value: {def_value}")),
                    ..field_info.cpp_field.clone()
                };
                let field_impl = CppFieldImpl {
                    value: def_value.clone(),
                    const_expr: true,
                    declaring_type: cpp_type.cpp_name_components.remove_pointer().combine_all(),
                    declaring_type_template: declaring_cpp_template.clone(),
                    ..field_decl.clone().into()
                };

                // get enum type to include impl
                // this is needed since the enum constructor is not defined
                // in the declaration
                // TODO: Make enum ctors inline defined
                if f_type.valuetype && f_type.ty == Il2CppTypeEnum::Valuetype {
                    let field_cpp_tag: CppTypeTag =
                        CppTypeTag::from_type_data(f_type.data, metadata.metadata);
                    let field_cpp_td_tag: CppTypeTag = field_cpp_tag.get_tdi().into();
                    let field_cpp_type = ctx_collection.get_cpp_type(field_cpp_td_tag);

                    if field_cpp_type.is_some_and(|f| f.is_enum_type) {
                        let field_cpp_context = ctx_collection
                            .get_context(field_cpp_td_tag)
                            .expect("No context for cpp enum type");

                        cpp_type.requirements.add_impl_include(
                            field_cpp_type,
                            CppInclude::new_context_typeimpl(field_cpp_context),
                        );
                    }
                }

                cpp_type
                    .declarations
                    .push(CppMember::FieldDecl(field_decl).into());
                cpp_type
                    .implementations
                    .push(CppMember::FieldImpl(field_impl).into());
            }
            true => {
                // primitive type
                let field_decl = CppFieldDecl {
                    instance: false,
                    const_expr: true,
                    readonly: f_type.is_constant(),

                    brief_comment: Some(format!(
                        "Field {f_name} offset 0x{f_offset:x} size 0x{f_size:x}"
                    )),
                    value: Some(def_value.clone()),
                    ..field_info.cpp_field.clone()
                };

                cpp_type
                    .declarations
                    .push(CppMember::FieldDecl(field_decl).into());
            }
        }
    }
}

pub(crate) fn handle_instance_fields(
    cpp_type: &mut CppType,
    fields: &[FieldInfo],
    metadata: &Metadata,
    tdi: TypeDefinitionIndex,
) {
    let t = CppType::get_type_definition(metadata, tdi);

    // if no fields, skip
    if t.field_count == 0 {
        return;
    }

    let instance_field_decls = fields
        .iter()
        .filter(|f| f.offset.is_some() && !f.is_static && !f.is_constant)
        .cloned()
        .collect_vec();

    let property_exists = |to_find: &str| {
        cpp_type.declarations.iter().any(|d| match d.as_ref() {
            CppMember::Property(p) => p.cpp_name == to_find,
            _ => false,
        })
    };

    let resulting_fields = instance_field_decls
        .into_iter()
        .map(|d| {
            let mut f = d.cpp_field;
            if property_exists(&f.cpp_name) {
                f.cpp_name = format!("_cordl_{}", &f.cpp_name);

                // make private if a property with this name exists
                f.is_private = true;
            }

            FieldInfo { cpp_field: f, ..d }
        })
        .collect_vec();

    // explicit layout types are packed into single unions
    if t.is_explicit_layout() {
        // oh no! the fields are unionizing! don't tell elon musk!
        let u = pack_fields_into_single_union(resulting_fields);
        cpp_type.declarations.push(CppMember::NestedUnion(u).into());
    } else {
        resulting_fields
            .into_iter()
            .map(|member| CppMember::FieldDecl(member.cpp_field))
            .for_each(|member| cpp_type.declarations.push(member.into()));

        // TODO: Make field offset asserts for explicit layouts!
        add_field_offset_asserts(cpp_type, fields);
    };
}

fn add_field_offset_asserts(cpp_type: &mut CppType, fields: &[FieldInfo]) {
    let cpp_name = cpp_type.cpp_name_components.remove_pointer().combine_all();
    for field in fields {
        let field_name = &field.cpp_field.cpp_name;
        let offset = field.offset.unwrap_or(u32::MAX);

        let assert = CppStaticAssert {
            condition: format!("offsetof({cpp_name}, {field_name}) == 0x{offset:x}"),
            message: Some("Offset mismatch!".to_string()),
        };
        cpp_type
            .nonmember_declarations
            .push(CppNonMember::CppStaticAssert(assert).into())
    }
}

pub(crate) fn fixup_backing_field(fieldname: &str) -> String {
    format!("{CORDL_ACCESSOR_FIELD_PREFIX}{fieldname}")
}

pub(crate) fn handle_valuetype_fields(
    cpp_type: &mut CppType,
    fields: &[FieldInfo],
    metadata: &Metadata,
    tdi: TypeDefinitionIndex,
) {
    // Value types only need getter fixes for explicit layout types
    let t = CppType::get_type_definition(metadata, tdi);

    // if no fields, skip
    if t.field_count == 0 {
        return;
    }

    // instance fields for explicit layout value types are special
    if t.is_explicit_layout() {
        for field_info in fields.iter().filter(|f| !f.is_constant && !f.is_static) {
            // don't get a template that has no names
            let template = cpp_type
                .cpp_template
                .clone()
                .and_then(|t| match t.names.is_empty() {
                    true => None,
                    false => Some(t),
                });

            let declaring_cpp_full_name =
                cpp_type.cpp_name_components.remove_pointer().combine_all();

            let prop = prop_decl_from_fieldinfo(metadata, field_info);
            let (accessor_decls, accessor_impls) =
                prop_methods_from_fieldinfo(field_info, template, declaring_cpp_full_name, false);

            cpp_type.declarations.push(CppMember::Property(prop).into());

            accessor_decls.into_iter().for_each(|method| {
                cpp_type
                    .declarations
                    .push(CppMember::MethodDecl(method).into());
            });

            accessor_impls.into_iter().for_each(|method| {
                cpp_type
                    .implementations
                    .push(CppMember::MethodImpl(method).into());
            });
        }

        let backing_fields = fields
            .iter()
            .cloned()
            .map(|mut f| {
                f.cpp_field.cpp_name = fixup_backing_field(&f.cpp_field.cpp_name);
                f
            })
            .collect_vec();

        handle_instance_fields(cpp_type, &backing_fields, metadata, tdi);
    } else {
        handle_instance_fields(cpp_type, fields, metadata, tdi);
    }
}

// create prop and field declaration from passed field info
pub(crate) fn prop_decl_from_fieldinfo(
    metadata: &Metadata,
    field_info: &FieldInfo,
) -> CppPropertyDecl {
    if field_info.is_static {
        panic!("Can't turn static fields into declspec properties!");
    }

    let f_name = field_info.field.name(metadata.metadata);
    let f_offset = field_info.offset.unwrap_or(u32::MAX);
    let f_size = field_info.size;
    let field_ty_cpp_name = &field_info.cpp_field.field_ty;

    let f_cpp_name = &field_info.cpp_field.cpp_name;

    let getter_name = format!("__get_{}", f_cpp_name);
    let setter_name = format!("__set_{}", f_cpp_name);

    CppPropertyDecl {
        cpp_name: f_cpp_name.clone(),
        prop_ty: field_ty_cpp_name.clone(),
        instance: !field_info.is_static,
        getter: Some(getter_name),
        setter: Some(setter_name),
        indexable: false,
        brief_comment: Some(format!(
            "Field {f_name}, offset 0x{f_offset:x}, size 0x{f_size:x} "
        )),
    }
}

pub(crate) fn prop_methods_from_fieldinfo(
    field_info: &FieldInfo,
    template: Option<CppTemplate>,
    declaring_cpp_name: String,
    declaring_is_ref: bool,
) -> (Vec<CppMethodDecl>, Vec<CppMethodImpl>) {
    let f_type = field_info.field_type;
    let field_ty_cpp_name = &field_info.cpp_field.field_ty;

    let f_cpp_name = &field_info.cpp_field.cpp_name;
    let cordl_field_name = fixup_backing_field(f_cpp_name);
    let field_access = format!("this->{cordl_field_name}");

    let getter_name = format!("__get_{}", f_cpp_name);
    let setter_name = format!("__set_{}", f_cpp_name);

    let (get_return_type, const_get_return_type) = match field_info.is_pointer {
        // Var types are default pointers
        true => (
            field_ty_cpp_name.clone(),
            format!("::cordl_internals::to_const_pointer<{field_ty_cpp_name}> const",),
        ),
        false => (
            field_ty_cpp_name.clone(),
            format!("{field_ty_cpp_name} const"),
        ),
    };

    // field accessors emit as ref because they are fields, you should be able to access them the same
    let (get_return_type, const_get_return_type) = (
        format!("{get_return_type}&"),
        format!("{const_get_return_type}&"),
    );

    // for ref types we emit an instance null check that is dependent on a compile time define,
    // that way we can prevent nullptr access and instead throw, if the user wants this
    // technically "this" should never ever be null, but in native modding this can happen
    let instance_null_check = match declaring_is_ref {
        true => Some("CORDL_FIELD_NULL_CHECK(static_cast<void const*>(this));"),
        false => None,
    };

    let getter_call = format!("return {field_access};");
    let setter_var_name = "value";
    // if the declaring type is a value type, we should not use wbarrier
    let setter_call = match !f_type.valuetype && declaring_is_ref {
        // ref type field write on a ref type
        true => {
            format!("il2cpp_functions::gc_wbarrier_set_field(this, static_cast<void**>(static_cast<void*>(&{field_access})), cordl_internals::convert(std::forward<decltype({setter_var_name})>({setter_var_name})));")
        }
        false => {
            format!("{field_access} = {setter_var_name};")
        }
    };

    let getter_decl = CppMethodDecl {
        cpp_name: getter_name.clone(),
        instance: true,
        return_type: get_return_type,

        brief: None,
        body: None, // TODO:
        // Const if instance for now
        is_const: false,
        is_constexpr: !f_type.is_static() || f_type.is_constant(),
        is_inline: true,
        is_virtual: false,
        is_operator: false,
        is_no_except: false, // TODO:
        parameters: vec![],
        prefix_modifiers: vec![],
        suffix_modifiers: vec![],
        template: None,
    };

    let const_getter_decl = CppMethodDecl {
        cpp_name: getter_name,
        instance: true,
        return_type: const_get_return_type,

        brief: None,
        body: None, // TODO:
        // Const if instance for now
        is_const: true,
        is_constexpr: !f_type.is_static() || f_type.is_constant(),
        is_inline: true,
        is_virtual: false,
        is_operator: false,
        is_no_except: false, // TODO:
        parameters: vec![],
        prefix_modifiers: vec![],
        suffix_modifiers: vec![],
        template: None,
    };

    let setter_decl = CppMethodDecl {
        cpp_name: setter_name,
        instance: true,
        return_type: "void".to_string(),

        brief: None,
        body: None,      //TODO:
        is_const: false, // TODO: readonly fields?
        is_constexpr: !f_type.is_static() || f_type.is_constant(),
        is_inline: true,
        is_virtual: false,
        is_operator: false,
        is_no_except: false, // TODO:
        parameters: vec![CppParam {
            def_value: None,
            modifiers: "".to_string(),
            name: setter_var_name.to_string(),
            ty: field_ty_cpp_name.clone(),
        }],
        prefix_modifiers: vec![],
        suffix_modifiers: vec![],
        template: None,
    };

    // construct getter and setter bodies
    let getter_body: Vec<Arc<dyn Writable>> = if let Some(instance_null_check) = instance_null_check
    {
        vec![
            Arc::new(CppLine::make(instance_null_check.into())),
            Arc::new(CppLine::make(getter_call)),
        ]
    } else {
        vec![Arc::new(CppLine::make(getter_call))]
    };

    let setter_body: Vec<Arc<dyn Writable>> = if let Some(instance_null_check) = instance_null_check
    {
        vec![
            Arc::new(CppLine::make(instance_null_check.into())),
            Arc::new(CppLine::make(setter_call)),
        ]
    } else {
        vec![Arc::new(CppLine::make(setter_call))]
    };

    let getter_impl = CppMethodImpl {
        body: getter_body.clone(),
        declaring_cpp_full_name: declaring_cpp_name.clone(),
        template: template.clone(),

        ..getter_decl.clone().into()
    };

    let const_getter_impl = CppMethodImpl {
        body: getter_body,
        declaring_cpp_full_name: declaring_cpp_name.clone(),
        template: template.clone(),

        ..const_getter_decl.clone().into()
    };

    let setter_impl = CppMethodImpl {
        body: setter_body,
        declaring_cpp_full_name: declaring_cpp_name.clone(),
        template: template.clone(),

        ..setter_decl.clone().into()
    };

    (
        vec![getter_decl, const_getter_decl, setter_decl],
        vec![getter_impl, const_getter_impl, setter_impl],
    )
}

pub(crate) fn handle_referencetype_fields(
    cpp_type: &mut CppType,
    fields: &[FieldInfo],
    metadata: &Metadata,
    tdi: TypeDefinitionIndex,
) {
    let t = CppType::get_type_definition(metadata, tdi);

    if t.is_explicit_layout() {
        warn!(
            "Reference type with explicit layout: {}",
            cpp_type.cpp_name_components.combine_all()
        );
    }

    // if no fields, skip
    if t.field_count == 0 {
        return;
    }

    for field_info in fields.iter().filter(|f| !f.is_constant && !f.is_static) {
        // don't get a template that has no names
        let template = cpp_type
            .cpp_template
            .clone()
            .and_then(|t| match t.names.is_empty() {
                true => None,
                false => Some(t),
            });

        let declaring_cpp_full_name = cpp_type.cpp_name_components.remove_pointer().combine_all();

        let prop = prop_decl_from_fieldinfo(metadata, field_info);
        let (accessor_decls, accessor_impls) =
            prop_methods_from_fieldinfo(field_info, template, declaring_cpp_full_name, true);

        cpp_type.declarations.push(CppMember::Property(prop).into());

        accessor_decls.into_iter().for_each(|method| {
            cpp_type
                .declarations
                .push(CppMember::MethodDecl(method).into());
        });

        accessor_impls.into_iter().for_each(|method| {
            cpp_type
                .implementations
                .push(CppMember::MethodImpl(method).into());
        });
    }

    let backing_fields = fields
        .iter()
        .cloned()
        .map(|mut f| {
            f.cpp_field.cpp_name = fixup_backing_field(&f.cpp_field.cpp_name);
            f
        })
        .collect_vec();

    handle_instance_fields(cpp_type, &backing_fields, metadata, tdi);
}

pub(crate) fn field_collision_check(instance_fields: &[FieldInfo]) -> bool {
    let mut next_offset = 0;
    return instance_fields
        .iter()
        .sorted_by(|a, b| a.offset.cmp(&b.offset))
        .any(|field| {
            let offset = field.offset.unwrap_or(u32::MAX);
            if offset < next_offset {
                true
            } else {
                next_offset = offset + field.size as u32;
                false
            }
        });
}

// inspired by what il2cpp does for explicitly laid out types
pub(crate) fn pack_fields_into_single_union(fields: Vec<FieldInfo>) -> CppNestedUnion {
    // get the min offset to use as a base for the packed structs
    let min_offset = fields.iter().map(|f| f.offset.unwrap()).min().unwrap_or(0);

    let packed_structs = fields
        .into_iter()
        .map(|field| {
            let structs = field_into_offset_structs(min_offset, field);

            vec![structs.0, structs.1]
        })
        .flat_map(|v| v.into_iter())
        .collect_vec();

    let declarations = packed_structs
        .into_iter()
        .map(|s| CppMember::NestedStruct(s).into())
        .collect_vec();

    CppNestedUnion {
        brief_comment: Some("Explicitly laid out type with union based offsets".into()),
        declarations,
        offset: min_offset,
        is_private: true,
    }
}

pub(crate) fn field_into_offset_structs(
    min_offset: u32,
    field: FieldInfo,
) -> (CppNestedStruct, CppNestedStruct) {
    // il2cpp basically turns each field into 2 structs within a union:
    // 1 which is packed with size 1, and padded with offset to fit to the end
    // the other which has the same padding and layout, except this one is for alignment so it's just packed as the parent struct demands

    let Some(actual_offset) = &field.offset else {
        panic!("don't call field_into_offset_structs with non instance fields!")
    };

    let padding = actual_offset;

    let packed_padding_cpp_name = format!("{}_padding[0x{padding:x}]", field.cpp_field.cpp_name);
    let alignment_padding_cpp_name = format!(
        "{}_padding_forAlignment[0x{padding:x}]",
        field.cpp_field.cpp_name
    );
    let alignment_cpp_name = format!("{}_forAlignment", field.cpp_field.cpp_name);

    let packed_padding_field = CppFieldDecl {
        brief_comment: Some(format!("Padding field 0x{padding:x}")),
        const_expr: false,
        cpp_name: packed_padding_cpp_name,
        field_ty: "uint8_t".into(),
        offset: *actual_offset,
        instance: true,
        is_private: false,
        readonly: false,
        value: None,
    };

    let alignment_padding_field = CppFieldDecl {
        brief_comment: Some(format!("Padding field 0x{padding:x} for alignment")),
        const_expr: false,
        cpp_name: alignment_padding_cpp_name,
        field_ty: "uint8_t".into(),
        offset: *actual_offset,
        instance: true,
        is_private: false,
        readonly: false,
        value: None,
    };

    let alignment_field = CppFieldDecl {
        cpp_name: alignment_cpp_name,
        is_private: false,
        ..field.cpp_field.clone()
    };

    let packed_field = CppFieldDecl {
        is_private: false,
        ..field.cpp_field
    };

    let packed_struct = CppNestedStruct {
        declaring_name: "".into(),
        base_type: None,
        declarations: vec![
            CppMember::FieldDecl(packed_padding_field).into(),
            CppMember::FieldDecl(packed_field).into(),
        ],
        brief_comment: None,
        is_class: false,
        is_enum: false,
        is_private: false,
        packing: Some(1),
    };

    let alignment_struct = CppNestedStruct {
        declaring_name: "".into(),
        base_type: None,
        declarations: vec![
            CppMember::FieldDecl(alignment_padding_field).into(),
            CppMember::FieldDecl(alignment_field).into(),
        ],
        brief_comment: None,
        is_class: false,
        is_enum: false,
        is_private: false,
        packing: None,
    };

    (packed_struct, alignment_struct)
}

/// generates the fields for the value type or reference type\
/// handles unions
pub(crate) fn make_or_unionize_fields(instance_fields: &[FieldInfo]) -> Vec<CppMember> {
    // make all fields like usual
    if !field_collision_check(instance_fields) {
        return instance_fields
            .iter()
            .map(|d| CppMember::FieldDecl(d.cpp_field.clone()))
            .collect_vec();
    }
    // we have a collision, investigate and handle

    let mut offset_map = HashMap::new();

    fn accumulated_size(fields: &[FieldInfo]) -> u32 {
        fields.iter().map(|f| f.size as u32).sum()
    }

    let mut current_max: u32 = 0;
    let mut current_offset: u32 = 0;

    // TODO: Field padding for exact offsets (explicit layouts?)

    // you can't sort instance fields on offset/size because it will throw off the unionization process
    instance_fields
        .iter()
        .sorted_by(|a, b| a.size.cmp(&b.size))
        .rev()
        .sorted_by(|a, b| a.offset.cmp(&b.offset))
        .for_each(|field| {
            let offset = field.offset.unwrap_or(u32::MAX);
            let size = field.size as u32;
            let max = offset + size;

            if max > current_max {
                current_offset = offset;
                current_max = max;
            }

            let current_set = offset_map
                .entry(current_offset)
                .or_insert_with(|| FieldInfoSet {
                    fields: vec![],
                    offset: current_offset,
                    size,
                });

            if current_max > current_set.max() {
                current_set.size = size
            }

            // if we have a last vector & the size of its fields + current_offset is smaller than current max add to that list
            if let Some(last) = current_set.fields.last_mut()
                && current_offset + accumulated_size(last) == offset
            {
                last.push(field.clone());
            } else {
                current_set.fields.push(vec![field.clone()]);
            }
        });

    offset_map
        .into_values()
        .map(|field_set| {
            // if we only have one list, just emit it as a set of fields
            if field_set.fields.len() == 1 {
                return field_set
                    .fields
                    .into_iter()
                    .flat_map(|v| v.into_iter())
                    .map(|d| CppMember::FieldDecl(d.cpp_field))
                    .collect_vec();
            }
            // we had more than 1 list, so we have unions to emit
            let declarations = field_set
                .fields
                .into_iter()
                .map(|struct_contents| {
                    if struct_contents.len() == 1 {
                        // emit a struct with only 1 field as just a field
                        return struct_contents
                            .into_iter()
                            .map(|d| CppMember::FieldDecl(d.cpp_field))
                            .collect_vec();
                    }
                    vec![
                        // if we have more than 1 field, emit a nested struct
                        CppMember::NestedStruct(CppNestedStruct {
                            base_type: None,
                            declaring_name: "".to_string(),
                            is_enum: false,
                            is_class: false,
                            is_private: false,
                            declarations: struct_contents
                                .into_iter()
                                .map(|d| CppMember::FieldDecl(d.cpp_field).into())
                                .collect_vec(),
                            brief_comment: Some(format!(
                                "Anonymous struct offset 0x{:x}, size 0x{:x}",
                                field_set.offset, field_set.size
                            )),
                            packing: None,
                        }),
                    ]
                })
                .flat_map(|v| v.into_iter())
                .collect_vec();

            // wrap our set into a union
            vec![CppMember::NestedUnion(CppNestedUnion {
                brief_comment: Some(format!(
                    "Anonymous union offset 0x{:x}, size 0x{:x}",
                    field_set.offset, field_set.size
                )),
                declarations: declarations.into_iter().map(|d| d.into()).collect_vec(),
                offset: field_set.offset,
                is_private: false,
            })]
        })
        .flat_map(|v| v.into_iter())
        .collect_vec()
}
