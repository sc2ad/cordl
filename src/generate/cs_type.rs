use core::panic;
use log::debug;
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
    generate::{members::CppUsingAlias, offsets},
    helpers::cursor::ReadBytesExtensions,
};

use super::{
    config::GenerationConfig,
    context_collection::{CppContextCollection, CppTypeTag},
    cpp_type::{
        CppType, CORDL_METHOD_HELPER_NAMESPACE, CORDL_NUM_ENUM_TYPE_CONSTRAINT,
        CORDL_REFERENCE_TYPE_CONSTRAINT, __CORDL_BACKING_ENUM_TYPE,
    },
    members::{
        CppCommentedString, CppConstructorDecl, CppConstructorImpl, CppFieldDecl, CppFieldImpl,
        CppForwardDeclare, CppInclude, CppLine, CppMember, CppMethodData, CppMethodDecl,
        CppMethodImpl, CppMethodSizeStruct, CppParam, CppPropertyDecl, CppTemplate,
    },
    metadata::Metadata,
    type_extensions::{
        Il2CppTypeEnumExtensions, MethodDefintionExtensions, ParameterDefinitionExtensions,
        TypeDefinitionExtensions, TypeExtentions, OBJECT_WRAPPER_TYPE,
    },
    writer::Writable,
};

type Endian = LittleEndian;

// negative
const VALUE_TYPE_SIZE_OFFSET: u32 = 0x10;

const VALUE_TYPE_WRAPPER_INSTANCE_NAME: &str = "__instance";
const VALUE_TYPE_WRAPPER_SIZE: &str = "__CORDL_VALUE_TYPE_SIZE";
const REFERENCE_TYPE_WRAPPER_SIZE: &str = "__CORDL_REFERENCE_TYPE_SIZE";
const REFERENCE_WRAPPER_INSTANCE_NAME: &str = "::bs_hook::Il2CppWrapperType::instance";

