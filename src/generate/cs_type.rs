use core::panic;
use log::{debug, error, info, warn};
use std::{
    collections::HashMap,
    io::{Cursor, Read},
    rc::Rc,
    sync::Arc,
};

use brocolib::{
    global_metadata::{
        FieldIndex, Il2CppTypeDefinition, MethodIndex, ParameterIndex, TypeDefinitionIndex,
        TypeIndex,
    },
    runtime_metadata::{Il2CppMethodSpec, Il2CppType, Il2CppTypeEnum, TypeData},
};
use byteorder::{LittleEndian, ReadBytesExt};

use itertools::Itertools;

use crate::{
    data::name_components::NameComponents,
    generate::{members::CppUsingAlias, offsets},
    helpers::cursor::ReadBytesExtensions,
};

use super::{
    config::GenerationConfig,
    context_collection::CppContextCollection,
    cpp_type::{
        CppType, CppTypeRequirements, CORDL_METHOD_HELPER_NAMESPACE,
        CORDL_NUM_ENUM_TYPE_CONSTRAINT, CORDL_REFERENCE_TYPE_CONSTRAINT, __CORDL_BACKING_ENUM_TYPE,
    },
    cpp_type_tag::CppTypeTag,
    members::{
        CppCommentedString, CppConstructorDecl, CppConstructorImpl, CppFieldDecl, CppFieldImpl,
        CppForwardDeclare, CppInclude, CppLine, CppMember, CppMethodData, CppMethodDecl,
        CppMethodImpl, CppMethodSizeStruct, CppParam, CppPropertyDecl, CppTemplate,
    },
    metadata::Metadata,
    type_extensions::{
        Il2CppTypeEnumExtensions, MethodDefintionExtensions, ParameterDefinitionExtensions,
        TypeDefinitionExtensions, TypeExtentions,
    },
    writer::Writable,
};

type Endian = LittleEndian;

// negative
const VALUE_TYPE_SIZE_OFFSET: u32 = 0x10;

const VALUE_TYPE_WRAPPER_INSTANCE_NAME: &str = "::bs_hook::ValueTypeWrapper::instance";
const VALUE_TYPE_WRAPPER_SIZE: &str = "__CORDL_VALUE_TYPE_SIZE";
const REFERENCE_TYPE_WRAPPER_SIZE: &str = "__CORDL_REFERENCE_TYPE_SIZE";
const REFERENCE_WRAPPER_INSTANCE_NAME: &str = "::bs_hook::Il2CppWrapperType::instance";

pub const VALUE_WRAPPER_TYPE: &str = "::bs_hook::ValueTypeWrapper";
pub const ENUM_WRAPPER_TYPE: &str = "::bs_hook::EnumTypeWrapper";
pub const INTERFACE_WRAPPER_TYPE: &str = "::cordl_internals::InterfaceW";
pub const OBJECT_WRAPPER_TYPE: &str = "::bs_hook::Il2CppWrapperType";
pub const CORDL_NO_INCLUDE_IMPL_DEFINE: &str = "CORDL_NO_IMPL_INCLUDE";

pub const ENUM_PTR_TYPE: &str = "::bs_hook::EnumPtr";
pub const VT_PTR_TYPE: &str = "::bs_hook::VTPtr";

const SIZEOF_IL2CPP_OBJECT: u32 = 0x10;

pub trait CSType: Sized {
    fn get_mut_cpp_type(&mut self) -> &mut CppType; // idk how else to do this
    fn get_cpp_type(&self) -> &CppType; // idk how else to do this

    fn get_tag_tdi(tag: TypeData) -> TypeDefinitionIndex {
        match tag {
            TypeData::TypeDefinitionIndex(tdi) => tdi,
            _ => panic!("Unsupported type: {tag:?}"),
        }
    }
    fn get_cpp_tag_tdi(tag: CppTypeTag) -> TypeDefinitionIndex {
        tag.into()
    }

    fn parent_joined_cpp_name(metadata: &Metadata, tdi: TypeDefinitionIndex) -> String {
        let ty_def = &metadata.metadata.global_metadata.type_definitions[tdi];

        let name = ty_def.name(metadata.metadata);

        if ty_def.declaring_type_index != u32::MAX {
            let declaring_ty =
                metadata.metadata_registration.types[ty_def.declaring_type_index as usize];

            if let TypeData::TypeDefinitionIndex(declaring_tdi) = declaring_ty.data {
                return Self::parent_joined_cpp_name(metadata, declaring_tdi) + "/" + name;
            } else {
                return declaring_ty.full_name(metadata.metadata) + "/" + name;
            }
        }

        ty_def.full_name(metadata.metadata, true)
    }

    fn fixup_into_generic_instantiation(&mut self) -> &mut CppType {
        let cpp_type = self.get_mut_cpp_type();
        assert!(
            cpp_type.generic_instantiations_args_types.is_some(),
            "No generic instantiation args!"
        );

        cpp_type.cpp_template = Some(CppTemplate { names: vec![] });
        cpp_type.is_stub = false;
        cpp_type.cpp_name_components.generics = None;

        cpp_type
    }

    fn add_method_generic_inst(
        &mut self,
        method_spec: &Il2CppMethodSpec,
        metadata: &Metadata,
    ) -> &mut CppType {
        assert!(method_spec.method_inst_index != u32::MAX);

        let cpp_type = self.get_mut_cpp_type();

        let inst = metadata
            .metadata_registration
            .generic_insts
            .get(method_spec.method_inst_index as usize)
            .unwrap();

        cpp_type.method_generic_instantiation_map.insert(
            method_spec.method_definition_index,
            inst.types.iter().map(|t| *t as TypeIndex).collect(),
        );

        cpp_type
    }

    fn make_cpp_type(
        metadata: &Metadata,
        config: &GenerationConfig,
        tdi: TypeDefinitionIndex,
        tag: CppTypeTag,
        generic_inst_types: Option<&Vec<usize>>,
    ) -> Option<CppType> {
        // let iface = metadata.interfaces.get(t.interfaces_start);
        // Then, handle interfaces

        // Then, handle methods
        // - This includes constructors
        // inherited methods will be inherited

        let t = &metadata.metadata.global_metadata.type_definitions[tdi];

        // Generics
        // This is a generic type def
        // TODO: Constraints!
        let generics = t.generic_container_index.is_valid().then(|| {
            t.generic_container(metadata.metadata)
                .generic_parameters(metadata.metadata)
                .iter()
                .map(|param| param)
                .collect_vec()
        });

        let cpp_template = generics.as_ref().map(|g| {
            CppTemplate::make_typenames(g.iter().map(|g| g.name(metadata.metadata).to_string()))
        });

        let ns = t.namespace(metadata.metadata);
        let name = t.name(metadata.metadata);
        let full_name = t.full_name(metadata.metadata, false);

        if metadata.blacklisted_types.contains(&tdi) {
            info!("Skipping {full_name} ({tdi:?}) because it's blacklisted");

            return None;
        }

        // all nested types are unnested
        let nested = false; // t.declaring_type_index != u32::MAX;
        let cs_name_components = t.get_name_components(metadata.metadata);

        let cpp_name_components = NameComponents {
            declaring_types: cs_name_components
                .declaring_types
                .iter()
                .map(|s| config.name_cpp(s))
                .collect_vec(),
            generics: cs_name_components.generics.clone(),
            name: config.name_cpp(&cs_name_components.name),
            namespace: config.namespace_cpp(&cs_name_components.namespace),
        };

        // TODO: Come up with a way to avoid this extra call to layout the entire type
        // We really just want to call it once for a given size and then move on
        // Every type should have a valid metadata size, even if it is 0
        let metadata_size = offsets::get_sizeof_type(t, tdi, generic_inst_types, metadata);

        // Modified later for nested types
        let mut cpptype = CppType {
            self_tag: tag,
            nested,
            prefix_comments: vec![format!("Type: {ns}::{name}")],

            calculated_size: Some(metadata_size as usize),

            cpp_name_components,
            cs_name_components,

            declarations: Default::default(),
            implementations: Default::default(),
            nonmember_implementations: Default::default(),
            nonmember_declarations: Default::default(),

            is_value_type: t.is_value_type(),
            is_enum_type: t.is_enum_type(),
            requirements: Default::default(),

            inherit: Default::default(),
            is_interface: t.is_interface(),
            cpp_template,

            generic_instantiations_args_types: generic_inst_types.cloned(),
            method_generic_instantiation_map: Default::default(),

            is_stub: false,
            is_hidden: true,
            nested_types: Default::default(),
        };

        if cpptype.generic_instantiations_args_types.is_some() {
            cpptype.fixup_into_generic_instantiation();
        }

        // Nested type unnesting fix
        if t.declaring_type_index != u32::MAX {
            let declaring_ty = &metadata
                .metadata
                .runtime_metadata
                .metadata_registration
                .types[t.declaring_type_index as usize];

            let declaring_tag = CppTypeTag::from_type_data(declaring_ty.data, metadata.metadata);
            let declaring_tdi: TypeDefinitionIndex = declaring_tag.into();
            let declaring_td = &metadata.metadata.global_metadata.type_definitions[declaring_tdi];
            let combined_name = cpptype.cpp_name_components.combine_all(false);

            cpptype.cpp_name_components.namespace =
                config.namespace_cpp(declaring_td.namespace(metadata.metadata));
            cpptype.cpp_name_components.declaring_types = vec![]; // remove declaring types

            cpptype.cpp_name_components.name = config.generic_nested_name(&combined_name);
        }

        if t.parent_index == u32::MAX {
            if !t.is_interface() {
                info!("Skipping type: {ns}::{name} because it has parent index: {} and is not an interface!", t.parent_index);
                return None;
            }
        } else if metadata
            .metadata_registration
            .types
            .get(t.parent_index as usize)
            .is_none()
        {
            panic!("NO PARENT! But valid index found: {}", t.parent_index);
        }

        Some(cpptype)
    }

    fn fill_from_il2cpp(
        &mut self,
        metadata: &Metadata,
        config: &GenerationConfig,
        ctx_collection: &CppContextCollection,
    ) {
        if self.get_cpp_type().is_stub {
            // Do not fill stubs
            return;
        }

        let tdi: TypeDefinitionIndex = self.get_cpp_type().self_tag.into();

        let t = &metadata.metadata.global_metadata.type_definitions[tdi];

        self.make_generics_args(metadata, ctx_collection, tdi);
        self.make_parents(metadata, ctx_collection, tdi);
        self.make_interfaces(metadata, ctx_collection, tdi);

        // we depend on parents and generic args here
        // default ctor
        if t.is_value_type() || t.is_enum_type() {
            self.create_valuetype_constructor(metadata, ctx_collection, config, tdi);
            self.create_valuetype_field_wrapper();
            if t.is_enum_type() {
                self.create_enum_wrapper(metadata, ctx_collection, tdi);
                self.create_enum_backing_type_constant(metadata, ctx_collection, tdi);
            }
        } else if t.is_interface() {
            self.make_interface_constructors();
        } else {
            self.create_ref_size();
            self.create_ref_default_constructor();
            self.create_ref_default_operators();
        }

        self.make_nested_types(metadata, ctx_collection, config, tdi);
        self.make_fields(metadata, ctx_collection, config, tdi);
        self.make_properties(metadata, ctx_collection, config, tdi);
        self.make_methods(metadata, config, ctx_collection, tdi);

        if let Some(func) = metadata.custom_type_handler.get(&tdi) {
            func(self.get_mut_cpp_type())
        }
    }

    // fn make_generic_constraints(
    //     &mut self,
    //     metadata: &Metadata,
    //     config: &GenerationConfig,
    //     ctx_collection: &CppContextCollection,
    //     tdi: TypeDefinitionIndex,
    // ) {
    //     let t = Self::get_type_definition(metadata, tdi);

    //     if !t.generic_container_index.is_valid() {
    //         return;
    //     }

    //     let generic_class = metadata.metadata_registration.generic_classes.iter().find(|t| t.);
    //     metadata.metadata_registration.generic_insts.get(generic_class.unwrap().context.class_inst_idx.unwrap())