pub const VALUE_WRAPPER_TYPE: &str = "::bs_hook::ValueTypeWrapper";
pub const ENUM_WRAPPER_TYPE: &str = "::bs_hook::EnumTypeWrapper";
pub const INTERFACE_WRAPPER_TYPE: &str = "::cordl_internals::InterfaceW";

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

    fn add_generic_inst(&mut self, generic_il2cpp_inst: u32, metadata: &Metadata) -> &mut CppType {
        assert!(generic_il2cpp_inst != u32::MAX);

        let cpp_type = self.get_mut_cpp_type();

        let inst = metadata
            .metadata_registration
            .generic_insts
            .get(generic_il2cpp_inst as usize)
            .unwrap();

        if cpp_type.generic_instantiations_args_types.is_some() {
            panic!("Generic instantiation args are already set!");
        }

        cpp_type.generic_instantiations_args_types =
            Some(inst.types.iter().map(|t| *t as TypeIndex).collect());

        cpp_type.cpp_template = Some(CppTemplate { names: vec![] });
        cpp_type.is_stub = false;

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
    ) -> Option<CppType> {
        // let iface = metadata.interfaces.get(t.interfaces_start);
        // Then, handle interfaces

        // Then, handle methods
        // - This includes constructors
        // inherited methods will be inherited

        let t = &metadata.metadata.global_metadata.type_definitions[tdi];

        // Generics
        // This is a generic type def
        let generics = t.generic_container_index.is_valid().then(|| {
            t.generic_container(metadata.metadata)
                .generic_parameters(metadata.metadata)
                .iter()
                .map(|param| (param, param.constraints(metadata.metadata)))
                .collect_vec()
        });

        let cpp_template = generics.as_ref().map(|g| {
            CppTemplate::make_typenames(
                g.iter()
                    .map(|(g, _)| g.name(metadata.metadata).to_string())
                    .collect(),
            )
        });

        let ns = t.namespace(metadata.metadata);
        let name = t.name(metadata.metadata);
        let full_name = t.full_name(metadata.metadata, false);

        // all nested types are unnested
        let nested = false; // t.declaring_type_index != u32::MAX;
        let cpp_full_name = t.full_name_cpp(metadata.metadata, config, false);

        // TODO: Come up with a way to avoid this extra call to layout the entire type
        // We really just want to call it once for a given size and then move on
        // Every type should have a valid metadata size, even if it is 0
        let mut metadata_size = offsets::get_size_of_type_table(metadata, tdi)
            .unwrap()
            .instance_size;
        if metadata_size == 0 && !t.is_interface() {
            debug!(
                "Computing instance size by laying out type for tdi: {:?}",
                tdi
            );
            metadata_size = offsets::layout_fields_for_type(t, tdi, metadata, None)
                .size
                .try_into()
                .unwrap();
            // Remove implicit size of object from total size of instance
        }
        if t.is_value_type() {
            // For value types we need to ALWAYS subtract our object size
            metadata_size = metadata_size
                .checked_sub(metadata.object_size() as u32)
                .unwrap();
            debug!(
                "Resulting computed instance size (post subtractiong) for type {:?} is: {}",
                t.full_name(metadata.metadata, true),
                metadata_size
            );
            // If we are still 0, todo!
            if metadata_size == 0 {
                todo!("We do not yet support cases where the instance type would be a 0 AFTER we have done computation!");
            }
        }

        // Modified later for nested types
        let mut cpptype = CppType {
            self_tag: tag,
            nested,
            prefix_comments: vec![format!("Type: {ns}::{name}")],
            namespace: ns.to_string(),
            cpp_namespace: config.namespace_cpp(ns),
            name: name.to_string(),
            cpp_name: config.name_cpp(name),

            calculated_size: Some(metadata_size as usize),

            cpp_full_name,
            full_name,

            declarations: Default::default(),
            implementations: Default::default(),
            nonmember_implementations: Default::default(),
            nonmember_declarations: Default::default(),

            is_value_type: t.is_value_type(),
            is_enum_type: t.is_enum_type(),
            requirements: Default::default(),

            inherit: Default::default(),
            cpp_template,

            generic_instantiations_args_types: Default::default(),
            generic_instantiation_args: Default::default(),
            method_generic_instantiation_map: Default::default(),

            is_stub: false,
            is_hidden: true,
            nested_types: Default::default(),
        };

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

            cpptype.cpp_namespace = config.namespace_cpp(declaring_td.namespace(metadata.metadata));

            cpptype.cpp_name = config.generic_nested_name(&cpptype.cpp_full_name);

            // full name will have literals in `fill_generic_class_inst`
            cpptype.cpp_full_name = format!("{}::{}", cpptype.cpp_namespace, cpptype.cpp_name);
        }

        if t.parent_index == u32::MAX {
            if !t.is_interface() {
                println!("Skipping type: {ns}::{name} because it has parent index: {} and is not an interface!", t.parent_index);
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

        // we depend on parents and generic args here
        // default ctor
        if t.is_value_type() || t.is_enum_type() {
            self.create_valuetype_constructor(metadata, ctx_collection, config, tdi);
            self.create_valuetype_field_wrapper();
            self.create_valuetype_convert();
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
                let t = metadata
                    .metadata_registration
                    .types
                    .get(*u as usize)
                    .unwrap();

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

        cpp_type.generic_instantiation_args = Some(
            generic_instantiation_args
                .into_iter()
                .map(|gen_name| gen_name)
                .collect_vec(),
        );

        // only set if there are no generic ref types
        cpp_type.cpp_full_name = format!(
            "{}<{}>",
            cpp_type.cpp_full_name,
            cpp_type
                .generic_instantiation_args
                .as_ref()
                .unwrap()
                .join(",")
        )
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
                let _resulting_size =
                    offsets::layout_fields_for_type(t, tdi, metadata, Some(&mut offsets));
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
            let f_offset = {
                if f_type.is_static() {
                    0
                } else {
                    // If we have a hotfix offset, use that instead
                    // We can safely assume this always returns None even if we "next" past the end
                    let offset = if let Some(computed_offset) = offset_iter.next() {
                        *computed_offset
                    } else {
                        field_offsets[i]
                    };

                    if !t.is_value_type() && !f_type.is_static() && !f_type.is_constant() {
                        offset
                    } else {
                        // value type fixup
                        offset - metadata.object_size() as u32
                    }
                }
            };

            if let TypeData::TypeDefinitionIndex(tdi) = f_type.data && metadata.blacklisted_types.contains(&tdi) {
                if !cpp_type.is_value_type && !cpp_type.is_enum_type {
                    continue;
                }
                println!("Value type uses {tdi:?} which is blacklisted! TODO");
            }

            let field_ty_cpp_name = if f_type.is_constant() && f_type.ty == Il2CppTypeEnum::String {
                "::ConstString".to_string()
            } else {
                cpp_type.cppify_name_il2cpp(ctx_collection, metadata, f_type, false)
            };

            // TODO: Check a flag to look for default values to speed this up
            let def_value = Self::field_default_value(metadata, field_index);

            assert!(def_value.is_none() || (def_value.is_some() && f_type.is_param_optional()));

            // TODO: Static fields
            if f_type.is_constant() {
                let def_value = def_value.expect("Constant with no default value?");

                match f_type.ty.is_primitive_builtin() {
                    false => {
                        // other type
                        let field_decl = CppFieldDecl {
                            cpp_name: config.name_cpp(f_name),
                            field_ty: field_ty_cpp_name,
                            instance: false,
                            readonly: f_type.is_constant(),
                            value: None,
                            const_expr: false,
                            brief_comment: Some(format!("Field {f_name} offset {f_offset}")),
                        };
                        let field_impl = CppFieldImpl {
                            value: def_value,
                            const_expr: true,
                            declaring_type: cpp_type.cpp_full_name.clone(),
                            ..field_decl.clone().into()
                        };

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
                            cpp_name: config.name_cpp(f_name),
                            field_ty: field_ty_cpp_name,
                            instance: false,
                            readonly: f_type.is_constant(),
                            value: Some(def_value),
                            const_expr: true,
                            brief_comment: Some(format!("Field {f_name} offset {f_offset}")),
                        };

                        cpp_type
                            .declarations
                            .push(CppMember::FieldDecl(field_decl).into());
                    }
                }
            } else {
                let self_wrapper_instance = match t.is_value_type() || t.is_enum_type() {
                    true => VALUE_TYPE_WRAPPER_INSTANCE_NAME.to_string(),
                    false => REFERENCE_WRAPPER_INSTANCE_NAME.to_string(),
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
                            "return {CORDL_METHOD_HELPER_NAMESPACE}::getInstanceField<{field_ty_cpp_name}, 0x{f_offset:x}>(this->{self_wrapper_instance});"
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
                            "{CORDL_METHOD_HELPER_NAMESPACE}::setInstanceField<{field_ty_cpp_name}, 0x{f_offset:x}>(this->{self_wrapper_instance}, std::forward<{field_ty_cpp_name}>({setter_var_name}));"
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
                let getter_decl = CppMethodDecl {
                    cpp_name: format!("__get_{}", config.name_cpp(f_name)),
                    instance: is_instance,
                    return_type: field_ty_cpp_name.clone(),

                    brief: None,
                    body: None, // TODO:
                    // Const if instance for now
                    is_const: is_instance,
                    is_constexpr: !f_type.is_static() || f_type.is_constant(),
                    is_virtual: false,
                    is_operator: false,
                    is_no_except: false, // TODO:
                    parameters: vec![],
                    prefix_modifiers: vec![],
                    suffix_modifiers: vec![],
                    template: None,
                };

                let setter_decl = CppMethodDecl {
                    cpp_name: format!("__set_{}", config.name_cpp(f_name)),
                    instance: !f_type.is_static() && !f_type.is_constant(),
                    return_type: "void".to_string(),

                    brief: None,
                    body: None,      //TODO:
                    is_const: false, // TODO: readonly fields?
                    is_constexpr: !f_type.is_static() || f_type.is_constant(),
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
                    declaring_cpp_full_name: cpp_type.cpp_full_name.clone(),
                    template: useful_template.clone(),

                    ..getter_decl.clone().into()
                };

                let setter_impl = CppMethodImpl {
                    body: vec![Arc::new(CppLine::make(setter_call))],
                    declaring_cpp_full_name: cpp_type.cpp_full_name.clone(),
                    template: useful_template.clone(),

                    ..setter_decl.clone().into()
                };

                let field_decl = CppPropertyDecl {
                    cpp_name: config.name_cpp(f_name),
                    prop_ty: field_ty_cpp_name.clone(),
                    instance: !f_type.is_static() && !f_type.is_constant(),
                    getter: getter_decl.cpp_name.clone().into(),
                    setter: setter_decl.cpp_name.clone().into(),
                    brief_comment: Some(format!("Field {f_name} offset {f_offset}")),
                };

                cpp_type
                    .declarations
                    .push(CppMember::Property(field_decl).into());

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
                    println!("Skipping type: {ns}::{name} because it has parent index: {} and is not an interface!", t.parent_index);
                }
            }
        } else if let Some(parent_type) = metadata
            .metadata_registration
            .types
            .get(t.parent_index as usize)
        {
            let parent_ty: CppTypeTag =
                CppTypeTag::from_type_data(parent_type.data, metadata.metadata);

            // We have a parent, lets do something with it
            let inherit_type =
                cpp_type.cppify_name_il2cpp(ctx_collection, metadata, parent_type, true);

            if matches!(
                parent_type.ty,
                Il2CppTypeEnum::Valuetype | Il2CppTypeEnum::Class | Il2CppTypeEnum::Genericinst
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
                        panic!("No CppType for base type {inherit_type}. Using tag {parent_ty:?}")
                    });

                cpp_type.requirements.add_impl_include(
                    Some(base_type_cpp_type),
                    CppInclude::new_context_typeimpl(base_type_context),
                )
            }

            cpp_type.inherit.push(inherit_type);
        } else {
            panic!("NO PARENT! But valid index found: {}", t.parent_index);
        }

        for &interface_index in t.interfaces(metadata.metadata) {
            let int_ty = &metadata.metadata_registration.types[interface_index as usize];

            // We have an interface, lets do something with it
            let interface_cpp_name =
                cpp_type.cppify_name_il2cpp(ctx_collection, metadata, int_ty, false);

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
                parameters: vec![],
                template: None,
                prefix_modifiers: vec![],
                suffix_modifiers: vec![],
            };
            let method_impl = CppMethodImpl {
                body: vec![Arc::new(CppLine::make(format!(
                    "return {interface_cpp_name}({convert_line});"
                )))],
                declaring_cpp_full_name: cpp_type.cpp_full_name.clone(),
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
        config: &GenerationConfig,

        tdi: TypeDefinitionIndex,
    ) {
        let cpp_type = self.get_mut_cpp_type();
        let t = &metadata.metadata.global_metadata.type_definitions[tdi];

        if t.nested_type_count == 0 {
            return;
        }

        let generic_instantiation_args = cpp_type.generic_instantiation_args.clone();

        let aliases = t
            .nested_types(metadata.metadata)
            .iter()
            .map(|nested_tdi| {
                let nested_tag = CppTypeTag::TypeDefinitionIndex(*nested_tdi);

                let nested_context = ctx_collection
                    .get_context(nested_tag)
                    .expect("Unable to find CppContext");
                let nested = ctx_collection
                    .get_cpp_type(nested_tag)
                    .expect("Unable to find nested CppType");

                let alias = CppUsingAlias::from_cpp_type(
                    config.name_cpp(&nested.name),
                    nested,
                    generic_instantiation_args.clone(),
                    // if no generic args are made, we can do the generic fixup
                    // ORDER OF PASSES MATTERS
                    nested.generic_instantiation_args.is_none(),
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
        //         None => println!("Failed to make nested CppType {nt_ty:?}"),
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

            let p_type_index = match p_getter {
                Some(g) => g.return_type as usize,
                None => p_setter.unwrap().parameters(metadata.metadata)[0].type_index as usize,
            };

            let p_type = metadata
                .metadata_registration
                .types
                .get(p_type_index)
                .unwrap();

            let p_ty_cpp_name =
                cpp_type.cppify_name_il2cpp(ctx_collection, metadata, p_type, false);

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
                    instance: !p_getter.or(p_setter).unwrap().is_static_method(),
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

        let enum_base =
            cpp_type.cppify_name_il2cpp(ctx_collection, metadata, backing_field_ty, false);

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

        let enum_base =
            cpp_type.cppify_name_il2cpp(ctx_collection, metadata, backing_field_ty, false);

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

                wrapper_declaration.push(format!("__{f_name} = {value},"));
            }
        }

        wrapper_declaration.push("};".into());

        cpp_type
            .declarations
            .push(CppMember::CppLine(CppLine::make(wrapper_declaration.join("\n"))).into());

        let operator_body =
            format!("return std::bit_cast<{unwrapped_name}>({VALUE_TYPE_WRAPPER_INSTANCE_NAME});");
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
            CppMember::FieldDecl(CppFieldDecl {
                cpp_name: VALUE_TYPE_WRAPPER_INSTANCE_NAME.to_string(),
                field_ty: format!("std::array<std::byte, {VALUE_TYPE_WRAPPER_SIZE}>"),
                instance: true,
                readonly: false,
                const_expr: false,
                value: None,
                brief_comment: Some("Holds the value type data".to_string()),
            })
            .into(),
        );

        let mut field_initializer: HashMap<String, String> = HashMap::new();
        field_initializer.insert("__instance".into(), "std::move(instance)".into());

        cpp_type.declarations.push(
            CppMember::ConstructorDecl(CppConstructorDecl {
                cpp_name: cpp_type.cpp_name.clone(),
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
                base_ctor: Some((cpp_type.inherit.get(0).unwrap().to_string(), "".to_string())),
                initialized_values: field_initializer.clone(),
                brief: Some(
                    "Constructor that lets you initialize the internal array explicitly".into(),
                ),
                body: Some(vec![]),
            })
            .into(),
        );
    }

    fn create_valuetype_convert(&mut self) {
        let cpp_type = self.get_mut_cpp_type();
        cpp_type.declarations.push(
            CppMember::MethodDecl(CppMethodDecl {
                body: Some(vec![Arc::new(CppLine::make(format!("return const_cast<void*>(static_cast<const void*>({VALUE_TYPE_WRAPPER_INSTANCE_NAME}.data()));")))]),
                cpp_name: "convert".to_string(),
                return_type: "void*".to_string(),
                parameters: vec![],
                instance: true,
                template: None,
                suffix_modifiers: vec![],
                prefix_modifiers: vec![],
                is_virtual: false,
                is_constexpr: true,
                is_const: true,
                is_no_except: true,
                is_operator: false,
                brief: Some("conversion method for value type".into()),
            }).into())
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

                let f_type_cpp_name = {
                    // add include because it's required

                    cpp_type.cppify_name_il2cpp(ctx_collection, metadata, f_type, false)
                };

                // Get the inner type of a Generic Inst
                // e.g ReadOnlySpan<char> -> ReadOnlySpan<T>
                let def_value = Self::type_default_value(metadata, Some(cpp_type), f_type);

                Some(CppParam {
                    name: config.name_cpp(field.name(metadata.metadata)),
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

            let constructor_impl = CppConstructorImpl {
                body,
                template: cpp_type.cpp_template.clone(),
                parameters: instance_fields,
                declaring_full_name: cpp_type.formatted_complete_cpp_name().to_string(),
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

        cpp_type.declarations.push(
            CppMember::CppLine(CppLine {
                line: format!(
                    "
                    constexpr {cpp_name}({cpp_name} const&) = default;
                    constexpr {cpp_name}({cpp_name}&&) = default;
                    constexpr {cpp_name}& operator=({cpp_name} const& o) {{
                        __instance = o.__instance;
                        return *this;
                    }};
                    constexpr {cpp_name}& operator=({cpp_name}&& o) noexcept {{
                        __instance = std::move(o.__instance);
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

        // Skip if System.ValueType or System.Enum
        if cpp_type.namespace() == "System"
            && (cpp_type.cpp_name() == "ValueType" || cpp_type.cpp_name() == "Enum")
        {
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
        let cpp_name = cpp_type.cpp_name().clone();

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
        impl_template: &Option<CppTemplate>,
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

        let ty_full_cpp_name = cpp_type.formatted_complete_cpp_name();

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
            prefix_modifiers: vec![],
            suffix_modifiers: vec![],
        };

        // To avoid trailing ({},)
        let base_ctor_params = CppParam::params_names(&decl.parameters).join(", ");

        let allocate_call =
            format!("THROW_UNLESS(::il2cpp_utils::New<{ty_full_cpp_name}>({base_ctor_params}))");

        let cpp_constructor_impl = CppMethodImpl {
            body: vec![
                Arc::new(CppLine::make(format!(
                    "{ty_full_cpp_name} o{{{allocate_call}}};"
                ))),
                Arc::new(CppLine::make("return o;".into())),
            ],

            declaring_cpp_full_name: cpp_type.formatted_complete_cpp_name().to_string(),
            parameters: m_params.to_vec(),
            template: impl_template.clone(),
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
            // println!("Skipping {}", m_name);
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
            let must_include = false;
            let make_param_cpp_type_name = |cpp_type: &mut CppType| {
                cpp_type.cppify_name_il2cpp(ctx_collection, metadata, param_type, must_include)
            };

            let param_cpp_name = {
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
                        .map(|param| param.name(metadata.metadata).to_string())
                        .collect_vec();

                    Some(CppTemplate::make_typenames(generics))
                }
            }
        } else {
            None
        };

        let impl_template: Option<CppTemplate> = {
            let declaring_type_template = cpp_type.cpp_template.clone();

            if let Some(declaring_type_template) = &declaring_type_template && let Some(method_template) = &template {
                let concat_template = CppTemplate{
                    names: declaring_type_template.names.iter().chain(method_template.names.iter()).cloned().collect_vec()
                };

                Some(concat_template)
            } else {

                // if just one of the two templates is Some(), then we OR
                declaring_type_template.or(template.clone())
            }
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
                .map(|t| cpp_type.cppify_name_il2cpp(ctx_collection, metadata, t, false))
                .collect_vec()
        });

        // Lazy cppify
        let make_ret_cpp_type_name = |cpp_type: &mut CppType| {
            cpp_type.cppify_name_il2cpp(ctx_collection, metadata, m_ret_type, false)
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
            Self::create_ref_constructor(
                cpp_type,
                declaring_type,
                &m_params_with_def,
                &template,
                &impl_template,
            );
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
        };

        let instance_ptr: String = if method.is_static_method() {
            "nullptr".into()
        } else if cpp_type.is_value_type {
            format!("const_cast<void*>(reinterpret_cast<const void*>({VALUE_TYPE_WRAPPER_INSTANCE_NAME}.data()))")
        } else {
            format!("const_cast<void*>(this->{REFERENCE_WRAPPER_INSTANCE_NAME})")
        };

        const METHOD_INFO_VAR_NAME: &str = "___internal_method";

        let method_invoke_params = vec![instance_ptr.as_str(), METHOD_INFO_VAR_NAME];
        let param_names = CppParam::params_names(&method_decl.parameters).map(|s| s.as_str());
        let declaring_type_cpp_full_name = cpp_type.formatted_complete_cpp_name().to_string();
        let declaring_classof_call = format!("::il2cpp_utils::il2cpp_type_check::il2cpp_no_arg_class<{declaring_type_cpp_full_name}>::get()");

        let params_types_format: String = CppParam::params_types(&method_decl.parameters)
            .map(|t| format!("::il2cpp_utils::il2cpp_type_check::il2cpp_no_arg_type<{t}>::get()"))
            .join(", ");

        let method_info_lines = match &template {
            Some(template) => {
                // generic
                let generics_classes_format = template
                    .names
                    .iter()
                    .map(|(_, t)| {
                        format!(
                            "::il2cpp_utils::il2cpp_type_check::il2cpp_no_arg_class<{t}>::get()"
                        )
                    })
                    .join(", ");

                vec![
                    format!("static auto* ___internal_method_base = THROW_UNLESS((::il2cpp_utils::FindMethod(
                        {declaring_classof_call},
                        \"{m_name}\",
                        std::vector<Il2CppClass*>{{{generics_classes_format}}},
                        ::std::vector<const Il2CppType*>{{{params_types_format}}}
                    )));"),
                    format!("static auto* {METHOD_INFO_VAR_NAME} = THROW_UNLESS(::il2cpp_utils::MakeGenericMethod(
                        ___internal_method_base,
                         std::vector<Il2CppClass*>{{{generics_classes_format}}}
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
            template: impl_template,

            // defaults
            ..method_decl.clone().into()
        };

        let declaring_cpp_type: Option<&CppType> = if tag == cpp_type.self_tag {
            Some(cpp_type)
        } else {
            ctx_collection.get_cpp_type(tag)
        };

        // don't emit method size structs for generic methods

        // if type is a generic
        let has_template_args = cpp_type
            .cpp_template
            .as_ref()
            .is_some_and(|t| !t.names.is_empty());

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
                    template,
                    generic_literals: resolved_generic_types,
                    method_data: CppMethodData {
                        addrs: method_calc.addrs,
                        estimated_size: method_calc.estimated_size,
                    },
                    interface_clazz_of: declaring_cpp_type
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
        const allow_generic_method_stubs_impl: bool = true;
        // If a generic instantiation or not a template
        if !method_stub || allow_generic_method_stubs_impl {
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
            Il2CppTypeEnum::I1 => cursor.read_i8().unwrap().to_string(),
            Il2CppTypeEnum::I2 => cursor.read_i16::<Endian>().unwrap().to_string(),
            Il2CppTypeEnum::I4 => cursor.read_compressed_i32::<Endian>().unwrap().to_string(),
            // TODO: We assume 64 bit
            Il2CppTypeEnum::I | Il2CppTypeEnum::I8 => {
                cursor.read_i64::<Endian>().unwrap().to_string()
            }
            Il2CppTypeEnum::U1 => cursor.read_u8().unwrap().to_string() + UNSIGNED_SUFFIX,
            Il2CppTypeEnum::U2 => {
                cursor.read_u16::<Endian>().unwrap().to_string() + UNSIGNED_SUFFIX
            }
            Il2CppTypeEnum::U4 => {
                cursor.read_compressed_u32::<Endian>().unwrap().to_string() + UNSIGNED_SUFFIX
            }
            // TODO: We assume 64 bit
            Il2CppTypeEnum::U | Il2CppTypeEnum::U8 => {
                cursor.read_u64::<Endian>().unwrap().to_string() + UNSIGNED_SUFFIX
            }

            // https://learn.microsoft.com/en-us/nimbusml/concepts/types
            // https://en.cppreference.com/w/cpp/types/floating-point
            Il2CppTypeEnum::R4 => cursor.read_f32::<Endian>().unwrap().to_string(),
            Il2CppTypeEnum::R8 => cursor.read_f64::<Endian>().unwrap().to_string(),
            Il2CppTypeEnum::Char => {
                let res = String::from_utf16_lossy(&[cursor.read_u16::<Endian>().unwrap()])
                    .escape_default()
                    .to_string();

                if string_quotes {
                    let literal_prefix = if string_as_u16 { "u" } else { "" };
                    return format!("{literal_prefix}\"{res}\"");
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
                                .map(|t| &metadata.metadata_registration.types[*t as usize])
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
        add_include: bool,
    ) -> String {
        let typ_tag = typ.data;

        let cpp_type = self.get_mut_cpp_type();

        let requirements = &mut cpp_type.requirements;
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
            Il2CppTypeEnum::Valuetype | Il2CppTypeEnum::Class => {
                let typ_cpp_tag: CppTypeTag = typ_tag.into();
                // Self
                if typ_cpp_tag == cpp_type.self_tag {
                    return cpp_type.formatted_complete_cpp_name().clone();
                }

                if let TypeData::TypeDefinitionIndex(tdi) = typ.data {
                    let td = &metadata.metadata.global_metadata.type_definitions[tdi];

                    if td.namespace(metadata.metadata) == "System"
                        && td.name(metadata.metadata) == "ValueType"
                    {
                        return VALUE_WRAPPER_TYPE.to_string();
                    }
                    if td.namespace(metadata.metadata) == "System"
                        && td.name(metadata.metadata) == "Enum"
                    {
                        return ENUM_WRAPPER_TYPE.to_string();
                    }
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
                        eprintln!("Can't forward declare nested type! Including!");
                        requirements.add_include(Some(to_incl_cpp_ty), inc);
                    } else {
                        requirements.add_forward_declare((
                            CppForwardDeclare::from_cpp_type(to_incl_cpp_ty),
                            inc,
                        ));
                    }
                }

                to_incl_cpp_ty.formatted_complete_cpp_name().clone()
            }
            // Single dimension array
            Il2CppTypeEnum::Szarray => {
                requirements.needs_arrayw_include();

                let generic: String = match typ.data {
                    TypeData::TypeIndex(e) => {
                        let ty = &metadata.metadata_registration.types[e];
                        self.cppify_name_il2cpp(ctx_collection, metadata, ty, add_include)
                    }

                    _ => panic!("Unknown type data for array {typ:?}!"),
                };

                format!("::ArrayW<{generic}>")
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

                    self.cppify_name_il2cpp(ctx_collection, metadata, ty, add_include)
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

                    // true if the type is intentionally a generic template type and not a specialization
                    let has_generic_template = cpp_type
                        .cpp_template
                        .as_ref()
                        .is_some_and(|template| !template.names.is_empty());

                    // if template arg is not found
                    if ty_idx_opt.is_none() {
                        let gen_name = generic_param.name(metadata.metadata);

                        return match has_generic_template {
                            true => gen_name.to_string(),
                            false => format!("/* TODO: FIX THIS, THIS SHOULDN'T HAPPEN! NO GENERIC INST ARGS FOUND HERE */ {gen_name}"),
                        };
                    }

                    cpp_type
                        .generic_instantiation_args
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

                    let generic_type_def = &mr.types[generic_class.type_index];
                    let TypeData::TypeDefinitionIndex(tdi) = generic_type_def.data else {
                        panic!()
                    };

                    let _type_def_name = cpp_type.cppify_name_il2cpp(
                        ctx_collection,
                        metadata,
                        generic_type_def,
                        add_include,
                    );

                    if add_include {
                        cpp_type.requirements.add_dependency_tag(tdi.into());

                        let generic_tag = CppTypeTag::from_type_data(typ.data, metadata.metadata);

                        cpp_type.requirements.add_dependency_tag(generic_tag);
                    }

                    let generic_types = generic_inst
                        .types
                        .iter()
                        .map(|t| mr.types.get(*t).unwrap())
                        .map(|t| self.cppify_name_il2cpp(ctx_collection, metadata, t, add_include));

                    let generic_types_formatted = generic_types.collect_vec();

                    let generic_type_def = &mr.types[generic_class.type_index];
                    let type_def_name = self.cppify_name_il2cpp(
                        ctx_collection,
                        metadata,
                        generic_type_def,
                        add_include,
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
            Il2CppTypeEnum::Ptr => "void*".to_owned(),
            // TODO: Void and the other primitives
            _ => format!("/* UNKNOWN TYPE! {typ:?} */"),
        };

        ret
    }

    fn classof_cpp_name(&self) -> String {
        format!(
            "::il2cpp_utils::il2cpp_type_check::il2cpp_no_arg_class<{}>::get",
            self.get_cpp_type().formatted_complete_cpp_name()
        )
    }

    fn get_type_definition<'a>(
        metadata: &'a Metadata,
        tdi: TypeDefinitionIndex,
    ) -> &'a Il2CppTypeDefinition {
        &metadata.metadata.global_metadata.type_definitions[tdi]
    }
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

    let inner_type = cpp_type.cppify_name_il2cpp(ctx_collection, metadata, t, false);

    // if int, int64 etc.
    // this allows for enums to be supported
    if matches!(
        t.ty,
        Il2CppTypeEnum::I1
            | Il2CppTypeEnum::I2
            | Il2CppTypeEnum::I4
            | Il2CppTypeEnum::I8
            | Il2CppTypeEnum::U1
            | Il2CppTypeEnum::U2
            | Il2CppTypeEnum::U4
            | Il2CppTypeEnum::U8
    ) {
        template_args.push((
            format!("{CORDL_NUM_ENUM_TYPE_CONSTRAINT}<{inner_type}>",),
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
                cpp_type.cppify_name_il2cpp(ctx_collection, metadata, gen_class_ty, false);

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