    //     let generics = t.generic_container(metadata.metadata);

    //     let generic_constraints: Vec<Vec<String>> = generics
    //         .generic_parameters(metadata.metadata)
    //         .iter()
    //         .map(|p| p.constraints(metadata.metadata))
    //         .map(|c| {
    //             c.iter()
    //                 .map(|ti| {
    //                     self.cppify_name_il2cpp(
    //                         ctx_collection,
    //                         metadata,
    //                         metadata
    //                             .metadata_registration
    //                             .types
    //                             .get(*ti as usize)
    //                             .unwrap(),
    //                         true,
    //                     )
    //                 })
    //                 .filter(|l| !l.is_empty())
    //                 .collect()
    //         })
    //         .filter(|l: &Vec<String>| !l.is_empty())
    //         .collect();
    //     let cpp_type = self.get_mut_cpp_type();
    // }

    fn make_generics_args(
        &mut self,
        metadata: &Metadata,
        ctx_collection: &CppContextCollection,
        tdi: TypeDefinitionIndex,
    ) {
        let cpp_type = self.get_mut_cpp_type();

        if cpp_type.generic_instantiations_args_types.is_none() {
            return;
        }

        let generic_instantiations_args_types =
            cpp_type.generic_instantiations_args_types.clone().unwrap();

        let td = &metadata.metadata.global_metadata.type_definitions[tdi];
        let _ty = &metadata.metadata_registration.types[td.byval_type_index as usize];
        let generic_container = td.generic_container(metadata.metadata);

        let mut template_args: Vec<(String, String)> = vec![];

        let generic_instantiation_args: Vec<String> = generic_instantiations_args_types
            .iter()
            .enumerate()
            .map(|(gen_param_idx, u)| {
                let t = metadata.metadata_registration.types.get(*u).unwrap();

                let gen_param = generic_container
                    .generic_parameters(metadata.metadata)
                    .iter()
                    .find(|p| p.num as usize == gen_param_idx)
                    .expect("No generic parameter found for this index!");

                (t, gen_param)
            })
            .map(|(t, gen_param)| {
                let gen_name = gen_param.name(metadata.metadata).to_string();

                parse_generic_arg(
                    t,
                    gen_name,
                    cpp_type,
                    ctx_collection,
                    metadata,
                    &mut template_args,
                )
            })
            .collect();

        // Handle nested types
        // Assumes these nested types exist,
        // which are created in the make_generic type func
        // TODO: Base off a CppType the alias path

        // Add template constraint
        // get all args with constraints
        if !template_args.is_empty() {
            cpp_type.cpp_template = Some(CppTemplate {
                names: template_args,
            });
        }

        cpp_type.cpp_name_components.generics =
            Some(generic_instantiation_args.into_iter().collect_vec());
    }

    fn make_methods(
        &mut self,
        metadata: &Metadata,
        config: &GenerationConfig,
        ctx_collection: &CppContextCollection,
        tdi: TypeDefinitionIndex,
    ) {
        let cpp_type = self.get_mut_cpp_type();
        let t = Self::get_type_definition(metadata, tdi);

        // Then, handle methods
        if t.method_count > 0 {
            // Write comment for methods
            cpp_type.declarations.push(
                CppMember::Comment(CppCommentedString {
                    data: "".to_string(),
                    comment: Some("Methods".to_string()),
                })
                .into(),
            );

            // 2 because each method gets a method struct and method decl
            // a constructor will add an additional one for each
            cpp_type
                .declarations
                .reserve(2 * (t.method_count as usize + 1));
            cpp_type
                .implementations
                .reserve(t.method_count as usize + 1);

            // Then, for each method, write it out
            for (i, _method) in t.methods(metadata.metadata).iter().enumerate() {
                let method_index = MethodIndex::new(t.method_start.index() + i as u32);
                self.create_method(t, method_index, metadata, ctx_collection, config, false);
            }
        }
    }

    fn make_fields(
        &mut self,
        metadata: &Metadata,
        ctx_collection: &CppContextCollection,
        config: &GenerationConfig,
        tdi: TypeDefinitionIndex,
    ) {
        let cpp_type = self.get_mut_cpp_type();
        let t = Self::get_type_definition(metadata, tdi);

        // Then, handle fields
        if t.field_count == 0 {
            return;
        }

        // Write comment for fields
        cpp_type.declarations.push(
            CppMember::Comment(CppCommentedString {
                data: "".to_string(),
                comment: Some("Fields".to_string()),
            })
            .into(),
        );

        // Then, for each field, write it out
        cpp_type.declarations.reserve(t.field_count as usize);
        cpp_type.implementations.reserve(t.field_count as usize);

        let field_offsets = &metadata
            .metadata_registration
            .field_offsets
            .as_ref()
            .unwrap()[tdi.index() as usize];

        let mut offsets = Vec::<u32>::new();
        if let Some(sz) = offsets::get_size_of_type_table(metadata, tdi) {
            if sz.instance_size == 0 {
                // At this point we need to compute the offsets
                debug!(
                    "Computing offsets for TDI: {:?}, as it has a size of 0",
                    tdi
                );
                let _resulting_size = offsets::layout_fields(
                    metadata,
                    t,
                    tdi,
                    cpp_type.generic_instantiations_args_types.as_ref(),
                    Some(&mut offsets),
                );
            }
        }
        let mut offset_iter = offsets.iter();

        for (i, field) in t.fields(metadata.metadata).iter().enumerate() {
            let f_type = metadata
                .metadata_registration
                .types
                .get(field.type_index as usize)
                .unwrap();

            let field_index = FieldIndex::new(t.field_start.index() + i as u32);
            let f_name = field.name(metadata.metadata);

            let f_cpp_name = config.name_cpp_plus(f_name, &[cpp_type.cpp_name().as_str()]);

            let f_offset = match f_type.is_static() || f_type.is_constant() {
                true => 0,
                false => {
                    // If we have a hotfix offset, use that instead
                    // We can safely assume this always returns None even if we "next" past the end
                    let offset = if let Some(computed_offset) = offset_iter.next() {
                        *computed_offset
                    } else {
                        field_offsets[i]
                    };

                    if offset < metadata.object_size() as u32 {
                        warn!("Field {f_name} ({offset:x}) of {} is smaller than object size {:x} is value type {}",
                            t.full_name(metadata.metadata, true),
                            metadata.object_size(),
                            t.is_value_type() || t.is_enum_type()
                        );
                    }

                    // TODO: Is the offset supposed to be smaller than object size for fixups?
                    match t.is_value_type() && offset >= metadata.object_size() as u32 {
                        true => {
                            // value type fixup
                            offset - metadata.object_size() as u32
                        }
                        false => offset,
                    }
                }
            };

            if let TypeData::TypeDefinitionIndex(field_tdi) = f_type.data && metadata.blacklisted_types.contains(&field_tdi) {
                if !cpp_type.is_value_type && !cpp_type.is_enum_type {
                    continue;
                }
                warn!("Value type uses {tdi:?} which is blacklisted! TODO");
            }

            let field_ty_cpp_name = if f_type.is_constant() && f_type.ty == Il2CppTypeEnum::String {
                "::ConstString".to_string()
            } else {
                cpp_type.cppify_name_il2cpp(ctx_collection, metadata, f_type, 0)
            };

            // TODO: Check a flag to look for default values to speed this up
            let def_value = Self::field_default_value(metadata, field_index);

            assert!(def_value.is_none() || (def_value.is_some() && f_type.is_param_optional()));

            let declaring_cpp_template = if cpp_type
                .cpp_template
                .as_ref()
                .is_some_and(|t| !t.names.is_empty())
            {
                cpp_type.cpp_template.clone()
            } else {
                None
            };

            if f_type.is_constant() {
                let def_value = def_value.expect("Constant with no default value?");

                match f_type.ty.is_primitive_builtin() {
                    false => {
                        // other type
                        let field_decl = CppFieldDecl {
                            cpp_name: f_cpp_name,
                            field_ty: field_ty_cpp_name,
                            instance: false,
                            readonly: f_type.is_constant(),
                            value: None,
                            const_expr: false,
                            brief_comment: Some(format!("Field {f_name} value: {def_value}")),
                        };
                        let field_impl = CppFieldImpl {
                            value: def_value,
                            const_expr: true,
                            declaring_type: cpp_type.cpp_name_components.combine_all(true),
                            declaring_type_template: declaring_cpp_template,
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
                            cpp_name: f_cpp_name,
                            field_ty: field_ty_cpp_name,
                            instance: false,
                            readonly: f_type.is_constant(),
                            value: Some(def_value),
                            const_expr: true,
                            brief_comment: Some(format!("Field {f_name} offset 0x{f_offset:x}")),
                        };

                        cpp_type
                            .declarations
                            .push(CppMember::FieldDecl(field_decl).into());
                    }
                }
            } else {
                let instance = match t.is_value_type() || t.is_enum_type() {
                    true => {
                        format!("this->{VALUE_WRAPPER_TYPE}<{VALUE_TYPE_WRAPPER_SIZE}>::instance")
                    }
                    false => "*this".to_string(),
                };

                let klass_resolver = cpp_type.classof_cpp_name();

                let getter_call = match f_type.is_static() {
                    true => {
                        format!(
                        "return {CORDL_METHOD_HELPER_NAMESPACE}::getStaticField<{field_ty_cpp_name}, \"{f_name}\", {klass_resolver}>();"
                    )
                    }
                    false => {
                        format!(
                            "return {CORDL_METHOD_HELPER_NAMESPACE}::getInstanceField<{field_ty_cpp_name}, 0x{f_offset:x}>({instance});"
                        )
                    }
                };

                let setter_var_name = "value";
                let setter_call = match f_type.is_static() {
                    true => {
                        format!(
                        "{CORDL_METHOD_HELPER_NAMESPACE}::setStaticField<{field_ty_cpp_name}, \"{f_name}\", {klass_resolver}>(std::forward<{field_ty_cpp_name}>({setter_var_name}));"
                    )
                    }
                    false => {
                        format!(
                            "{CORDL_METHOD_HELPER_NAMESPACE}::setInstanceField<{field_ty_cpp_name}, 0x{f_offset:x}>({instance}, std::forward<{field_ty_cpp_name}>({setter_var_name}));"
                        )
                    }
                };

                // don't get a template that has no names
                let useful_template =
                    cpp_type
                        .cpp_template
                        .clone()
                        .and_then(|t| match t.names.is_empty() {
                            true => None,
                            false => Some(t),
                        });

                let is_instance = !f_type.is_static() && !f_type.is_constant();

                let (getter_name, setter_name) = match is_instance {
                    true => (
                        format!("__get_{}", f_cpp_name),
                        format!("__set_{}", f_cpp_name),
                    ),
                    false => (
                        format!("getStaticF_{}", f_cpp_name),
                        format!("setStaticF_{}", f_cpp_name),
                    ),
                };

                let getter_decl = CppMethodDecl {
                    cpp_name: getter_name,
                    instance: is_instance,
                    return_type: field_ty_cpp_name.clone(),

                    brief: None,
                    body: None, // TODO:
                    // Const if instance for now
                    is_const: is_instance,
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
                    instance: is_instance,
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
                    body: vec![Arc::new(CppLine::make(getter_call))],
                    declaring_cpp_full_name: cpp_type.cpp_name_components.combine_all(true),
                    template: useful_template.clone(),

                    ..getter_decl.clone().into()
                };

                let setter_impl = CppMethodImpl {
                    body: vec![Arc::new(CppLine::make(setter_call))],
                    declaring_cpp_full_name: cpp_type.cpp_name_components.combine_all(true),
                    template: useful_template.clone(),

                    ..setter_decl.clone().into()
                };

                if is_instance {
                    // instance fields should declare a cpp property
                    let field_decl = CppPropertyDecl {
                        cpp_name: f_cpp_name,
                        prop_ty: field_ty_cpp_name.clone(),
                        instance: !f_type.is_static() && !f_type.is_constant(),
                        getter: getter_decl.cpp_name.clone().into(),
                        setter: setter_decl.cpp_name.clone().into(),
                        brief_comment: Some(format!("Field {f_name} offset 0x{f_offset:x}")),
                    };

                    cpp_type
                        .declarations
                        .push(CppMember::Property(field_decl).into());
                } else {
                    // static fields can't declare a cpp property
                    let field_comment = CppLine {
                        line: format!("// Static field {f_name}"),
                    };

                    cpp_type
                        .declarations
                        .push(CppMember::CppLine(field_comment).into());
                }

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
    }

    fn make_parents(
        &mut self,
        metadata: &Metadata,
        ctx_collection: &CppContextCollection,
        tdi: TypeDefinitionIndex,
    ) {
        let cpp_type = self.get_mut_cpp_type();
        let t = &metadata.metadata.global_metadata.type_definitions[tdi];

        let ns = t.namespace(metadata.metadata);
        let name = t.name(metadata.metadata);

        if t.parent_index == u32::MAX {
            // TYPE_ATTRIBUTE_INTERFACE = 0x00000020
            match t.is_interface() {
                true => {
                    cpp_type.inherit.push(INTERFACE_WRAPPER_TYPE.to_string());
                }
                false => {
                    info!("Skipping type: {ns}::{name} because it has parent index: {} and is not an interface!", t.parent_index);
                }
            }
            return;
        }

        let parent_type = metadata
            .metadata_registration
            .types
            .get(t.parent_index as usize)
            .unwrap_or_else(|| panic!("NO PARENT! But valid index found: {}", t.parent_index));

        let parent_ty: CppTypeTag = CppTypeTag::from_type_data(parent_type.data, metadata.metadata);

        // handle value types and enum types specially
        match t.is_value_type() || t.is_enum_type() {
            // parent will be a value wrapper type
            // which will have the size
            // OF THE TYPE ITSELF, NOT PARENT
            true => {
                let size = cpp_type
                    .calculated_size
                    .expect("No size for value/enum type!");

                let wrapper = wrapper_type_for_tdi(t);

                cpp_type.inherit.push(format!("{wrapper}<0x{size:x}>"));
            }
            // handle as reference type
            false => {
                // make sure our parent is intended
                assert!(
                    matches!(
                        parent_type.ty,
                        Il2CppTypeEnum::Class
                            | Il2CppTypeEnum::Genericinst
                            | Il2CppTypeEnum::Object
                    ),
                    "Not a class, object or generic inst!"
                );

                // We have a parent, lets do something with it
                let inherit_type =
                    cpp_type.cppify_name_il2cpp(ctx_collection, metadata, parent_type, usize::MAX);

                if matches!(
                    parent_type.ty,
                    Il2CppTypeEnum::Class | Il2CppTypeEnum::Genericinst
                ) {
                    // TODO: Figure out why some generic insts don't work here
                    let parent_tdi: TypeDefinitionIndex = parent_ty.into();

                    let base_type_context = ctx_collection
                                    .get_context(parent_ty)
                                    .or_else(|| ctx_collection.get_context(parent_tdi.into()))
                                    .unwrap_or_else(|| {
                                        panic!(
                                        "No CppContext for base type {inherit_type}. Using tag {parent_ty:?}"
                                    )
                                    });

                    let base_type_cpp_type = ctx_collection
                        .get_cpp_type(parent_ty)
                        .or_else(|| ctx_collection.get_cpp_type(parent_tdi.into()))
                        .unwrap_or_else(|| {
                            panic!(
                                "No CppType for base type {inherit_type}. Using tag {parent_ty:?}"
                            )
                        });

                    cpp_type.requirements.add_impl_include(
                        Some(base_type_cpp_type),
                        CppInclude::new_context_typeimpl(base_type_context),
                    )
                }

                cpp_type.inherit.push(inherit_type);
            }
        }
    }

    fn make_interfaces(
        &mut self,
        metadata: &Metadata<'_>,
        ctx_collection: &CppContextCollection,
        tdi: TypeDefinitionIndex,
    ) {
        let cpp_type = self.get_mut_cpp_type();
        let t = &metadata.metadata.global_metadata.type_definitions[tdi];

        for &interface_index in t.interfaces(metadata.metadata) {
            let int_ty = &metadata.metadata_registration.types[interface_index as usize];

            // We have an interface, lets do something with it
            let interface_cpp_name =
                cpp_type.cppify_name_il2cpp(ctx_collection, metadata, int_ty, 0);

            let convert_line = match t.is_value_type() || t.is_enum_type() {
                true => {
                    // box
                    "::cordl_internals::Box(this).convert()".to_string()
                }
                false => REFERENCE_WRAPPER_INSTANCE_NAME.to_string(),
            };

            let method_decl = CppMethodDecl {
                body: Default::default(),
                brief: Some(format!("Convert operator to {interface_cpp_name}")),
                cpp_name: interface_cpp_name.clone(),
                return_type: "".to_string(),
                instance: true,
                is_const: true,
                is_constexpr: true,
                is_no_except: !t.is_value_type() && !t.is_enum_type(),
                is_operator: true,
                is_virtual: false,
                is_inline: true,
                parameters: vec![],
                template: None,
                prefix_modifiers: vec![],
                suffix_modifiers: vec![],
            };

            let method_impl_template = if cpp_type
                .cpp_template
                .as_ref()
                .is_some_and(|c| !c.names.is_empty())
            {
                cpp_type.cpp_template.clone()
            } else {
                None
            };

            let method_impl = CppMethodImpl {
                body: vec![Arc::new(CppLine::make(format!(
                    "return {interface_cpp_name}({convert_line});"
                )))],
                declaring_cpp_full_name: cpp_type.cpp_name_components.combine_all(true),
                template: method_impl_template,
                ..method_decl.clone().into()
            };
            cpp_type
                .declarations
                .push(CppMember::MethodDecl(method_decl).into());

            cpp_type
                .implementations
                .push(CppMember::MethodImpl(method_impl).into());
        }
    }

    fn make_nested_types(
        &mut self,
        metadata: &Metadata,
        ctx_collection: &CppContextCollection,
        _config: &GenerationConfig,

        tdi: TypeDefinitionIndex,
    ) {
        let cpp_type = self.get_mut_cpp_type();
        let t = &metadata.metadata.global_metadata.type_definitions[tdi];

        if t.nested_type_count == 0 {
            return;
        }

        let generic_instantiation_args = cpp_type.cpp_name_components.generics.clone();

        let aliases = t
            .nested_types(metadata.metadata)
            .iter()
            .filter(|t| !metadata.blacklisted_types.contains(t))
            .map(|nested_tdi| {
                let nested_tag = CppTypeTag::TypeDefinitionIndex(*nested_tdi);

                let nested_context = ctx_collection
                    .get_context(nested_tag)
                    .expect("Unable to find CppContext");
                let nested = ctx_collection
                    .get_cpp_type(nested_tag)
                    .expect("Unable to find nested CppType");

                let alias = CppUsingAlias::from_cpp_type(
                    nested.cpp_name().clone(),
                    nested,
                    generic_instantiation_args.clone(),
                    // if no generic args are made, we can do the generic fixup
                    // ORDER OF PASSES MATTERS
                    nested.generic_instantiations_args_types.is_none(),
                );
                let fd = CppForwardDeclare::from_cpp_type(nested);
                let inc = CppInclude::new_context_typedef(nested_context);

                (alias, fd, inc)
            })
            .collect_vec();

        for (alias, fd, inc) in aliases {
            cpp_type
                .declarations
                .insert(0, CppMember::CppUsingAlias(alias).into());
            cpp_type.requirements.add_forward_declare((fd, inc));
        }

        // forward

        // old way of making nested types

        // let mut nested_types: Vec<CppType> = Vec::with_capacity(t.nested_type_count as usize);

        // for &nested_type_index in t.nested_types(metadata.metadata) {
        //     let nt_ty = &metadata.metadata.global_metadata.type_definitions[nested_type_index];

        //     // We have a parent, lets do something with it
        //     let nested_type = CppType::make_cpp_type(
        //         metadata,
        //         config,
        //         CppTypeTag::TypeDefinitionIndex(nested_type_index),
        //         nested_type_index,
        //     );

        //     match nested_type {
        //         Some(unwrapped) => nested_types.push(unwrapped),
        //         None => info!("Failed to make nested CppType {nt_ty:?}"),
        //     };
        // }

        // cpp_type.nested_types = nested_types.into_iter().map(|t| (t.self_tag, t)).collect()
    }

    fn make_properties(
        &mut self,
        metadata: &Metadata,
        ctx_collection: &CppContextCollection,
        config: &GenerationConfig,
        tdi: TypeDefinitionIndex,
    ) {
        let cpp_type = self.get_mut_cpp_type();
        let t = Self::get_type_definition(metadata, tdi);

        // Then, handle properties
        if t.property_count == 0 {
            return;
        }
        // Write comment for properties
        cpp_type.declarations.push(
            CppMember::Comment(CppCommentedString {
                data: "".to_string(),
                comment: Some("Properties".to_string()),
            })
            .into(),
        );
        cpp_type.declarations.reserve(t.property_count as usize);
        // Then, for each field, write it out
        for prop in t.properties(metadata.metadata) {
            let p_name = prop.name(metadata.metadata);
            let p_setter = (prop.set != u32::MAX).then(|| prop.set_method(t, metadata.metadata));
            let p_getter = (prop.get != u32::MAX).then(|| prop.get_method(t, metadata.metadata));

            // if this is a static property, skip emitting a cpp property since those can't be static
            if p_getter.or(p_setter).unwrap().is_static_method() {
                continue;
            }

            let p_type_index = match p_getter {
                Some(g) => g.return_type as usize,
                None => p_setter.unwrap().parameters(metadata.metadata)[0].type_index as usize,
            };

            let p_type = metadata
                .metadata_registration
                .types
                .get(p_type_index)
                .unwrap();

            let p_ty_cpp_name = cpp_type.cppify_name_il2cpp(ctx_collection, metadata, p_type, 0);

            let _method_map = |p: MethodIndex| {
                let method_calc = metadata.method_calculations.get(&p).unwrap();
                CppMethodData {
                    estimated_size: method_calc.estimated_size,
                    addrs: method_calc.addrs,
                }
            };

            let _abstr = p_getter.is_some_and(|p| p.is_abstract_method())
                || p_setter.is_some_and(|p| p.is_abstract_method());

            // Need to include this type
            cpp_type.declarations.push(
                CppMember::Property(CppPropertyDecl {
                    cpp_name: config.name_cpp(p_name),
                    prop_ty: p_ty_cpp_name.clone(),
                    // methods generated in make_methods
                    setter: p_setter.map(|m| config.name_cpp(m.name(metadata.metadata))),
                    getter: p_getter.map(|m| config.name_cpp(m.name(metadata.metadata))),
                    brief_comment: None,
                    instance: true,
                })
                .into(),
            );
        }
    }

    fn create_ref_size(&mut self) {
        let cpp_type = self.get_mut_cpp_type();
        if let Some(size) = cpp_type.calculated_size {
            cpp_type.declarations.push(
                CppMember::FieldDecl(CppFieldDecl {
                    cpp_name: REFERENCE_TYPE_WRAPPER_SIZE.to_string(),
                    field_ty: "auto".to_string(),
                    instance: false,
                    readonly: false,
                    const_expr: true,
                    value: Some(format!("0x{size:x}")),
                    brief_comment: Some("The size of the true reference type".to_string()),
                })
                .into(),
            );
        } else {
            todo!("Why does this type not have a valid size??? {:?}", cpp_type);
        }
    }
    fn create_enum_backing_type_constant(
        &mut self,
        metadata: &Metadata,
        ctx_collection: &CppContextCollection,
        tdi: TypeDefinitionIndex,
    ) {
        let cpp_type = self.get_mut_cpp_type();

        let t = Self::get_type_definition(metadata, tdi);

        let backing_field_idx = t.element_type_index as usize;
        let backing_field_ty = &metadata.metadata_registration.types[backing_field_idx];

        let enum_base = cpp_type.cppify_name_il2cpp(ctx_collection, metadata, backing_field_ty, 0);

        cpp_type.declarations.push(
            CppMember::CppUsingAlias(CppUsingAlias {
                alias: __CORDL_BACKING_ENUM_TYPE.to_string(),
                result: enum_base,
                template: None,
            })
            .into(),
        );
    }

    fn create_enum_wrapper(
        &mut self,
        metadata: &Metadata,
        ctx_collection: &CppContextCollection,
        tdi: TypeDefinitionIndex,
    ) {
        let cpp_type = self.get_mut_cpp_type();
        let t = Self::get_type_definition(metadata, tdi);
        let mut wrapper_declaration = vec![];
        let unwrapped_name = format!("__{}_Unwrapped", cpp_type.cpp_name());
        let backing_field_idx = t.element_type_index as usize;
        let backing_field_ty = &metadata.metadata_registration.types[backing_field_idx];

        let enum_base = cpp_type.cppify_name_il2cpp(ctx_collection, metadata, backing_field_ty, 0);

        wrapper_declaration.push(format!("enum class {unwrapped_name} : {enum_base} {{"));

        for (i, field) in t.fields(metadata.metadata).iter().enumerate() {
            let f_type = metadata
                .metadata_registration
                .types
                .get(field.type_index as usize)
                .unwrap();

            if f_type.is_static() {
                // enums static fields are always the enum values
                let field_index = FieldIndex::new(t.field_start.index() + i as u32);
                let f_name = field.name(metadata.metadata);
                let value =
                    Self::field_default_value(metadata, field_index).expect("Enum without value!");

                wrapper_declaration.push(format!("__E_{f_name} = {value},"));
            }
        }

        wrapper_declaration.push("};".into());

        cpp_type
            .declarations
            .push(CppMember::CppLine(CppLine::make(wrapper_declaration.join("\n"))).into());

        let wrapper = format!("{VALUE_WRAPPER_TYPE}<{VALUE_TYPE_WRAPPER_SIZE}>::instance");
        let operator_body = format!("return std::bit_cast<{unwrapped_name}>(this->{wrapper});");
        let operator_decl = CppMethodDecl {
            cpp_name: Default::default(),
            instance: true,
            return_type: unwrapped_name,

            brief: Some("Conversion into unwrapped enum value".to_string()),
            body: Some(vec![Arc::new(CppLine::make(operator_body))]), // TODO:
            is_const: true,
            is_constexpr: true,
            is_virtual: false,
            is_operator: true,
            is_inline: true,
            is_no_except: true, // TODO:
            parameters: vec![],
            prefix_modifiers: vec![],
            suffix_modifiers: vec![],
            template: None,
        };
        cpp_type
            .declarations
            .push(CppMember::MethodDecl(operator_decl).into());
    }

    fn create_valuetype_field_wrapper(&mut self) {
        let cpp_type = self.get_mut_cpp_type();
        if cpp_type.calculated_size.is_none() {
            todo!("Why does this type not have a valid size??? {:?}", cpp_type);
        }

        let size = cpp_type.calculated_size.unwrap();

        cpp_type.requirements.needs_byte_include();
        cpp_type.declarations.push(
            CppMember::FieldDecl(CppFieldDecl {
                cpp_name: VALUE_TYPE_WRAPPER_SIZE.to_string(),
                field_ty: "auto".to_string(),
                instance: false,
                readonly: false,
                const_expr: true,
                value: Some(format!("0x{size:x}")),
                brief_comment: Some("The size of the true value type".to_string()),
            })
            .into(),
        );

        cpp_type.declarations.push(
            CppMember::ConstructorDecl(CppConstructorDecl {
                cpp_name: cpp_type.cpp_name().clone(),
                parameters: vec![CppParam {
                    name: "instance".to_string(),
                    ty: format!("std::array<std::byte, {VALUE_TYPE_WRAPPER_SIZE}>"),
                    modifiers: Default::default(),
                    def_value: None,
                }],
                template: None,
                is_constexpr: true,
                is_explicit: true,
                is_default: false,
                is_no_except: true,
                base_ctor: Some((
                    cpp_type.inherit.get(0).unwrap().to_string(),
                    "instance".to_string(),
                )),
                initialized_values: Default::default(),
                brief: Some(
                    "Constructor that lets you initialize the internal array explicitly".into(),
                ),
                body: Some(vec![]),
            })
            .into(),
        );
    }

    fn create_valuetype_constructor(
        &mut self,
        metadata: &Metadata,
        ctx_collection: &CppContextCollection,
        config: &GenerationConfig,
        tdi: TypeDefinitionIndex,
    ) {
        let cpp_type = self.get_mut_cpp_type();

        let t = &metadata.metadata.global_metadata.type_definitions[tdi];

        let instance_fields = t
            .fields(metadata.metadata)
            .iter()
            .filter_map(|field| {
                let f_type = metadata
                    .metadata_registration
                    .types
                    .get(field.type_index as usize)
                    .unwrap();

                // ignore statics or constants
                if f_type.is_static() || f_type.is_constant() {
                    return None;
                }

                let f_type_cpp_name =
                    cpp_type.cppify_name_il2cpp(ctx_collection, metadata, f_type, 0);

                // Get the inner type of a Generic Inst
                // e.g ReadOnlySpan<char> -> ReadOnlySpan<T>
                let def_value = Self::type_default_value(metadata, Some(cpp_type), f_type);

                let f_cpp_name = config.name_cpp_plus(
                    field.name(metadata.metadata),
                    &[cpp_type.cpp_name().as_str()],
                );

                Some(CppParam {
                    name: f_cpp_name,
                    ty: f_type_cpp_name,
                    modifiers: "".to_string(),
                    // no default value for first param
                    def_value: Some(def_value),
                })
            })
            .collect_vec();

        if !instance_fields.is_empty() {
            // Maps into the first parent -> ""
            // so then Parent()
            let base_ctor = cpp_type.inherit.get(0).map(|s| (s.clone(), "".to_string()));

            let body: Vec<Arc<dyn Writable>> = instance_fields
                .iter()
                .map(|p| {
                    let name = &p.name;
                    CppLine::make(format!("this->{name} = {name};"))
                })
                .map(Arc::new)
                // Why is this needed? _sigh_
                .map(|arc| -> Arc<dyn Writable> { arc })
                .collect_vec();

            let params_no_def = instance_fields
                .iter()
                .cloned()
                .map(|mut c| {
                    c.def_value = None;
                    c
                })
                .collect_vec();

            let constructor_decl = CppConstructorDecl {
                cpp_name: cpp_type.cpp_name().clone(),
                template: None,
                is_constexpr: true,
                is_explicit: false,
                is_default: false,
                is_no_except: true,

                base_ctor,
                initialized_values: HashMap::new(),
                // initialize values with params
                // initialized_values: instance_fields
                //     .iter()
                //     .map(|p| (p.name.to_string(), p.name.to_string()))
                //     .collect(),
                parameters: params_no_def,
                brief: None,
                body: None,
            };

            let method_impl_template = if cpp_type
                .cpp_template
                .as_ref()
                .is_some_and(|c| !c.names.is_empty())
            {
                cpp_type.cpp_template.clone()
            } else {
                None
            };

            let constructor_impl = CppConstructorImpl {
                body,
                template: method_impl_template,
                parameters: instance_fields,
                declaring_full_name: cpp_type.cpp_name_components.combine_all(true),
                ..constructor_decl.clone().into()
            };

            cpp_type
                .declarations
                .push(CppMember::ConstructorDecl(constructor_decl).into());
            cpp_type
                .implementations
                .push(CppMember::ConstructorImpl(constructor_impl).into());
        }

        let cpp_name = cpp_type.cpp_name();

        let wrapper = format!("{VALUE_WRAPPER_TYPE}<{VALUE_TYPE_WRAPPER_SIZE}>::instance");
        cpp_type.declarations.push(
            CppMember::CppLine(CppLine {
                line: format!(
                    "
                    constexpr {cpp_name}({cpp_name} const&) = default;
                    constexpr {cpp_name}({cpp_name}&&) = default;
                    constexpr {cpp_name}& operator=({cpp_name} const& o) {{
                        this->{wrapper} = o.{wrapper};
                        return *this;
                    }};
                    constexpr {cpp_name}& operator=({cpp_name}&& o) noexcept {{
                        this->{wrapper} = std::move(o.{wrapper});
                        return *this;
                    }};
                "
                ),
            })
            .into(),
        );
    }

    fn create_ref_default_constructor(&mut self) {
        let cpp_type = self.get_mut_cpp_type();
        let cpp_name = cpp_type.cpp_name().clone();

        let cs_name = cpp_type.name().clone();

        // Skip if System.ValueType or System.Enum
        if cpp_type.namespace() == "System" && (cs_name == "ValueType" || cs_name == "Enum") {
            return;
        }

        cpp_type.declarations.push(
            CppMember::CppLine(CppLine {
                line: format!("virtual ~{cpp_name}() = default;"),
            })
            .into(),
        );

        let copy_ctor = CppConstructorDecl {
            cpp_name: cpp_name.clone(),
            parameters: vec![CppParam {
                name: "".to_string(),
                modifiers: " const&".to_string(),
                ty: cpp_name.clone(),
                def_value: None,
            }],
            template: None,
            is_constexpr: true,
            is_explicit: false,
            is_default: true,
            is_no_except: true,

            base_ctor: None,
            initialized_values: HashMap::new(),
            brief: None,
            body: Some(vec![]),
        };
        let move_ctor = CppConstructorDecl {
            cpp_name: cpp_name.clone(),
            parameters: vec![CppParam {
                name: "".to_string(),
                modifiers: "&&".to_string(),
                ty: cpp_name.clone(),
                def_value: None,
            }],
            template: None,
            is_constexpr: true,
            is_explicit: false,
            is_default: true,
            is_no_except: true,
            base_ctor: None,
            initialized_values: HashMap::new(),
            brief: None,
            body: Some(vec![]),
        };

        cpp_type
            .declarations
            .push(CppMember::ConstructorDecl(copy_ctor).into());
        cpp_type
            .declarations
            .push(CppMember::ConstructorDecl(move_ctor).into());

        // Delegates and such are reference types with no inheritance
        if cpp_type.inherit.is_empty() {
            return;
        }

        let base_type = cpp_type
            .inherit
            .get(0)
            .expect("No parent for reference type?");

        cpp_type.declarations.push(
            CppMember::ConstructorDecl(CppConstructorDecl {
                cpp_name: cpp_name.clone(),
                parameters: vec![CppParam {
                    name: "ptr".to_string(),
                    modifiers: "".to_string(),
                    ty: "void*".to_string(),
                    def_value: None,
                }],
                template: None,
                is_constexpr: true,
                is_explicit: true,
                is_default: false,
                is_no_except: true,

                base_ctor: Some((base_type.clone(), "ptr".to_string())),
                initialized_values: HashMap::new(),
                brief: None,
                body: Some(vec![]),
            })
            .into(),
        );
    }
    fn make_interface_constructors(&mut self) {
        let cpp_type = self.get_mut_cpp_type();
        let cpp_name = cpp_type.cpp_name().clone();

        cpp_type.declarations.push(
            CppMember::CppLine(CppLine {
                line: format!("~{cpp_name}() = default;"),
            })
            .into(),
        );

        let base_type = cpp_type
            .inherit
            .get(0)
            .expect("No parent for interface type?");

        cpp_type.declarations.push(
            CppMember::ConstructorDecl(CppConstructorDecl {
                cpp_name: cpp_name.clone(),
                parameters: vec![CppParam {
                    name: "ptr".to_string(),
                    modifiers: "".to_string(),
                    ty: "void*".to_string(),
                    def_value: None,
                }],
                template: None,
                is_constexpr: true,
                is_explicit: true,
                is_default: false,
                is_no_except: true,

                base_ctor: Some((base_type.clone(), "ptr".to_string())),
                initialized_values: HashMap::new(),
                brief: None,
                body: Some(vec![]),
            })
            .into(),
        );
    }
    fn create_ref_default_operators(&mut self) {
        let cpp_type = self.get_mut_cpp_type();
        let cpp_name = cpp_type.cpp_name();

        // Skip if System.ValueType or System.Enum
        if cpp_type.namespace() == "System"
            && (cpp_type.cpp_name() == "ValueType" || cpp_type.cpp_name() == "Enum")
        {
            return;
        }

        // Delegates and such are reference types with no inheritance
        if cpp_type.inherit.is_empty() {
            return;
        }

        cpp_type.declarations.push(
            CppMember::CppLine(CppLine {
                line: format!(
                    "
  constexpr {cpp_name}& operator=(std::nullptr_t) noexcept {{
    this->{REFERENCE_WRAPPER_INSTANCE_NAME} = nullptr;
    return *this;
  }};

  constexpr {cpp_name}& operator=(void* o) noexcept {{
    this->{REFERENCE_WRAPPER_INSTANCE_NAME} = o;
    return *this;
  }};

  constexpr {cpp_name}& operator=({cpp_name}&& o) noexcept = default;
  constexpr {cpp_name}& operator=({cpp_name} const& o) noexcept = default;
                "
                ),
            })
            .into(),
        );
    }

    fn create_ref_constructor(
        cpp_type: &mut CppType,
        declaring_type: &Il2CppTypeDefinition,
        m_params: &[CppParam],
        template: &Option<CppTemplate>,
    ) {
        if declaring_type.is_value_type() || declaring_type.is_enum_type() {
            return;
        }

        let params_no_default = m_params
            .iter()
            .cloned()
            .map(|mut c| {
                c.def_value = None;
                c
            })
            .collect_vec();

        let ty_full_cpp_name = format!("::{}", cpp_type.cpp_name_components.combine_all(true));

        let decl: CppMethodDecl = CppMethodDecl {
            cpp_name: "New_ctor".into(),
            return_type: ty_full_cpp_name.clone(),
            parameters: params_no_default,
            template: template.clone(),
            body: None, // TODO:
            brief: None,
            is_no_except: false,
            is_constexpr: false,
            instance: false,
            is_const: false,
            is_operator: false,
            is_virtual: false,
            is_inline: true,
            prefix_modifiers: vec![],
            suffix_modifiers: vec![],
        };

        // To avoid trailing ({},)
        let base_ctor_params = CppParam::params_names(&decl.parameters).join(", ");

        let allocate_call =
            format!("THROW_UNLESS(::il2cpp_utils::New<{ty_full_cpp_name}>({base_ctor_params}))");

        let declaring_template = if cpp_type
            .cpp_template
            .as_ref()
            .is_some_and(|t| !t.names.is_empty())
        {
            cpp_type.cpp_template.clone()
        } else {
            None
        };

        let cpp_constructor_impl = CppMethodImpl {
            body: vec![
                Arc::new(CppLine::make(format!(
                    "{ty_full_cpp_name} _cordl_instantiated_o{{{allocate_call}}};"
                ))),
                Arc::new(CppLine::make("return _cordl_instantiated_o;".into())),
            ],

            declaring_cpp_full_name: cpp_type.cpp_name_components.combine_all(true),
            parameters: m_params.to_vec(),
            template: declaring_template,
            ..decl.clone().into()
        };

        cpp_type
            .implementations
            .push(CppMember::MethodImpl(cpp_constructor_impl).into());

        cpp_type
            .declarations
            .push(CppMember::MethodDecl(decl).into());
    }

    fn create_method(
        &mut self,
        declaring_type: &Il2CppTypeDefinition,
        method_index: MethodIndex,

        metadata: &Metadata,
        ctx_collection: &CppContextCollection,
        config: &GenerationConfig,
        is_generic_method_inst: bool,
    ) {
        let method = &metadata.metadata.global_metadata.methods[method_index];
        let cpp_type = self.get_mut_cpp_type();

        // TODO: sanitize method name for c++
        let m_name = method.name(metadata.metadata);
        if m_name == ".cctor" {
            // info!("Skipping {}", m_name);
            return;
        }

        let m_ret_type = metadata
            .metadata_registration
            .types
            .get(method.return_type as usize)
            .unwrap();

        let mut m_params_with_def: Vec<CppParam> =
            Vec::with_capacity(method.parameter_count as usize);

        for (pi, param) in method.parameters(metadata.metadata).iter().enumerate() {
            let param_index = ParameterIndex::new(method.parameter_start.index() + pi as u32);
            let param_type = metadata
                .metadata_registration
                .types
                .get(param.type_index as usize)
                .unwrap();

            let def_value = Self::param_default_value(metadata, param_index);

            let make_param_cpp_type_name = |cpp_type: &mut CppType| -> String {
                let full_name = param_type.full_name(metadata.metadata);
                if full_name == "System.Enum" {
                    ENUM_PTR_TYPE.into()
                } else if full_name == "System.ValueType" {
                    VT_PTR_TYPE.into()
                } else {
                    cpp_type.cppify_name_il2cpp(ctx_collection, metadata, param_type, 0)
                }
            };

            let mut param_cpp_name = {
                let fixup_name = match is_generic_method_inst {
                    false => cpp_type.il2cpp_mvar_use_param_name(
                        metadata,
                        method_index,
                        make_param_cpp_type_name,
                        param_type,
                    ),
                    true => make_param_cpp_type_name(cpp_type),
                };

                cpp_type.il2cpp_byref(fixup_name, param_type)
            };

            m_params_with_def.push(CppParam {
                name: config.name_cpp(param.name(metadata.metadata)),
                def_value,
                ty: param_cpp_name,
                modifiers: "".to_string(),
            });
        }

        let m_params_no_def: Vec<CppParam> = m_params_with_def
            .iter()
            .cloned()
            .map(|mut p| {
                p.def_value = None;
                p
            })
            .collect_vec();

        // TODO: Add template<typename ...> if a generic inst e.g
        // T UnityEngine.Component::GetComponent<T>() -> bs_hook::Il2CppWrapperType UnityEngine.Component::GetComponent()
        let template = if method.generic_container_index.is_valid() {
            match is_generic_method_inst {
                true => Some(CppTemplate { names: vec![] }),
                false => {
                    let generics = method
                        .generic_container(metadata.metadata)
                        .unwrap()
                        .generic_parameters(metadata.metadata)
                        .iter()
                        .map(|param| param.name(metadata.metadata).to_string());

                    Some(CppTemplate::make_typenames(generics))
                }
            }
        } else {
            None
        };

        let declaring_type_template = if cpp_type
            .cpp_template
            .as_ref()
            .is_some_and(|t| !t.names.is_empty())
        {
            cpp_type.cpp_template.clone()
        } else {
            None
        };

        let literal_types = if is_generic_method_inst {
            cpp_type
                .method_generic_instantiation_map
                .get(&method_index)
                .cloned()
        } else {
            None
        };

        let resolved_generic_types = literal_types.map(|literal_types| {
            literal_types
                .iter()
                .map(|t| &metadata.metadata_registration.types[*t as usize])
                .map(|t| cpp_type.cppify_name_il2cpp(ctx_collection, metadata, t, 0))
                .collect_vec()
        });

        // Lazy cppify
        let make_ret_cpp_type_name = |cpp_type: &mut CppType| -> String {
            let full_name = m_ret_type.full_name(metadata.metadata);
            if full_name == "System.Enum" {
                ENUM_PTR_TYPE.into()
            } else if full_name == "System.ValueType" {
                VT_PTR_TYPE.into()
            } else {
                cpp_type.cppify_name_il2cpp(ctx_collection, metadata, m_ret_type, 0)
            }
        };

        let m_ret_cpp_type_name = {
            let fixup_name = match is_generic_method_inst {
                false => cpp_type.il2cpp_mvar_use_param_name(
                    metadata,
                    method_index,
                    make_ret_cpp_type_name,
                    m_ret_type,
                ),
                true => make_ret_cpp_type_name(cpp_type),
            };

            cpp_type.il2cpp_byref(fixup_name, m_ret_type)
        };

        // Reference type constructor
        if m_name == ".ctor" {
            Self::create_ref_constructor(cpp_type, declaring_type, &m_params_with_def, &template);
        }
        let cpp_m_name = {
            let cpp_m_name = config.name_cpp(m_name);

            // static functions with same name and params but
            // different ret types can exist
            // so we add their ret types
            let fixup_name = match cpp_m_name == "op_Implicit" || cpp_m_name == "op_Explicit" {
                true => cpp_m_name + "_" + &config.generic_nested_name(&m_ret_cpp_type_name),
                false => cpp_m_name,
            };

            match &resolved_generic_types {
                Some(resolved_generic_types) => {
                    format!("{fixup_name}<{}>", resolved_generic_types.join(", "))
                }
                None => fixup_name,
            }
        };

        let declaring_type = method.declaring_type(metadata.metadata);
        let tag = CppTypeTag::TypeDefinitionIndex(method.declaring_type);

        let method_calc = metadata.method_calculations.get(&method_index);

        // generic methods don't have definitions if not an instantiation
        let method_stub = !is_generic_method_inst && template.is_some();

        let method_decl = CppMethodDecl {
            body: None,
            brief: format!(
                "Method {m_name} addr 0x{:x} size 0x{:x} virtual {} final {}",
                method_calc.map(|m| m.addrs).unwrap_or(u64::MAX),
                method_calc.map(|m| m.estimated_size).unwrap_or(usize::MAX),
                method.is_virtual_method(),
                method.is_final_method()
            )
            .into(),
            is_const: false,
            is_constexpr: false,
            is_no_except: false,
            cpp_name: cpp_m_name.clone(),
            return_type: m_ret_cpp_type_name.clone(),
            parameters: m_params_no_def.clone(),
            instance: !method.is_static_method(),
            template: template.clone(),
            suffix_modifiers: Default::default(),
            prefix_modifiers: Default::default(),
            is_virtual: false,
            is_operator: false,
            is_inline: true,
        };

        let instance_ptr: String = if method.is_static_method() {
            "nullptr".into()
        } else {
            "*this".into()
        };

        const METHOD_INFO_VAR_NAME: &str = "___internal_method";

        let method_invoke_params = vec![instance_ptr.as_str(), METHOD_INFO_VAR_NAME];
        let param_names = CppParam::params_names(&method_decl.parameters).map(|s| s.as_str());
        let declaring_type_cpp_full_name = cpp_type.cpp_name_components.combine_all(true);
        let declaring_classof_call = format!("::il2cpp_utils::il2cpp_type_check::il2cpp_no_arg_class<::{declaring_type_cpp_full_name}>::get()");

        let params_types_format: String = CppParam::params_types(&method_decl.parameters)
            .map(|t| format!("::il2cpp_utils::il2cpp_type_check::il2cpp_no_arg_type<{t}>::get()"))
            .join(", ");

        let method_info_lines = match &template {
            Some(template) => {
                // generic
                let template_names = template
                    .just_names()
                    .map(|t| {
                        format!(
                            "::il2cpp_utils::il2cpp_type_check::il2cpp_no_arg_class<{t}>::get()"
                        )
                    })
                    .join(", ");

                vec![
                    format!("static auto* ___internal_method_base = THROW_UNLESS((::il2cpp_utils::FindMethod(
                        {declaring_classof_call},
                        \"{m_name}\",
                        std::vector<Il2CppClass*>{{{template_names}}},
                        ::std::vector<const Il2CppType*>{{{params_types_format}}}
                    )));"),
                    format!("static auto* {METHOD_INFO_VAR_NAME} = THROW_UNLESS(::il2cpp_utils::MakeGenericMethod(
                        ___internal_method_base,
                         std::vector<Il2CppClass*>{{{template_names}}}
                        ));"),
                ]
            }
            None => {
                vec![
                    format!("static auto* {METHOD_INFO_VAR_NAME} = THROW_UNLESS((::il2cpp_utils::FindMethod(
                            {declaring_classof_call},
                            \"{m_name}\",
                            std::vector<Il2CppClass*>{{}},
                            ::std::vector<const Il2CppType*>{{{params_types_format}}}
                        )));"),
                ]
            }
        };

        let method_body_lines = [format!(
            "return ::cordl_internals::RunMethodRethrow<{m_ret_cpp_type_name}, false>({});",
            method_invoke_params
                .into_iter()
                .chain(param_names)
                .join(", ")
        )];

        //   static auto ___internal__logger = ::Logger::get().WithContext("::Org::BouncyCastle::Crypto::Parameters::DHPrivateKeyParameters::Equals");
        //   auto* ___internal__method = THROW_UNLESS((::il2cpp_utils::FindMethod(this, "Equals", std::vector<Il2CppClass*>{}, ::std::vector<const Il2CppType*>{::il2cpp_utils::ExtractType(obj)})));
        //   return ::il2cpp_utils::RunMethodRethrow<bool, false>(this, ___internal__method, obj);

        let method_impl = CppMethodImpl {
            body: method_info_lines
                .iter()
                .chain(method_body_lines.iter())
                .cloned()
                .map(|l| -> Arc<dyn Writable> { Arc::new(CppLine::make(l)) })
                .collect_vec(),
            parameters: m_params_with_def.clone(),
            brief: None,
            declaring_cpp_full_name: declaring_type_cpp_full_name,
            instance: !method.is_static_method(),
            suffix_modifiers: Default::default(),
            prefix_modifiers: Default::default(),
            template: template.clone(),
            declaring_type_template: declaring_type_template.clone(),

            // defaults
            ..method_decl.clone().into()
        };

        // check if declaring type is the current type or the interface
        // we check TDI because if we are a generic instantiation
        // we just use ourselves if the declaring type is also the same TDI
        let interface_declaring_cpp_type: Option<&CppType> =
            if tag.get_tdi() == cpp_type.self_tag.get_tdi() {
                Some(cpp_type)
            } else {
                ctx_collection.get_cpp_type(tag)
            };

        // don't emit method size structs for generic methods

        // don't emit method size structs for generic methods

        // if type is a generic
        let has_template_args = cpp_type
            .cpp_template
            .as_ref()
            .is_some_and(|t| !t.names.is_empty());

        // don't emit method size structs for generic methods
        if let Some(method_calc) = method_calc && template.is_none() && !has_template_args && !is_generic_method_inst {
            cpp_type
                .nonmember_implementations
                .push(Rc::new(CppMethodSizeStruct {
                    ret_ty: method_decl.return_type.clone(),
                    cpp_method_name: method_decl.cpp_name.clone(),
                    method_name: m_name.to_string(),
                    declaring_type_name: method_impl.declaring_cpp_full_name.clone(),
                    method_info_lines,
                    method_info_var: METHOD_INFO_VAR_NAME.to_string(),
                    instance: method_decl.instance,
                    params: method_decl.parameters.clone(),
                    template: template.clone(),
                    generic_literals: resolved_generic_types,
                    method_data: CppMethodData {
                        addrs: method_calc.addrs,
                        estimated_size: method_calc.estimated_size,
                    },
                    interface_clazz_of: interface_declaring_cpp_type
                        .map(|d| d.classof_cpp_name())
                        .unwrap_or_else(|| format!("Bad stuff happened {declaring_type:?}")),
                    is_final: method.is_final_method(),
                    slot: if method.slot != u16::MAX {
                        Some(method.slot)
                    } else {
                        None
                    },
                }));
        }

        // TODO: Revise this
        const ALLOW_GENERIC_METHOD_STUBS_IMPL: bool = true;
        // If a generic instantiation or not a template
        if !method_stub || ALLOW_GENERIC_METHOD_STUBS_IMPL {
            cpp_type
                .implementations
                .push(CppMember::MethodImpl(method_impl).into());
        }

        if !is_generic_method_inst {
            cpp_type
                .declarations
                .push(CppMember::MethodDecl(method_decl).into());
        }
    }

    fn default_value_blob(
        metadata: &Metadata,
        ty: &Il2CppType,
        data_index: usize,
        string_quotes: bool,
        string_as_u16: bool,
    ) -> String {
        let data = &metadata
            .metadata
            .global_metadata
            .field_and_parameter_default_value_data
            .as_vec()[data_index..];

        let mut cursor = Cursor::new(data);

        const UNSIGNED_SUFFIX: &str = "u";
        match ty.ty {
            Il2CppTypeEnum::Boolean => (if data[0] == 0 { "false" } else { "true" }).to_string(),
            Il2CppTypeEnum::I1 => {
                format!("static_cast<int8_t>(0x{:x})", cursor.read_i8().unwrap())
            }
            Il2CppTypeEnum::I2 => {
                format!(
                    "static_cast<int16_t>(0x{:x})",
                    cursor.read_i16::<Endian>().unwrap()
                )
            }
            Il2CppTypeEnum::I4 => {
                format!(
                    "static_cast<int32_t>(0x{:x})",
                    cursor.read_compressed_i32::<Endian>().unwrap()
                )
            }
            // TODO: We assume 64 bit
            Il2CppTypeEnum::I | Il2CppTypeEnum::I8 => {
                format!(
                    "static_cast<int64_t>(0x{:x})",
                    cursor.read_i64::<Endian>().unwrap()
                )
            }
            Il2CppTypeEnum::U1 => {
                format!(
                    "static_cast<uint8_t>(0x{:x}{UNSIGNED_SUFFIX})",
                    cursor.read_u8().unwrap()
                )
            }
            Il2CppTypeEnum::U2 => {
                format!(
                    "static_cast<uint16_t>(0x{:x}{UNSIGNED_SUFFIX})",
                    cursor.read_u16::<Endian>().unwrap()
                )
            }
            Il2CppTypeEnum::U4 => {
                format!(
                    "static_cast<uint32_t>(0x{:x}{UNSIGNED_SUFFIX})",
                    cursor.read_u32::<Endian>().unwrap()
                )
            }
            // TODO: We assume 64 bit
            Il2CppTypeEnum::U | Il2CppTypeEnum::U8 => {
                format!(
                    "static_cast<uint64_t>(0x{:x}{UNSIGNED_SUFFIX})",
                    cursor.read_u64::<Endian>().unwrap()
                )
            }
            // https://learn.microsoft.com/en-us/nimbusml/concepts/types
            // https://en.cppreference.com/w/cpp/types/floating-point
            Il2CppTypeEnum::R4 => {
                let val = format!("{}", cursor.read_f32::<Endian>().unwrap());
                if !val.contains('.')
                    && val
                        .find(|c: char| !c.is_ascii_digit() && c != '-')
                        .is_none()
                {
                    val + ".0"
                } else {
                    val.replace("inf", "INFINITY").replace("NaN", "NAN")
                }
            }
            Il2CppTypeEnum::R8 => {
                let val = format!("{}", cursor.read_f64::<Endian>().unwrap());
                if !val.contains('.')
                    && val
                        .find(|c: char| !c.is_ascii_digit() && c != '-')
                        .is_none()
                {
                    val + ".0"
                } else {
                    val.replace("inf", "INFINITY").replace("NaN", "NAN")
                }
            }
            Il2CppTypeEnum::Char => {
                let res = String::from_utf16_lossy(&[cursor.read_u16::<Endian>().unwrap()])
                    .escape_default()
                    .to_string();

                if string_quotes {
                    let literal_prefix = if string_as_u16 { "u" } else { "" };
                    return format!("{literal_prefix}'{res}'");
                }

                res
            }
            Il2CppTypeEnum::String => {
                // UTF-16 byte array len
                // which means the len is 2x the size of the string's len
                let stru16_len = cursor.read_compressed_i32::<Endian>().unwrap();
                if stru16_len == -1 {
                    return "".to_string();
                }

                let mut buf = vec![0u8; stru16_len as usize];

                cursor.read_exact(buf.as_mut_slice()).unwrap();

                let res = String::from_utf8(buf).unwrap().escape_default().to_string();

                if string_quotes {
                    let literal_prefix = if string_as_u16 { "u" } else { "" };
                    return format!("{literal_prefix}\"{res}\"");
                }

                res
            }
            // Il2CppTypeEnum::Genericinst => match ty.data {
            //     TypeData::GenericClassIndex(inst_idx) => {
            //         let gen_class = &metadata
            //             .metadata
            //             .runtime_metadata
            //             .metadata_registration
            //             .generic_classes[inst_idx];

            //         let inner_ty = &metadata.metadata_registration.types[gen_class.type_index];

            //         Self::default_value_blob(
            //             metadata,
            //             inner_ty,
            //             data_index,
            //             string_quotes,
            //             string_as_u16,
            //         )
            //     }
            //     _ => todo!(),
            // },
            Il2CppTypeEnum::Genericinst
            | Il2CppTypeEnum::Byref
            | Il2CppTypeEnum::Ptr
            | Il2CppTypeEnum::Array
            | Il2CppTypeEnum::Object
            | Il2CppTypeEnum::Class
            | Il2CppTypeEnum::Szarray => {
                let def = Self::type_default_value(metadata, None, ty);
                format!("/* TODO: Fix these default values */ {ty:?} */ {def}")
            }

            _ => "unknown".to_string(),
        }
    }

    fn unbox_nullable_valuetype<'a>(metadata: &'a Metadata, ty: &'a Il2CppType) -> &'a Il2CppType {
        if let Il2CppTypeEnum::Valuetype = ty.ty {
            match ty.data {
                TypeData::TypeDefinitionIndex(tdi) => {
                    let type_def = &metadata.metadata.global_metadata.type_definitions[tdi];

                    // System.Nullable`1
                    if type_def.name(metadata.metadata) == "Nullable`1"
                        && type_def.namespace(metadata.metadata) == "System"
                    {
                        return metadata
                            .metadata_registration
                            .types
                            .get(type_def.byval_type_index as usize)
                            .unwrap();
                    }
                }
                _ => todo!(),
            }
        }

        ty
    }

    fn type_default_value(
        metadata: &Metadata,
        cpp_type: Option<&CppType>,
        ty: &Il2CppType,
    ) -> String {
        let matched_ty: &Il2CppType = match ty.data {
            // get the generic inst
            TypeData::GenericClassIndex(inst_idx) => {
                let gen_class = &metadata
                    .metadata
                    .runtime_metadata
                    .metadata_registration
                    .generic_classes[inst_idx];

                &metadata.metadata_registration.types[gen_class.type_index]
            }
            // get the underlying type of the generic param
            TypeData::GenericParameterIndex(param) => match param.is_valid() {
                true => {
                    let gen_param = &metadata.metadata.global_metadata.generic_parameters[param];

                    cpp_type
                        .and_then(|cpp_type| {
                            cpp_type
                                .generic_instantiations_args_types
                                .as_ref()
                                .and_then(|gen_args| gen_args.get(gen_param.num as usize))
                                .map(|t| &metadata.metadata_registration.types[*t])
                        })
                        .unwrap_or(ty)
                }
                false => ty,
            },
            _ => ty,
        };

        match matched_ty.valuetype {
            true => "{}".to_string(),
            false => "csnull".to_string(),
        }
    }

    fn field_default_value(metadata: &Metadata, field_index: FieldIndex) -> Option<String> {
        metadata
            .metadata
            .global_metadata
            .field_default_values
            .as_vec()
            .iter()
            .find(|f| f.field_index == field_index)
            .map(|def| {
                let ty: &Il2CppType = metadata
                    .metadata_registration
                    .types
                    .get(def.type_index as usize)
                    .unwrap();

                // get default value for given type
                if !def.data_index.is_valid() {
                    return Self::type_default_value(metadata, None, ty);
                }

                Self::default_value_blob(metadata, ty, def.data_index.index() as usize, true, true)
            })
    }
    fn param_default_value(metadata: &Metadata, parameter_index: ParameterIndex) -> Option<String> {
        metadata
            .metadata
            .global_metadata
            .parameter_default_values
            .as_vec()
            .iter()
            .find(|p| p.parameter_index == parameter_index)
            .map(|def| {
                let mut ty = metadata
                    .metadata_registration
                    .types
                    .get(def.type_index as usize)
                    .unwrap();

                ty = Self::unbox_nullable_valuetype(metadata, ty);

                // This occurs when the type is `null` or `default(T)` for value types
                if !def.data_index.is_valid() {
                    return Self::type_default_value(metadata, None, ty);
                }

                if let Il2CppTypeEnum::Valuetype = ty.ty {
                    match ty.data {
                        TypeData::TypeDefinitionIndex(tdi) => {
                            let type_def = &metadata.metadata.global_metadata.type_definitions[tdi];

                            // System.Nullable`1
                            if type_def.name(metadata.metadata) == "Nullable`1"
                                && type_def.namespace(metadata.metadata) == "System"
                            {
                                ty = metadata
                                    .metadata_registration
                                    .types
                                    .get(type_def.byval_type_index as usize)
                                    .unwrap();
                            }
                        }
                        _ => todo!(),
                    }
                }

                Self::default_value_blob(metadata, ty, def.data_index.index() as usize, true, true)
            })
    }

    fn il2cpp_byref(&mut self, cpp_name: String, typ: &Il2CppType) -> String {
        let requirements = &mut self.get_mut_cpp_type().requirements;
        // handle out T or
        // ref T when T is a value type

        // typ.valuetype -> false when T&
        // apparently even if `T` is a valuetype
        if typ.is_param_out() || (typ.byref && !typ.valuetype) {
            requirements.needs_byref_include();
            return format!("ByRef<{cpp_name}>");
        }

        if typ.is_param_in() {
            requirements.needs_byref_include();

            return format!("ByRefConst<{cpp_name}>");
        }

        cpp_name
    }

    // Basically decides to use the template param name (if applicable)
    // instead of the generic instantiation of the type
    // TODO: Make this less confusing
    fn il2cpp_mvar_use_param_name<'a>(
        &mut self,
        metadata: &'a Metadata,
        method_index: MethodIndex,
        // use a lambda to do this lazily
        cpp_name: impl FnOnce(&mut CppType) -> String,
        typ: &'a Il2CppType,
    ) -> String {
        let tys = self
            .get_mut_cpp_type()
            .method_generic_instantiation_map
            .remove(&method_index);

        // fast path for generic param name
        // otherwise cpp_name() will default to generic param anyways
        let ret = match typ.ty {
            Il2CppTypeEnum::Mvar => match typ.data {
                TypeData::GenericParameterIndex(index) => {
                    let generic_param =
                        &metadata.metadata.global_metadata.generic_parameters[index];

                    let owner = generic_param.owner(metadata.metadata);
                    assert!(owner.is_method != u32::MAX);

                    generic_param.name(metadata.metadata).to_string()
                }
                _ => todo!(),
            },
            _ => cpp_name(self.get_mut_cpp_type()),
        };

        if let Some(tys) = tys {
            self.get_mut_cpp_type()
                .method_generic_instantiation_map
                .insert(method_index, tys);
        }

        ret
    }

    fn cppify_name_il2cpp(
        &mut self,
        ctx_collection: &CppContextCollection,
        metadata: &Metadata,
        typ: &Il2CppType,
        include_depth: usize,
    ) -> String {
        let cpp_type = self.get_mut_cpp_type();

        let mut requirements = cpp_type.requirements.clone();

        let res = cpp_type.cppify_name_il2cpp_recurse(
            &mut requirements,
            ctx_collection,
            metadata,
            typ,
            include_depth,
            cpp_type.generic_instantiations_args_types.as_ref(),
        );

        cpp_type.requirements = requirements;

        res
    }

    fn cppify_name_il2cpp_recurse(
        &self,
        requirements: &mut CppTypeRequirements,
        ctx_collection: &CppContextCollection,
        metadata: &Metadata,
        typ: &Il2CppType,
        include_depth: usize,
        declaring_generic_inst_types: Option<&Vec<usize>>,
    ) -> String {
        let add_include = include_depth > 0;
        let next_include_depth = if add_include { include_depth - 1 } else { 0 };

        let typ_tag = typ.data;

        let cpp_type = self.get_cpp_type();

        match typ.ty {
            Il2CppTypeEnum::I1
            | Il2CppTypeEnum::U1
            | Il2CppTypeEnum::I2
            | Il2CppTypeEnum::U2
            | Il2CppTypeEnum::I4
            | Il2CppTypeEnum::U4
            | Il2CppTypeEnum::I8
            | Il2CppTypeEnum::U8
            | Il2CppTypeEnum::I
            | Il2CppTypeEnum::U => {
                requirements.needs_int_include();
            }
            Il2CppTypeEnum::R4 | Il2CppTypeEnum::R8 => {
                requirements.needs_math_include();
            }
            _ => (),
        };

        let ret = match typ.ty {
            Il2CppTypeEnum::Object => {
                requirements.need_wrapper();
                OBJECT_WRAPPER_TYPE.to_string()
            }
            Il2CppTypeEnum::Valuetype | Il2CppTypeEnum::Class | Il2CppTypeEnum::Typedbyref => {
                let typ_cpp_tag: CppTypeTag = typ_tag.into();
                // Self

                // we add :: here since we can't add it to method ddefinitions
                // e.g void ::Foo::method() <- not allowed
                if typ_cpp_tag == cpp_type.self_tag {
                    return format!("::{}", cpp_type.cpp_name_components.combine_all(false));
                }

                if let TypeData::TypeDefinitionIndex(tdi) = typ.data {
                    let td = &metadata.metadata.global_metadata.type_definitions[tdi];

                    // TODO: Do we need generic inst types here? Hopefully not!
                    let size = offsets::get_sizeof_type(td, tdi, None, metadata);

                    if metadata.blacklisted_types.contains(&tdi) {
                        return wrapper_type_for_tdi(td).to_string();
                    }

                    // if td.namespace(metadata.metadata) == "System"
                    //     && td.name(metadata.metadata) == "ValueType"
                    // {
                    //     requirements.needs_value_include();
                    //     // FIXME: get the correct size!
                    //     return format!("{VALUE_WRAPPER_TYPE}<0x{size:x}>");
                    // }
                    // TODO: Should System.Enum be a enum or ref type?
                    // if td.namespace(metadata.metadata) == "System"
                    //     && td.name(metadata.metadata) == "Enum"
                    // {
                    //     requirements.needs_enum_include();
                    //     // FIXME: get the correct size!
                    //     return format!("{ENUM_WRAPPER_TYPE}<0x{size:x}>");
                    // }
                }

                if add_include {
                    requirements.add_dependency_tag(typ_cpp_tag);
                }

                // In this case, just inherit the type
                // But we have to:
                // - Determine where to include it from
                let to_incl = ctx_collection.get_context(typ_cpp_tag).unwrap_or_else(|| {
                    let t = &metadata.metadata.global_metadata.type_definitions
                        [Self::get_tag_tdi(typ.data)];

                    panic!(
                        "no context for type {typ:?} {}",
                        t.full_name(metadata.metadata, true)
                    )
                });

                let other_context_ty = ctx_collection.get_context_root_tag(typ_cpp_tag);
                let own_context_ty = ctx_collection.get_context_root_tag(cpp_type.self_tag);

                let inc = CppInclude::new_context_typedef(to_incl);
                let to_incl_cpp_ty = ctx_collection
                    .get_cpp_type(typ.data.into())
                    .unwrap_or_else(|| panic!("Unable to get type to include {:?}", typ.data));

                let own_context = other_context_ty == own_context_ty;

                // - Include it
                // Skip including the context if we're already in it
                if add_include && !own_context {
                    requirements.add_include(Some(to_incl_cpp_ty), inc.clone());
                } else if !add_include && !own_context {
                    // Forward declare it
                    if to_incl_cpp_ty.nested {
                        // TODO: What should we do here?
                        error!("Can't forward declare nested type! Including!");
                        requirements.add_include(Some(to_incl_cpp_ty), inc);
                    } else {
                        requirements.add_forward_declare((
                            CppForwardDeclare::from_cpp_type(to_incl_cpp_ty),
                            inc,
                        ));
                    }
                }

                // we add :: here since we can't add it to method ddefinitions
                // e.g void ::Foo::method() <- not allowed
                format!(
                    "::{}",
                    to_incl_cpp_ty.cpp_name_components.combine_all(false)
                )
            }
            // Single dimension array
            Il2CppTypeEnum::Szarray => {
                requirements.needs_arrayw_include();

                let generic: String = match typ.data {
                    TypeData::TypeIndex(e) => {
                        let ty = &metadata.metadata_registration.types[e];
                        cpp_type.cppify_name_il2cpp_recurse(
                            requirements,
                            ctx_collection,
                            metadata,
                            ty,
                            include_depth,
                            declaring_generic_inst_types,
                        )
                    }

                    _ => panic!("Unknown type data for array {typ:?}!"),
                };

                format!("::ArrayW<{generic}>")
            }
            // multi dimensional array
            Il2CppTypeEnum::Array => {
                // FIXME: when stack further implements the TypeData::ArrayType we can actually implement this fully to be a multidimensional array, whatever that might mean
                warn!("Multidimensional array was requested but this is not implemented, typ: {typ:?}");
                "::bs_hook::Il2CppWrapperType".to_string()
            }
            Il2CppTypeEnum::Mvar => match typ.data {
                TypeData::GenericParameterIndex(index) => {
                    let generic_param: &brocolib::global_metadata::Il2CppGenericParameter =
                        &metadata.metadata.global_metadata.generic_parameters[index];

                    let owner = generic_param.owner(metadata.metadata);
                    assert!(owner.is_method != u32::MAX);

                    let (_gen_param_idx, gen_param) = owner
                        .generic_parameters(metadata.metadata)
                        .iter()
                        .find_position(|&p| p.name_index == generic_param.name_index)
                        .unwrap();

                    let method_index = MethodIndex::new(owner.owner_index);
                    let _method = &metadata.metadata.global_metadata.methods[method_index];

                    let method_args_opt =
                        cpp_type.method_generic_instantiation_map.get(&method_index);

                    if method_args_opt.is_none() {
                        return gen_param.name(metadata.metadata).to_string();
                    }

                    let method_args = method_args_opt.unwrap();

                    let ty_idx = method_args[gen_param.num as usize];
                    let ty = metadata
                        .metadata_registration
                        .types
                        .get(ty_idx as usize)
                        .unwrap();

                    cpp_type.cppify_name_il2cpp_recurse(
                        requirements,
                        ctx_collection,
                        metadata,
                        ty,
                        include_depth,
                        declaring_generic_inst_types,
                    )
                }
                _ => todo!(),
            },
            Il2CppTypeEnum::Var => match typ.data {
                // Il2CppMetadataGenericParameterHandle
                TypeData::GenericParameterIndex(index) => {
                    let generic_param: &brocolib::global_metadata::Il2CppGenericParameter =
                        &metadata.metadata.global_metadata.generic_parameters[index];

                    let owner = generic_param.owner(metadata.metadata);
                    let (_gen_param_idx, _gen_param) = owner
                        .generic_parameters(metadata.metadata)
                        .iter()
                        .find_position(|&p| p.name_index == generic_param.name_index)
                        .unwrap();

                    let ty_idx_opt = cpp_type
                        .generic_instantiations_args_types
                        .as_ref()
                        .and_then(|args| args.get(generic_param.num as usize))
                        .cloned();

                    // if template arg is not found
                    if ty_idx_opt.is_none() {
                        let gen_name = generic_param.name(metadata.metadata);

                        // true if the type is intentionally a generic template type and not a specialization
                        let has_generic_template =
                            cpp_type.cpp_template.as_ref().is_some_and(|template| {
                                template.just_names().any(|name| name == gen_name)
                            });

                        return match has_generic_template {
                            true => gen_name.to_string(),
                            false => panic!("/* TODO: FIX THIS, THIS SHOULDN'T HAPPEN! NO GENERIC INST ARGS FOUND HERE */ {gen_name}"),
                        };
                    }

                    cpp_type
                        .cpp_name_components
                        .generics
                        .as_ref()
                        .expect("Generic instantiation args not made yet!")
                        .get(generic_param.num as usize)
                        .expect("No generic parameter at index found!")
                        .clone()

                    // This is for calculating on the fly
                    // which is slower and won't work for the reference type lookup fix
                    // we do in make_generic_args

                    // let ty_idx = ty_idx_opt.unwrap();

                    // let ty = metadata
                    //     .metadata_registration
                    //     .types
                    //     .get(ty_idx as usize)
                    //     .unwrap();
                    // self.cppify_name_il2cpp(ctx_collection, metadata, ty, add_include)
                }
                _ => todo!(),
            },
            Il2CppTypeEnum::Genericinst => match typ.data {
                TypeData::GenericClassIndex(e) => {
                    let mr = &metadata.metadata_registration;
                    let generic_class = mr.generic_classes.get(e).unwrap();
                    let generic_inst = mr
                        .generic_insts
                        .get(generic_class.context.class_inst_idx.unwrap())
                        .unwrap();

                    let new_generic_inst_types = &generic_inst.types;

                    let generic_type_def = &mr.types[generic_class.type_index];
                    let TypeData::TypeDefinitionIndex(tdi) = generic_type_def.data else {
                        panic!()
                    };

                    if add_include {
                        requirements.add_dependency_tag(tdi.into());

                        let generic_tag = CppTypeTag::from_type_data(typ.data, metadata.metadata);

                        requirements.add_dependency_tag(generic_tag);
                    }

                    let generic_types_formatted = new_generic_inst_types
                        // let generic_types_formatted = new_generic_inst_types
                        .iter()
                        .map(|t| mr.types.get(*t).unwrap())
                        // if t is a Var, we use the generic inst provided by the caller
                        // TODO: This commented code breaks generic params where we intentionally use the template name
                        // .map(|inst_t| match inst_t.data {
                        //     TypeData::GenericParameterIndex(gen_param_idx) => {
                        //         let gen_param =
                        //             &metadata.metadata.global_metadata.generic_parameters
                        //                 [gen_param_idx];
                        //         declaring_generic_inst_types
                        //             .and_then(|declaring_generic_inst_types| {
                        //                 // TODO: Figure out why we this goes out of bounds
                        //                 declaring_generic_inst_types.get(gen_param.num as usize)
                        //             })
                        //             .map(|t| &mr.types[*t])
                        //             // fallback to T since generic typedefs can be called
                        //             .unwrap_or(inst_t)
                        //     }
                        //     _ => inst_t,
                        // })
                        .map(|t| {
                            cpp_type.cppify_name_il2cpp_recurse(
                                requirements,
                                ctx_collection,
                                metadata,
                                t,
                                next_include_depth,
                                // use declaring generic inst since we're cppifying generic args
                                declaring_generic_inst_types,
                            )
                        })
                        .collect_vec();

                    let generic_type_def = &mr.types[generic_class.type_index];
                    let type_def_name = cpp_type.cppify_name_il2cpp_recurse(
                        requirements,
                        ctx_collection,
                        metadata,
                        generic_type_def,
                        include_depth,
                        Some(new_generic_inst_types),
                    );

                    format!("{type_def_name}<{}>", generic_types_formatted.join(","))
                }

                _ => panic!("Unknown type data for generic inst {typ:?}!"),
            },
            Il2CppTypeEnum::I1 => "int8_t".to_string(),
            Il2CppTypeEnum::I2 => "int16_t".to_string(),
            Il2CppTypeEnum::I4 => "int32_t".to_string(),
            Il2CppTypeEnum::I8 => "int64_t".to_string(),
            Il2CppTypeEnum::I => "::cordl_internals::intptr_t".to_string(),
            Il2CppTypeEnum::U1 => "uint8_t".to_string(),
            Il2CppTypeEnum::U2 => "uint16_t".to_string(),
            Il2CppTypeEnum::U4 => "uint32_t".to_string(),
            Il2CppTypeEnum::U8 => "uint64_t".to_string(),
            Il2CppTypeEnum::U => "::cordl_internals::uintptr_t".to_string(),

            // https://learn.microsoft.com/en-us/nimbusml/concepts/types
            // https://en.cppreference.com/w/cpp/types/floating-point
            Il2CppTypeEnum::R4 => "float_t".to_string(),
            Il2CppTypeEnum::R8 => "double_t".to_string(),

            Il2CppTypeEnum::Void => "void".to_string(),
            Il2CppTypeEnum::Boolean => "bool".to_string(),
            Il2CppTypeEnum::Char => "char16_t".to_string(),
            Il2CppTypeEnum::String => {
                requirements.needs_stringw_include();
                "::StringW".to_string()
            }
            Il2CppTypeEnum::Ptr => {
                let generic: String = match typ.data {
                    TypeData::TypeIndex(e) => {
                        let ty = &metadata.metadata_registration.types[e];
                        cpp_type.cppify_name_il2cpp_recurse(
                            requirements,
                            ctx_collection,
                            metadata,
                            ty,
                            include_depth,
                            declaring_generic_inst_types,
                        )
                    }

                    _ => panic!("Unknown type data for array {typ:?}!"),
                };

                format!("::cordl_internals::Ptr<{generic}>")
            }
            // Il2CppTypeEnum::Typedbyref => {
            //     // TODO: test this
            //     if add_include && let TypeData::TypeDefinitionIndex(tdi) = typ.data {
            //         cpp_type.requirements.add_dependency_tag(tdi.into());
            //     }

            //     "::System::TypedReference".to_string()
            //     // "::cordl_internals::TypedByref".to_string()
            // },
            // TODO: Void and the other primitives
            _ => format!("/* UNKNOWN TYPE! {typ:?} */"),
        };

        ret
    }

    fn classof_cpp_name(&self) -> String {
        format!(
            "::il2cpp_utils::il2cpp_type_check::il2cpp_no_arg_class<::{}>::get",
            self.get_cpp_type().cpp_name_components.combine_all(true)
        )
    }

    fn get_type_definition<'a>(
        metadata: &'a Metadata,
        tdi: TypeDefinitionIndex,
    ) -> &'a Il2CppTypeDefinition {
        &metadata.metadata.global_metadata.type_definitions[tdi]
    }
}

fn wrapper_type_for_tdi(td: &Il2CppTypeDefinition) -> &str {
    if td.is_enum_type() {
        return ENUM_WRAPPER_TYPE;
    }

    if td.is_value_type() {
        return VALUE_WRAPPER_TYPE;
    }

    if td.is_interface() {
        return INTERFACE_WRAPPER_TYPE;
    }

    OBJECT_WRAPPER_TYPE
}

///
/// This makes generic args for types such as ValueTask<List<T>> work
/// by recursively checking if any generic arg is a reference or numeric type (for enums)
///
fn parse_generic_arg(
    t: &Il2CppType,
    gen_name: String,
    cpp_type: &mut CppType,
    ctx_collection: &CppContextCollection,
    metadata: &Metadata<'_>,
    template_args: &mut Vec<(String, String)>,
) -> String {
    // If reference type, we use a template and add a requirement
    if !t.valuetype {
        template_args.push((
            CORDL_REFERENCE_TYPE_CONSTRAINT.to_string(),
            gen_name.clone(),
        ));
        return gen_name;
    }

    let inner_type = cpp_type.cppify_name_il2cpp(ctx_collection, metadata, t, 0);

    /*
       mscorelib.xml
       <type fullname="System.SByteEnum" />
       <type fullname="System.Int16Enum" />
       <type fullname="System.Int32Enum" />
       <type fullname="System.Int64Enum" />

       <type fullname="System.ByteEnum" />
       <type fullname="System.UInt16Enum" />
       <type fullname="System.UInt32Enum" />
       <type fullname="System.UInt64Enum" />
    */
    let enum_system_type_discriminator = match t.data {
        TypeData::TypeDefinitionIndex(tdi) => {
            let td = &metadata.metadata.global_metadata.type_definitions[tdi];
            let namespace = td.namespace(metadata.metadata);
            let name = td.name(metadata.metadata);

            if namespace == "System" {
                match name {
                    "SByteEnum" => Some(Il2CppTypeEnum::I1),
                    "Int16Enum" => Some(Il2CppTypeEnum::I2),
                    "Int32Enum" => Some(Il2CppTypeEnum::I4),
                    "Int64Enum" => Some(Il2CppTypeEnum::I8),
                    "ByteEnum" => Some(Il2CppTypeEnum::U1),
                    "UInt16Enum" => Some(Il2CppTypeEnum::U2),
                    "UInt32Enum" => Some(Il2CppTypeEnum::U4),
                    "UInt64Enum" => Some(Il2CppTypeEnum::U8),
                    _ => None,
                }
            } else {
                None
            }
        }
        _ => None,
    };

    let inner_enum_type = enum_system_type_discriminator.map(|e| Il2CppType {
        attrs: u16::MAX,
        byref: false,
        data: TypeData::TypeIndex(usize::MAX),
        pinned: false,
        ty: e,
        valuetype: true,
    });

    // if int, int64 etc.
    // this allows for enums to be supported
    // if matches!(
    //     t.ty,
    //     Il2CppTypeEnum::I1
    //         | Il2CppTypeEnum::I2
    //         | Il2CppTypeEnum::I4
    //         | Il2CppTypeEnum::I8
    //         | Il2CppTypeEnum::U1
    //         | Il2CppTypeEnum::U2
    //         | Il2CppTypeEnum::U4
    //         | Il2CppTypeEnum::U8
    // ) ||
    if let Some(inner_enum_type) = inner_enum_type {
        let inner_enum_type_cpp =
            cpp_type.cppify_name_il2cpp(ctx_collection, metadata, &inner_enum_type, 0);

        template_args.push((
            format!("{CORDL_NUM_ENUM_TYPE_CONSTRAINT}<{inner_enum_type_cpp}>",),
            gen_name.clone(),
        ));

        return gen_name;
    }

    match t.data {
        TypeData::GenericClassIndex(gen_class_idx) => {
            let gen_class = &metadata.metadata_registration.generic_classes[gen_class_idx];
            let gen_class_ty = &metadata.metadata_registration.types[gen_class.type_index];
            let TypeData::TypeDefinitionIndex(gen_class_tdi) = gen_class_ty.data else {
                todo!()
            };
            let gen_class_td = &metadata.metadata.global_metadata.type_definitions[gen_class_tdi];

            let gen_container = gen_class_td.generic_container(metadata.metadata);

            let gen_class_inst = &metadata.metadata_registration.generic_insts
                [gen_class.context.class_inst_idx.unwrap()];

            // this relies on the fact TDIs do not include their generic params
            let non_generic_inner_type =
                cpp_type.cppify_name_il2cpp(ctx_collection, metadata, gen_class_ty, 0);

            let inner_generic_params = gen_class_inst
                .types
                .iter()
                .enumerate()
                .map(|(param_idx, u)| {
                    let t = metadata.metadata_registration.types.get(*u).unwrap();
                    let gen_param = gen_container
                        .generic_parameters(metadata.metadata)
                        .iter()
                        .find(|p| p.num as usize == param_idx)
                        .expect("No generic param at this num");

                    (t, gen_param)
                })
                .map(|(t, gen_param)| {
                    let inner_gen_name = gen_param.name(metadata.metadata).to_owned();
                    let mangled_gen_name =
                        format!("{inner_gen_name}_cordlgen_{}", template_args.len());
                    parse_generic_arg(
                        t,
                        mangled_gen_name,
                        cpp_type,
                        ctx_collection,
                        metadata,
                        template_args,
                    )
                })
                .join(", ");

            format!("{non_generic_inner_type}<{inner_generic_params}>")
        }
        _ => inner_type,
    }
}

impl CSType for CppType {
    #[inline(always)]
    fn get_mut_cpp_type(&mut self) -> &mut CppType {
        self
    }

    #[inline(always)]
    fn get_cpp_type(&self) -> &CppType {
        self
    }
}
