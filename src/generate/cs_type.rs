use core::panic;
use log::{debug, info, warn};
use std::{
    collections::HashMap,
    io::{Cursor, Read},
    rc::Rc,
    slice::Iter,
    sync::Arc,
};

use brocolib::{
    global_metadata::{
        FieldIndex, Il2CppFieldDefinition, Il2CppTypeDefinition, MethodIndex, ParameterIndex,
        TypeDefinitionIndex, TypeIndex,
    },
    runtime_metadata::{Il2CppMethodSpec, Il2CppType, Il2CppTypeEnum, TypeData},
};
use byteorder::{LittleEndian, ReadBytesExt};

use itertools::Itertools;

use crate::{
    data::name_components::NameComponents,
    generate::{
        members::{CppNestedUnion, CppUsingAlias},
        offsets,
    },
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
        CppConstructorDecl, CppConstructorImpl, CppFieldDecl, CppFieldImpl,
        CppForwardDeclare, CppInclude, CppLine, CppMember, CppMethodData, CppMethodDecl,
        CppMethodImpl, CppMethodSizeStruct, CppNestedStruct, CppNonMember, CppParam,
        CppPropertyDecl, CppStaticAssert, CppTemplate,
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
pub const VALUE_TYPE_SIZE_OFFSET: u32 = 0x10;

pub const VALUE_TYPE_WRAPPER_SIZE: &str = "__IL2CPP_VALUE_TYPE_SIZE";
pub const REFERENCE_TYPE_WRAPPER_SIZE: &str = "__IL2CPP_REFERENCE_TYPE_SIZE";
pub const REFERENCE_TYPE_FIELD_SIZE: &str = "__fields";
pub const REFERENCE_WRAPPER_INSTANCE_NAME: &str = "::bs_hook::Il2CppWrapperType::instance";

pub const VALUE_WRAPPER_TYPE: &str = "::bs_hook::ValueType";
pub const ENUM_WRAPPER_TYPE: &str = "::bs_hook::EnumType";
pub const INTERFACE_WRAPPER_TYPE: &str = "::cordl_internals::InterfaceW";
pub const IL2CPP_OBJECT_TYPE: &str = "Il2CppObject";
pub const CORDL_NO_INCLUDE_IMPL_DEFINE: &str = "CORDL_NO_IMPL_INCLUDE";
pub const CORDL_ACCESSOR_FIELD_PREFIX: &str = "___";

pub const ENUM_PTR_TYPE: &str = "::bs_hook::EnumPtr";
pub const VT_PTR_TYPE: &str = "::bs_hook::VTPtr";

const SIZEOF_IL2CPP_OBJECT: u32 = 0x10;

#[derive(Clone, Debug)]
pub struct FieldInfo<'a> {
    cpp_field: CppFieldDecl,
    field: &'a Il2CppFieldDefinition,
    field_type: &'a Il2CppType,
    is_constant: bool,
    is_static: bool,
    is_pointer: bool,

    offset: Option<u32>,
    size: usize,
}

struct FieldInfoSet<'a> {
    fields: Vec<Vec<FieldInfo<'a>>>,
    size: u32,
    offset: u32,
}

impl<'a> FieldInfoSet<'a> {
    fn max(&self) -> u32 {
        self.size + self.offset
    }
}

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

        let is_pointer = cs_name_components.is_pointer;

        let cpp_name_components = NameComponents {
            declaring_types: cs_name_components
                .declaring_types
                .as_ref()
                .map(|declaring_types| {
                    declaring_types
                        .iter()
                        .map(|s| config.name_cpp(s))
                        .collect_vec()
                }),
            generics: cs_name_components.generics.clone(),
            name: config.name_cpp(&cs_name_components.name),
            namespace: cs_name_components
                .namespace
                .as_ref()
                .map(|s| config.namespace_cpp(s)),
            is_pointer,
        };

        // TODO: Come up with a way to avoid this extra call to layout the entire type
        // We really just want to call it once for a given size and then move on
        // Every type should have a valid metadata size, even if it is 0
        let size_info: offsets::SizeInfo =
            offsets::get_size_info(t, tdi, generic_inst_types, metadata);

        // best results of cordl are when specified packing is strictly what is used, but experimentation may be required
        let packing = size_info.specified_packing;

        // Modified later for nested types
        let mut cpptype = CppType {
            self_tag: tag,
            nested,
            prefix_comments: vec![format!("Type: {ns}::{name}"), format!("{size_info:?}")],

            size_info: Some(size_info),
            packing,

            cpp_name_components,
            cs_name_components,

            declarations: Default::default(),
            implementations: Default::default(),
            nonmember_implementations: Default::default(),
            nonmember_declarations: Default::default(),

            is_value_type: t.is_value_type(),
            is_enum_type: t.is_enum_type(),
            is_reference_type: is_pointer,
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
            let combined_name = cpptype
                .cpp_name_components
                .clone()
                .remove_generics()
                .remove_pointer()
                .combine_all();

            cpptype.cpp_name_components.namespace =
                Some(config.namespace_cpp(declaring_td.namespace(metadata.metadata)));
            cpptype.cpp_name_components.declaring_types = None; // remove declaring types

            cpptype.cpp_name_components.name = config.generic_nested_name(&combined_name);
        }

        if t.parent_index == u32::MAX {
            if !t.is_interface() && t.full_name(metadata.metadata, true) != "System.Object" {
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
            self.add_default_ctor(false);
        } else if t.is_interface() {
            // self.make_interface_constructors();
            self.delete_move_ctor();
            self.delete_copy_ctor();
            // self.delete_default_ctor();
        } else {
            // ref type
            self.delete_move_ctor();
            self.delete_copy_ctor();
            self.add_default_ctor(true);
            // self.delete_default_ctor();
        }

        if !t.is_interface() {
            self.create_size_assert();
        }

        self.make_nested_types(metadata, ctx_collection, config, tdi);
        self.make_fields(metadata, ctx_collection, config, tdi);
        self.make_properties(metadata, ctx_collection, config, tdi);
        self.make_methods(metadata, config, ctx_collection, tdi);

        if !t.is_interface() {
            self.create_size_padding(metadata, tdi);
        }

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
            .map(|n| n.combine_all())
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

        // if no fields, skip
        if t.field_count == 0 {
            return;
        }

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
                    false,
                );
            }
        }
        let mut offset_iter = offsets.iter();

        let get_offset = |field: &Il2CppFieldDefinition, i: usize, iter: &mut Iter<u32>| {
            let f_type = metadata
                .metadata_registration
                .types
                .get(field.type_index as usize)
                .unwrap();
            let f_name = field.name(metadata.metadata);

            match f_type.is_static() || f_type.is_constant() {
                // return u32::MAX for static fields as an "invalid offset" value
                true => None,
                false => Some({
                    // If we have a hotfix offset, use that instead
                    // We can safely assume this always returns None even if we "next" past the end
                    let offset = if let Some(computed_offset) = iter.next() {
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
                }),
            }
        };

        let get_size = |field: &Il2CppFieldDefinition, gen_args: Option<&Vec<usize>>| {
            let f_type = metadata
                .metadata_registration
                .types
                .get(field.type_index as usize)
                .unwrap();

            let sa = offsets::get_il2cpptype_sa(metadata, f_type, gen_args);

            sa.size

            // // TODO: is valuetype even a good way of checking whether we should be size of pointer?
            // match f_type.valuetype {
            //     false => metadata.pointer_size as u32,
            //     true => {
            //         match f_type.data {
            //             TypeData::TypeDefinitionIndex(f_tdi) => {
            //                 let f_td = Self::get_type_definition(metadata, f_tdi);

            //             },
            //             TypeData::GenericClassIndex(class_index) => {

            //             }
            //         }
            //         let f_tag = CppTypeTag::from_type_data(f_type.data, metadata.metadata);
            //         let f_tdi = f_tag.get_tdi();
            //         let f_td = Self::get_type_definition(metadata, f_tdi);

            //         if let Some(sz) = offsets::get_size_of_type_table(metadata, f_tdi) {
            //             if sz.instance_size == 0 {
            //                 // At this point we need to compute the offsets
            //                 debug!(
            //                     "Computing offsets for TDI: {:?}, as it has a size of 0",
            //                     tdi
            //                 );
            //                 let _resulting_size = offsets::layout_fields(
            //                     metadata,
            //                     f_td,
            //                     f_tdi,
            //                     gen_args.as_ref(),
            //                     None,
            //                 );

            //                 // TODO: check for VT fixup?
            //                 if f_td.is_value_type() {
            //                     _resulting_size.size as u32 - metadata.object_size() as u32
            //                 } else {
            //                     _resulting_size.size as u32
            //                 }
            //             } else {
            //                 sz.instance_size - metadata.object_size() as u32
            //             }
            //         } else {
            //             0
            //         }
            //     }
            // }
        };

        let fields = t
            .fields(metadata.metadata)
            .iter()
            .enumerate()
            .filter_map(|(i, field)| {
                let f_type = metadata
                    .metadata_registration
                    .types
                    .get(field.type_index as usize)
                    .unwrap();

                let field_index = FieldIndex::new(t.field_start.index() + i as u32);
                let f_name = field.name(metadata.metadata);

                let f_cpp_name = config.name_cpp_plus(f_name, &[cpp_type.cpp_name().as_str()]);

                let f_offset = get_offset(field, i, &mut offset_iter);

                // calculate / fetch the field size
                let f_size = if let Some(generics) = &cpp_type.generic_instantiations_args_types {
                    get_size(field, Some(generics))
                } else {
                    get_size(field, None)
                };

                if let TypeData::TypeDefinitionIndex(field_tdi) = f_type.data
                    && metadata.blacklisted_types.contains(&field_tdi)
                {
                    if !cpp_type.is_value_type && !cpp_type.is_enum_type {
                        return None;
                    }
                    warn!("Value type uses {tdi:?} which is blacklisted! TODO");
                }

                // Var types are default pointers so we need to get the name component's pointer bool
                let (field_ty_cpp_name, field_is_pointer) =
                    if f_type.is_constant() && f_type.ty == Il2CppTypeEnum::String {
                        ("::ConstString".to_string(), false)
                    } else {
                        let include_depth = match f_type.valuetype {
                            true => usize::MAX,
                            false => 0,
                        };

                        let field_name_components = cpp_type.cppify_name_il2cpp(
                            ctx_collection,
                            metadata,
                            f_type,
                            include_depth,
                        );

                        (
                            field_name_components.combine_all(),
                            field_name_components.is_pointer,
                        )
                    };

                // TODO: Check a flag to look for default values to speed this up
                let def_value = Self::field_default_value(metadata, field_index);

                assert!(def_value.is_none() || (def_value.is_some() && f_type.is_param_optional()));

                let cpp_field_decl = CppFieldDecl {
                    cpp_name: f_cpp_name,
                    field_ty: field_ty_cpp_name,
                    offset: f_offset.unwrap_or(u32::MAX),
                    instance: !f_type.is_static() && !f_type.is_constant(),
                    readonly: f_type.is_constant(),
                    brief_comment: Some(format!("Field {f_name}, offset: 0x{:x}, size: 0x{f_size:x}, def value: {def_value:?}", f_offset.unwrap_or(u32::MAX))),
                    value: def_value,
                    const_expr: false,
                    is_private: false,
                };

                Some(FieldInfo {
                    cpp_field: cpp_field_decl,
                    field,
                    field_type: f_type,
                    is_constant: f_type.is_constant(),
                    is_static: f_type.is_static(),
                    is_pointer: field_is_pointer,
                    offset: f_offset,
                    size: f_size,
                })
            })
            .collect_vec();

        for field_info in fields.iter() {
            let f_type = field_info.field_type;

            // only push def dependency if valuetype field & not a primitive builtin
            if f_type.valuetype && !f_type.ty.is_primitive_builtin() {
                let field_cpp_tag: CppTypeTag =
                    CppTypeTag::from_type_data(f_type.data, metadata.metadata);
                let field_cpp_td_tag: CppTypeTag = field_cpp_tag.get_tdi().into();
                let field_cpp_type = ctx_collection.get_cpp_type(field_cpp_td_tag);

                if field_cpp_type.is_some() {
                    let field_cpp_context = ctx_collection
                        .get_context(field_cpp_td_tag)
                        .expect("No context for cpp value type");

                    cpp_type.requirements.add_def_include(
                        field_cpp_type,
                        CppInclude::new_context_typedef(field_cpp_context),
                    );

                    cpp_type.requirements.add_impl_include(
                        field_cpp_type,
                        CppInclude::new_context_typeimpl(field_cpp_context),
                    );
                }
            }
        }

        if t.is_value_type() || t.is_enum_type() {
            cpp_type.handle_valuetype_fields(&fields, ctx_collection, metadata, tdi);
        } else {
            cpp_type.handle_referencetype_fields(&fields, ctx_collection, metadata, tdi);
        }

        cpp_type.handle_static_fields(&fields, metadata, tdi);
        cpp_type.handle_const_fields(&fields, ctx_collection, metadata, tdi);
    }

    fn handle_static_fields(
        &mut self,
        fields: &[FieldInfo],
        metadata: &Metadata,
        tdi: TypeDefinitionIndex,
    ) {
        let cpp_type = self.get_mut_cpp_type();
        let t = Self::get_type_definition(metadata, tdi);

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
                declaring_cpp_full_name: cpp_type
                    .cpp_name_components
                    .remove_pointer()
                    .combine_all(),
                template: useful_template.clone(),

                ..getter_decl.clone().into()
            };

            let setter_impl = CppMethodImpl {
                body: vec![Arc::new(CppLine::make(setter_call))],
                declaring_cpp_full_name: cpp_type
                    .cpp_name_components
                    .remove_pointer()
                    .combine_all(),
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
    fn handle_const_fields(
        &mut self,
        fields: &[FieldInfo],
        ctx_collection: &CppContextCollection,
        metadata: &Metadata,
        tdi: TypeDefinitionIndex,
    ) {
        let cpp_type = self.get_mut_cpp_type();
        let t = Self::get_type_definition(metadata, tdi);

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

    fn handle_instance_fields(
        &mut self,
        fields: &[FieldInfo],
        ctx_collection: &CppContextCollection,
        metadata: &Metadata,
        tdi: TypeDefinitionIndex,
    ) {
        let cpp_type = self.get_mut_cpp_type();
        let t = Self::get_type_definition(metadata, tdi);

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
            let u = Self::pack_fields_into_single_union(resulting_fields);
            cpp_type.declarations.push(CppMember::NestedUnion(u).into());
        } else {
            resulting_fields
                .into_iter()
                .map(|member| CppMember::FieldDecl(member.cpp_field))
                .for_each(|member| cpp_type.declarations.push(member.into()));
        };
    }

    fn fixup_backing_field(
        fieldname: &str
    ) -> String {
        format!("{CORDL_ACCESSOR_FIELD_PREFIX}{fieldname}")
    }

    fn handle_valuetype_fields(
        &mut self,
        fields: &[FieldInfo],
        ctx_collection: &CppContextCollection,
        metadata: &Metadata,
        tdi: TypeDefinitionIndex,
    ) {
        // Value types only need getter fixes for explicit layout types
        let cpp_type = self.get_mut_cpp_type();
        let t = Self::get_type_definition(metadata, tdi);

        // if no fields, skip
        if t.field_count == 0 {
            return;
        }

        // instance fields for explicit layout value types are special
        if t.is_explicit_layout() {
            for field_info in fields.iter().filter(|f| !f.is_constant && !f.is_static) {
                // don't get a template that has no names
                let template =
                    cpp_type
                        .cpp_template
                        .clone()
                        .and_then(|t| match t.names.is_empty() {
                            true => None,
                            false => Some(t),
                        });


                let declaring_cpp_full_name = cpp_type
                    .cpp_name_components
                    .remove_pointer()
                    .combine_all();

                let prop = Self::prop_decl_from_fieldinfo(metadata, field_info);
                let (accessor_decls, accessor_impls) = Self::prop_methods_from_fieldinfo(field_info, template, declaring_cpp_full_name, false);

                cpp_type.declarations.push(CppMember::Property(prop).into());

                accessor_decls
                    .into_iter()
                    .for_each(|method| {
                        cpp_type.declarations.push(CppMember::MethodDecl(method).into());
                    });

                accessor_impls
                    .into_iter()
                    .for_each(|method| {
                        cpp_type.implementations.push(CppMember::MethodImpl(method).into());
                    });
            }

            let backing_fields = fields
                .iter()
                .cloned()
                .map(|mut f| {
                    f.cpp_field.cpp_name = Self::fixup_backing_field(&f.cpp_field.cpp_name);
                    f
                })
                .collect_vec();

            cpp_type.handle_instance_fields(&backing_fields, ctx_collection, metadata, tdi);
        } else {
            cpp_type.handle_instance_fields(fields, ctx_collection, metadata, tdi);
        }
    }

    // create prop and field declaration from passed field info
    fn prop_decl_from_fieldinfo (
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

    fn prop_methods_from_fieldinfo(
        field_info: &FieldInfo,
        template: Option<CppTemplate>,
        declaring_cpp_name: String,
        declaring_is_ref: bool
    ) -> (Vec<CppMethodDecl>, Vec<CppMethodImpl>) {

        let f_type = field_info.field_type;
        let field_ty_cpp_name = &field_info.cpp_field.field_ty;

        let f_cpp_name = &field_info.cpp_field.cpp_name;
        let cordl_field_name = Self::fixup_backing_field(f_cpp_name);
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
            false => None
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
        let getter_body: Vec<Arc<dyn Writable>> = if let Some(instance_null_check) = instance_null_check {
            vec![
                Arc::new(CppLine::make(instance_null_check.into())),
                Arc::new(CppLine::make(getter_call)),
            ]
        } else {
            vec![Arc::new(CppLine::make(getter_call))]
        };

        let setter_body: Vec<Arc<dyn Writable>> = if let Some(instance_null_check) = instance_null_check {
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

        (vec![
            getter_decl,
            const_getter_decl,
            setter_decl,
        ], vec![
            getter_impl,
            const_getter_impl,
            setter_impl,
        ])
    }

    fn handle_referencetype_fields(
        &mut self,
        fields: &[FieldInfo],
        ctx_collection: &CppContextCollection,
        metadata: &Metadata,
        tdi: TypeDefinitionIndex,
    ) {
        let cpp_type = self.get_mut_cpp_type();
        let t = Self::get_type_definition(metadata, tdi);

        if t.is_explicit_layout() {
            warn!("Reference type with explicit layout: {}", cpp_type.cpp_name_components.combine_all());
        }

        // if no fields, skip
        if t.field_count == 0 {
            return;
        }

        for field_info in fields.iter().filter(|f| !f.is_constant && !f.is_static) {
            // don't get a template that has no names
            let template =
                cpp_type
                    .cpp_template
                    .clone()
                    .and_then(|t| match t.names.is_empty() {
                        true => None,
                        false => Some(t),
                    });


            let declaring_cpp_full_name = cpp_type
                .cpp_name_components
                .remove_pointer()
                .combine_all();

            let prop = Self::prop_decl_from_fieldinfo(metadata, field_info);
            let (accessor_decls, accessor_impls) = Self::prop_methods_from_fieldinfo(field_info, template, declaring_cpp_full_name, true);

            cpp_type.declarations.push(CppMember::Property(prop).into());

            accessor_decls
                .into_iter()
                .for_each(|method| {
                    cpp_type.declarations.push(CppMember::MethodDecl(method).into());
                });

            accessor_impls
                .into_iter()
                .for_each(|method| {
                    cpp_type.implementations.push(CppMember::MethodImpl(method).into());
                });
        }

        let backing_fields = fields
            .iter()
            .cloned()
            .map(|mut f| {
                f.cpp_field.cpp_name = Self::fixup_backing_field(&f.cpp_field.cpp_name);
                f
            })
            .collect_vec();

        cpp_type.handle_instance_fields(&backing_fields, ctx_collection, metadata, tdi);
    }

    fn field_collision_check(instance_fields: &[FieldInfo]) -> bool {
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
    fn pack_fields_into_single_union(fields: Vec<FieldInfo>) -> CppNestedUnion {
        // get the min offset to use as a base for the packed structs
        let min_offset = fields.iter().map(|f| f.offset.unwrap()).min().unwrap_or(0);

        let packed_structs = fields
            .into_iter()
            .map(|field| {
                let structs = Self::field_into_offset_structs(min_offset, field);

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

    fn field_into_offset_structs(
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

        let packed_padding_cpp_name =
            format!("{}_padding[0x{padding:x}]", field.cpp_field.cpp_name);
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
    fn make_or_unionize_fields(instance_fields: &[FieldInfo]) -> Vec<CppMember> {
        // make all fields like usual
        if !Self::field_collision_check(instance_fields) {
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

                let current_set =
                    offset_map
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
                    // FIXME: should interfaces have a base type? I don't think they need to
                    // cpp_type.inherit.push(INTERFACE_WRAPPER_TYPE.to_string());
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
                // let Some(size_info) = &cpp_type.size_info else {
                //     panic!("No size for value/enum type!")
                // };

                // if t.is_enum_type() {
                //     cpp_type.requirements.needs_enum_include();
                // } else if t.is_value_type() {
                //     cpp_type.requirements.needs_value_include();
                // }

                // let wrapper = wrapper_type_for_tdi(t);

                // cpp_type.inherit.push(wrapper.to_string());
            }
            // handle as reference type
            false => {
                // make sure our parent is intended\
                let is_ref_type = matches!(
                    parent_type.ty,
                    Il2CppTypeEnum::Class | Il2CppTypeEnum::Genericinst | Il2CppTypeEnum::Object
                );
                assert!(is_ref_type, "Not a class, object or generic inst!");

                // We have a parent, lets do something with it
                let inherit_type =
                    cpp_type.cppify_name_il2cpp(ctx_collection, metadata, parent_type, usize::MAX);

                if is_ref_type {
                    // TODO: Figure out why some generic insts don't work here
                    let parent_tdi: TypeDefinitionIndex = parent_ty.into();

                    let base_type_context = ctx_collection
                                    .get_context(parent_ty)
                                    .or_else(|| ctx_collection.get_context(parent_tdi.into()))
                                    .unwrap_or_else(|| {
                                        panic!(
                                        "No CppContext for base type {inherit_type:?}. Using tag {parent_ty:?}"
                                    )
                                    });

                    let base_type_cpp_type = ctx_collection
                                        .get_cpp_type(parent_ty)
                                        .or_else(|| ctx_collection.get_cpp_type(parent_tdi.into()))
                                        .unwrap_or_else(|| {
                                panic!(
                                    "No CppType for base type {inherit_type:?}. Using tag {parent_ty:?}"
                                )
                            });

                    cpp_type.requirements.add_impl_include(
                        Some(base_type_cpp_type),
                        CppInclude::new_context_typeimpl(base_type_context),
                    )
                }

                cpp_type
                    .inherit
                    .push(inherit_type.remove_pointer().combine_all());
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
            let interface_cpp_name = cpp_type
                .cppify_name_il2cpp(ctx_collection, metadata, int_ty, 0)
                .remove_pointer()
                .combine_all();
            let interface_cpp_pointer = cpp_type
                .cppify_name_il2cpp(ctx_collection, metadata, int_ty, 0)
                .as_pointer()
                .combine_all();

            let method_decl = CppMethodDecl {
                body: Default::default(),
                brief: Some(format!("Convert operator to {interface_cpp_name:?}")),
                cpp_name: interface_cpp_pointer.clone(),
                return_type: "".to_string(),
                instance: true,
                is_const: false,
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

            let convert_line = match t.is_value_type() || t.is_enum_type() {
                true => {
                    // box
                    "static_cast<void*>(::cordl_internals::Box(this))".to_string()
                }
                false => "static_cast<void*>(this)".to_string(),
            };

            let method_impl = CppMethodImpl {
                body: vec![Arc::new(CppLine::make(format!(
                    "return static_cast<{interface_cpp_pointer}>({convert_line});"
                )))],
                declaring_cpp_full_name: cpp_type
                    .cpp_name_components
                    .remove_pointer()
                    .combine_all(),
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
        config: &GenerationConfig,
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
                let nested_td = &metadata.metadata.global_metadata.type_definitions[*nested_tdi];
                let nested_tag = CppTypeTag::TypeDefinitionIndex(*nested_tdi);

                let nested_context = ctx_collection
                    .get_context(nested_tag)
                    .expect("Unable to find CppContext");
                let nested = ctx_collection
                    .get_cpp_type(nested_tag)
                    .expect("Unable to find nested CppType");

                let alias = CppUsingAlias::from_cpp_type(
                    config.name_cpp(nested_td.name(metadata.metadata)),
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

            let p_ty_cpp_name = cpp_type
                .cppify_name_il2cpp(ctx_collection, metadata, p_type, 0)
                .combine_all();

            let _method_map = |p: MethodIndex| {
                let method_calc = metadata.method_calculations.get(&p).unwrap();
                CppMethodData {
                    estimated_size: method_calc.estimated_size,
                    addrs: method_calc.addrs,
                }
            };

            let _abstr = p_getter.is_some_and(|p| p.is_abstract_method())
                || p_setter.is_some_and(|p| p.is_abstract_method());

            let index = p_getter.is_some_and(|p| p.parameter_count > 0);

            // Need to include this type
            cpp_type.declarations.push(
                CppMember::Property(CppPropertyDecl {
                    cpp_name: config.name_cpp(p_name),
                    prop_ty: p_ty_cpp_name.clone(),
                    // methods generated in make_methods
                    setter: p_setter.map(|m| config.name_cpp(m.name(metadata.metadata))),
                    getter: p_getter.map(|m| config.name_cpp(m.name(metadata.metadata))),
                    indexable: index,
                    brief_comment: None,
                    instance: true,
                })
                .into(),
            );
        }
    }

    fn create_size_assert(&mut self) {
        let cpp_type = self.get_mut_cpp_type();

        // FIXME: make this work with templated types that either: have a full template (complete instantiation), or only require a pointer (size should be stable)
        // for now, skip templated types
        if cpp_type.cpp_template.is_some() {
            return;
        }

        if let Some(size) = cpp_type.size_info.as_ref().map(|s| s.instance_size) {
            let cpp_name = cpp_type.cpp_name_components.remove_pointer().combine_all();

            let assert = CppStaticAssert {
                condition: format!("::cordl_internals::size_check_v<{cpp_name}, 0x{size:x}>"),
                message: Some("Size mismatch!".to_string()),
            };

            cpp_type
                .nonmember_declarations
                .push(Rc::new(CppNonMember::CppStaticAssert(assert)));
        } else {
            todo!("Why does this type not have a valid size??? {cpp_type:?}");
        }
    }
    ///
    /// add missing size for type
    ///
    fn create_size_padding(&mut self, metadata: &Metadata, tdi: TypeDefinitionIndex) {
        let cpp_type = self.get_mut_cpp_type();

        // // get type metadata size
        let Some(type_definition_sizes) = &metadata.metadata_registration.type_definition_sizes
        else {
            return;
        };

        let metadata_size = &type_definition_sizes.get(tdi.index() as usize);

        let Some(metadata_size) = metadata_size else {
            return;
        };

        // // ignore types that aren't sized
        if metadata_size.instance_size == 0 || metadata_size.instance_size == u32::MAX {
            return;
        }

        // // if the size matches what we calculated, we're fine
        // if metadata_size.instance_size == calculated_size {
        //     return;
        // }
        // let remaining_size = metadata_size.instance_size.abs_diff(calculated_size);

        let Some(size_info) = cpp_type.size_info.as_ref() else {
            return;
        };

        // for all types, the size il2cpp metadata says the type should be, for generics this is calculated though
        let metadata_size_instance = size_info.instance_size;

        // align the calculated size to the next multiple of natural_alignment, similiar to what happens when clang compiles our generated code
        // this comes down to adding our size, and removing any bits that make it more than the next multiple of alignment
        let aligned_calculated_size = match size_info.natural_alignment as u32 {
            0 => size_info.calculated_instance_size,
            alignment => (size_info.calculated_instance_size + alignment) & !(alignment - 1),
        };

        // return if calculated layout size == metadata size
        if aligned_calculated_size == metadata_size_instance {
            return;
        }

        let remaining_size = metadata_size_instance.abs_diff(size_info.calculated_instance_size);

        // pack the remaining size to fit the packing of the type
        let closest_packing = |size: u32| {
            match size {
                0 => 0,
                1 => 1,
                2 => 2,
                3 => 4,
                4 => 4,
                _ => 8
            }
        };

        let packing = cpp_type.packing.unwrap_or_else(|| closest_packing(size_info.calculated_instance_size));
        let packed_remaining_size = match packing == 0 {
            true => remaining_size,
            false => remaining_size & !(packing as u32 - 1),
        };

        // if the packed remaining size ends up being 0, don't emit padding
        if packed_remaining_size == 0 {
            return;
        }

        cpp_type.declarations.push(
            CppMember::FieldDecl(CppFieldDecl {
                cpp_name: format!("_cordl_size_padding[0x{packed_remaining_size:x}]").to_string(),
                field_ty: "uint8_t".into(),
                offset: size_info.instance_size,
                instance: true,
                readonly: false,
                const_expr: false,
                value: None,
                brief_comment: Some(format!(
                    "Size padding 0x{:x} - 0x{:x} = 0x{remaining_size:x}, packed as 0x{packed_remaining_size:x}",
                    metadata_size_instance, size_info.calculated_instance_size
                )),
                is_private: false,
            })
            .into(),
        );
    }

    fn create_ref_size(&mut self) {
        let cpp_type = self.get_mut_cpp_type();
        if let Some(size) = cpp_type.size_info.as_ref().map(|s| s.instance_size) {
            cpp_type.declarations.push(
                CppMember::FieldDecl(CppFieldDecl {
                    cpp_name: REFERENCE_TYPE_WRAPPER_SIZE.to_string(),
                    field_ty: "auto".to_string(),
                    offset: u32::MAX,
                    instance: false,
                    readonly: false,
                    const_expr: true,
                    value: Some(format!("0x{size:x}")),
                    brief_comment: Some("The size of the true reference type".to_string()),
                    is_private: false,
                })
                .into(),
            );

            // here we push an instance field like uint8_t __fields[total_size - base_size] to make sure ref types are the exact size they should be
            let fixup_size = match cpp_type.inherit.first() {
                Some(base_type) => format!("0x{size:x} - sizeof({base_type})"),
                None => format!("0x{size:x}"),
            };

            cpp_type.declarations.push(
                CppMember::FieldDecl(CppFieldDecl {
                    cpp_name: format!("{REFERENCE_TYPE_FIELD_SIZE}[{fixup_size}]"),
                    field_ty: "uint8_t".to_string(),
                    offset: u32::MAX,
                    instance: true,
                    readonly: false,
                    const_expr: false,
                    value: Some("".into()),
                    brief_comment: Some(
                        "The size this ref type adds onto its base type, may evaluate to 0"
                            .to_string(),
                    ),
                    is_private: false,
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

        let enum_base = cpp_type
            .cppify_name_il2cpp(ctx_collection, metadata, backing_field_ty, 0)
            .remove_pointer()
            .combine_all();

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
        let unwrapped_name = format!("__{}_Unwrapped", cpp_type.cpp_name());
        let backing_field = metadata
            .metadata_registration
            .types
            .get(t.element_type_index as usize)
            .unwrap();

        let enum_base = cpp_type
            .cppify_name_il2cpp(ctx_collection, metadata, backing_field, 0)
            .remove_pointer()
            .combine_all();

        let enum_entries = t
            .fields(metadata.metadata)
            .iter()
            .enumerate()
            .map(|(i, field)| {
                let field_index = FieldIndex::new(t.field_start.index() + i as u32);

                (field_index, field)
            })
            .filter_map(|(field_index, field)| {
                let f_type = metadata
                    .metadata_registration
                    .types
                    .get(field.type_index as usize)
                    .unwrap();

                f_type.is_static().then(|| {
                    // enums static fields are always the enum values
                    let f_name = field.name(metadata.metadata);
                    let value = Self::field_default_value(metadata, field_index)
                        .expect("Enum without value!");

                    // prepend enum name with __E_ to prevent accidentally creating enum values that are reserved for builtin macros
                    format!("__E_{f_name} = {value},")
                })
            })
            .map(|s| -> CppMember { CppMember::CppLine(s.into()) });

        let nested_struct = CppNestedStruct {
            base_type: Some(enum_base),
            declaring_name: unwrapped_name.clone(),
            is_class: false,
            is_enum: true,
            is_private: false,
            declarations: enum_entries.map(Rc::new).collect(),
            brief_comment: Some(format!("Nested struct {unwrapped_name}")),
            packing: None,
        };
        cpp_type
            .declarations
            .push(CppMember::NestedStruct(nested_struct).into());

        let operator_body = format!("return static_cast<{unwrapped_name}>(this->value__);");
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
            is_inline: true,
        };

        cpp_type
            .declarations
            .push(CppMember::MethodDecl(operator_decl).into());
    }

    fn create_valuetype_field_wrapper(&mut self) {
        let cpp_type = self.get_mut_cpp_type();
        if cpp_type.size_info.is_none() {
            todo!("Why does this type not have a valid size??? {:?}", cpp_type);
        }

        let size = cpp_type
            .size_info
            .as_ref()
            .map(|s| s.instance_size)
            .unwrap();

        cpp_type.requirements.needs_byte_include();
        cpp_type.declarations.push(
            CppMember::FieldDecl(CppFieldDecl {
                cpp_name: VALUE_TYPE_WRAPPER_SIZE.to_string(),
                field_ty: "auto".to_string(),
                offset: u32::MAX,
                instance: false,
                readonly: false,
                const_expr: true,
                value: Some(format!("0x{size:x}")),
                brief_comment: Some("The size of the true value type".to_string()),
                is_private: false,
            })
            .into(),
        );

        // cpp_type.declarations.push(
        //     CppMember::ConstructorDecl(CppConstructorDecl {
        //         cpp_name: cpp_type.cpp_name().clone(),
        //         parameters: vec![CppParam {
        //             name: "instance".to_string(),
        //             ty: format!("std::array<std::byte, {VALUE_TYPE_WRAPPER_SIZE}>"),
        //             modifiers: Default::default(),
        //             def_value: None,
        //         }],
        //         template: None,
        //         is_constexpr: true,
        //         is_explicit: true,
        //         is_default: false,
        //         is_no_except: true,
        //         is_delete: false,
        //         is_protected: false,
        //         base_ctor: Some((
        //             cpp_type.inherit.first().unwrap().to_string(),
        //             "instance".to_string(),
        //         )),
        //         initialized_values: Default::default(),
        //         brief: Some(
        //             "Constructor that lets you initialize the internal array explicitly".into(),
        //         ),
        //         body: Some(vec![]),
        //     })
        //     .into(),
        // );
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

                let f_type_cpp_name = cpp_type
                    .cppify_name_il2cpp(ctx_collection, metadata, f_type, 0)
                    .combine_all();

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


        if instance_fields.is_empty() {
            return;
        }
        // Maps into the first parent -> ""
        // so then Parent()
        let base_ctor = cpp_type
            .inherit
            .first()
            .map(|s| (s.clone(), "".to_string()));

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
            is_delete: false,
            is_protected: false,

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
            declaring_full_name: cpp_type.cpp_name_components.remove_pointer().combine_all(),
            ..constructor_decl.clone().into()
        };

        cpp_type
            .declarations
            .push(CppMember::ConstructorDecl(constructor_decl).into());
        cpp_type
            .implementations
            .push(CppMember::ConstructorImpl(constructor_impl).into());
    }

    fn create_valuetype_default_constructors(&mut self) {
        let cpp_type = self.get_mut_cpp_type();
        // create the various copy and move ctors and operators
        let cpp_name = cpp_type.cpp_name();
        let wrapper = format!("{VALUE_WRAPPER_TYPE}<{VALUE_TYPE_WRAPPER_SIZE}>::instance");

        let move_ctor = CppConstructorDecl {
            cpp_name: cpp_name.clone(),
            parameters: vec![CppParam {
                ty: cpp_name.clone(),
                name: "".to_string(),
                modifiers: "&&".to_string(),
                def_value: None,
            }],
            template: None,
            is_constexpr: true,
            is_explicit: false,
            is_default: true,
            is_no_except: false,
            is_delete: false,
            is_protected: false,
            base_ctor: None,
            initialized_values: Default::default(),
            brief: None,
            body: None,
        };

        let copy_ctor = CppConstructorDecl {
            cpp_name: cpp_name.clone(),
            parameters: vec![CppParam {
                ty: cpp_name.clone(),
                name: "".to_string(),
                modifiers: "const &".to_string(),
                def_value: None,
            }],
            template: None,
            is_constexpr: true,
            is_explicit: false,
            is_default: true,
            is_no_except: false,
            is_delete: false,
            is_protected: false,
            base_ctor: None,
            initialized_values: Default::default(),
            brief: None,
            body: None,
        };

        let move_operator_eq = CppMethodDecl {
            cpp_name: "operator=".to_string(),
            return_type: format!("{cpp_name}&"),
            parameters: vec![CppParam {
                ty: cpp_name.clone(),
                name: "o".to_string(),
                modifiers: "&&".to_string(),
                def_value: None,
            }],
            instance: true,
            template: None,
            suffix_modifiers: vec![],
            prefix_modifiers: vec![],
            is_virtual: false,
            is_constexpr: true,
            is_const: false,
            is_no_except: true,
            is_operator: false,
            is_inline: false,
            brief: None,
            body: Some(vec![
                Arc::new(CppLine::make(format!(
                    "this->{wrapper} = std::move(o.{wrapper});"
                ))),
                Arc::new(CppLine::make("return *this;".to_string())),
            ]),
        };

        let copy_operator_eq = CppMethodDecl {
            cpp_name: "operator=".to_string(),
            return_type: format!("{cpp_name}&"),
            parameters: vec![CppParam {
                ty: cpp_name.clone(),
                name: "o".to_string(),
                modifiers: "const &".to_string(),
                def_value: None,
            }],
            instance: true,
            template: None,
            suffix_modifiers: vec![],
            prefix_modifiers: vec![],
            is_virtual: false,
            is_constexpr: true,
            is_const: false,
            is_no_except: true,
            is_operator: false,
            is_inline: false,
            brief: None,
            body: Some(vec![
                Arc::new(CppLine::make(format!("this->{wrapper} = o.{wrapper};"))),
                Arc::new(CppLine::make("return *this;".to_string())),
            ]),
        };

        cpp_type
            .declarations
            .push(CppMember::ConstructorDecl(move_ctor).into());
        cpp_type
            .declarations
            .push(CppMember::ConstructorDecl(copy_ctor).into());
        cpp_type
            .declarations
            .push(CppMember::MethodDecl(move_operator_eq).into());
        cpp_type
            .declarations
            .push(CppMember::MethodDecl(copy_operator_eq).into());
    }

    fn create_ref_default_constructor(&mut self) {
        let cpp_type = self.get_mut_cpp_type();
        let cpp_name = cpp_type.cpp_name().clone();

        let cs_name = cpp_type.name().clone();

        // Skip if System.ValueType or System.Enum
        if cpp_type.namespace() == "System" && (cs_name == "ValueType" || cs_name == "Enum") {
            return;
        }

        let default_ctor = CppConstructorDecl {
            cpp_name: cpp_name.clone(),
            parameters: vec![],
            template: None,
            is_constexpr: true,
            is_explicit: false,
            is_default: true,
            is_no_except: true,
            is_delete: false,
            is_protected: true,

            base_ctor: None,
            initialized_values: HashMap::new(),
            brief: Some("Default ctor for custom type constructor invoke".to_string()),
            body: None,
        };
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
            is_delete: false,
            is_protected: false,

            base_ctor: None,
            initialized_values: HashMap::new(),
            brief: None,
            body: None,
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
            is_delete: false,
            is_protected: false,

            base_ctor: None,
            initialized_values: HashMap::new(),
            brief: None,
            body: None,
        };

        cpp_type
            .declarations
            .push(CppMember::ConstructorDecl(default_ctor).into());
        cpp_type
            .declarations
            .push(CppMember::ConstructorDecl(copy_ctor).into());
        cpp_type
            .declarations
            .push(CppMember::ConstructorDecl(move_ctor).into());

        // // Delegates and such are reference types with no inheritance
        // if cpp_type.inherit.is_empty() {
        //     return;
        // }

        // let base_type = cpp_type
        //     .inherit
        //     .get(0)
        //     .expect("No parent for reference type?");

        // cpp_type.declarations.push(
        //     CppMember::ConstructorDecl(CppConstructorDecl {
        //         cpp_name: cpp_name.clone(),
        //         parameters: vec![CppParam {
        //             name: "ptr".to_string(),
        //             modifiers: "".to_string(),
        //             ty: "void*".to_string(),
        //             def_value: None,
        //         }],
        //         template: None,
        //         is_constexpr: true,
        //         is_explicit: true,
        //         is_default: false,
        //         is_no_except: true,
        //         is_delete: false,
        //         is_protected: false,

        //         base_ctor: Some((base_type.clone(), "ptr".to_string())),
        //         initialized_values: HashMap::new(),
        //         brief: None,
        //         body: Some(vec![]),
        //     })
        //     .into(),
        // );
    }
    fn make_interface_constructors(&mut self) {
        let cpp_type = self.get_mut_cpp_type();
        let cpp_name = cpp_type.cpp_name().clone();

        let base_type = cpp_type
            .inherit
            .first()
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
                is_delete: false,
                is_protected: false,

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

    fn delete_move_ctor(&mut self) {
        let cpp_type = self.get_mut_cpp_type();
        let t = &cpp_type.cpp_name_components.name;

        let move_ctor = CppConstructorDecl {
            cpp_name: t.clone(),
            parameters: vec![CppParam {
                def_value: None,
                modifiers: "&&".to_string(),
                name: "".to_string(),
                ty: t.clone(),
            }],
            template: None,
            is_constexpr: false,
            is_explicit: false,
            is_default: false,
            is_no_except: false,
            is_protected: false,
            is_delete: true,
            base_ctor: None,
            initialized_values: Default::default(),
            brief: Some("delete move ctor to prevent accidental deref moves".to_string()),
            body: None,
        };

        cpp_type
            .declarations
            .push(CppMember::ConstructorDecl(move_ctor).into());
    }

    fn delete_copy_ctor(&mut self) {
        let cpp_type = self.get_mut_cpp_type();
        let t = &cpp_type.cpp_name_components.name;

        let move_ctor = CppConstructorDecl {
            cpp_name: t.clone(),
            parameters: vec![CppParam {
                def_value: None,
                modifiers: "const&".to_string(),
                name: "".to_string(),
                ty: t.clone(),
            }],
            template: None,
            is_constexpr: false,
            is_explicit: false,
            is_default: false,
            is_no_except: false,
            is_delete: true,
            is_protected: false,
            base_ctor: None,
            initialized_values: Default::default(),
            brief: Some("delete copy ctor to prevent accidental deref copies".to_string()),
            body: None,
        };

        cpp_type
            .declarations
            .push(CppMember::ConstructorDecl(move_ctor).into());
    }

    fn add_default_ctor(&mut self, protected: bool) {
        let cpp_type = self.get_mut_cpp_type();
        let t = &cpp_type.cpp_name_components.name;

        let default_ctor_decl = CppConstructorDecl {
            cpp_name: t.clone(),
            parameters: vec![],
            template: None,
            is_constexpr: true,
            is_explicit: false,
            is_default: false,
            is_no_except: false,
            is_delete: false,
            is_protected: protected,
            base_ctor: None,
            initialized_values: Default::default(),
            brief: Some("default ctor".to_string()),
            body: None,
        };

        let default_ctor_impl = CppConstructorImpl {
            body: vec![],
            declaring_full_name: cpp_type.cpp_name_components.remove_pointer().combine_all(),
            template: cpp_type.cpp_template.clone(),
            ..default_ctor_decl.clone().into()
        };

        cpp_type
            .declarations
            .push(CppMember::ConstructorDecl(default_ctor_decl).into());

        cpp_type
            .implementations
            .push(CppMember::ConstructorImpl(default_ctor_impl).into());
    }

    fn delete_default_ctor(&mut self) {
        let cpp_type = self.get_mut_cpp_type();
        let t = &cpp_type.cpp_name_components.name;

        let default_ctor = CppConstructorDecl {
            cpp_name: t.clone(),
            parameters: vec![],
            template: None,
            is_constexpr: false,
            is_explicit: false,
            is_default: false,
            is_no_except: false,
            is_delete: true,
            is_protected: false,
            base_ctor: None,
            initialized_values: Default::default(),
            brief: Some(
                "delete default ctor to prevent accidental value type instantiations of ref types"
                    .to_string(),
            ),
            body: None,
        };

        cpp_type
            .declarations
            .push(CppMember::ConstructorDecl(default_ctor).into());
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

        let ty_full_cpp_name = cpp_type.cpp_name_components.combine_all();

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
            body: vec![Arc::new(CppLine::make(format!("return {allocate_call};")))],

            declaring_cpp_full_name: cpp_type.cpp_name_components.remove_pointer().combine_all(),
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
                    cpp_type.requirements.needs_enum_include();
                    ENUM_PTR_TYPE.into()
                } else if full_name == "System.ValueType" {
                    cpp_type.requirements.needs_value_include();
                    VT_PTR_TYPE.into()
                } else {
                    cpp_type
                        .cppify_name_il2cpp(ctx_collection, metadata, param_type, 0)
                        .combine_all()
                }
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
                .map(|t| {
                    cpp_type
                        .cppify_name_il2cpp(ctx_collection, metadata, t, 0)
                        .combine_all()
                })
                .collect_vec()
        });

        // Lazy cppify
        let make_ret_cpp_type_name = |cpp_type: &mut CppType| -> String {
            let full_name = m_ret_type.full_name(metadata.metadata);
            if full_name == "System.Enum" {
                cpp_type.requirements.needs_enum_include();
                ENUM_PTR_TYPE.into()
            } else if full_name == "System.ValueType" {
                cpp_type.requirements.needs_value_include();
                VT_PTR_TYPE.into()
            } else {
                cpp_type
                    .cppify_name_il2cpp(ctx_collection, metadata, m_ret_type, 0)
                    .combine_all()
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
                true => {
                    cpp_m_name
                        + "_"
                        + &config
                            .generic_nested_name(&m_ret_cpp_type_name)
                            .replace('*', "_")
                }
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
                "Method {m_name} addr 0x{:x}, size 0x{:x}, virtual {}, abstract {}, final {}",
                method_calc.map(|m| m.addrs).unwrap_or(u64::MAX),
                method_calc.map(|m| m.estimated_size).unwrap_or(usize::MAX),
                method.is_virtual_method(),
                method.is_abstract_method(),
                method.is_final_method()
            )
            .into(),
            is_const: false,
            is_constexpr: false,
            is_no_except: false,
            cpp_name: cpp_m_name.clone(),
            return_type: m_ret_cpp_type_name.clone(),
            parameters: m_params_with_def.clone(),
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
            "this".into()
        };

        const METHOD_INFO_VAR_NAME: &str = "___internal_method";

        let method_invoke_params = vec![instance_ptr.as_str(), METHOD_INFO_VAR_NAME];
        let param_names = CppParam::params_names(&method_decl.parameters).map(|s| s.as_str());
        let declaring_type_cpp_full_name =
            cpp_type.cpp_name_components.remove_pointer().combine_all();

        let declaring_classof_call = format!(
            "::il2cpp_utils::il2cpp_type_check::il2cpp_no_arg_class<{}>::get()",
            cpp_type.cpp_name_components.combine_all()
        );

        let extract_self_class =
            "il2cpp_functions::object_get_class(reinterpret_cast<Il2CppObject*>(this))";

        let params_types_format: String = CppParam::params_types(&method_decl.parameters)
            .map(|t| format!("::il2cpp_utils::il2cpp_type_check::il2cpp_no_arg_type<{t}>::get()"))
            .join(", ");

        let resolve_instance_slot_lines = if method.slot != u16::MAX {
            let slot = &method.slot;
            vec![format!(
                "auto* {METHOD_INFO_VAR_NAME} = THROW_UNLESS((::il2cpp_utils::ResolveVtableSlot(
                    {extract_self_class},
                    {declaring_classof_call},
                    {slot}
                )));"
            )]
        } else {
            vec![]
        };

        // TODO: link the method to the interface that originally declared it
        // then the resolve should look something like:
        // resolve(classof(GlobalNamespace::BeatmapLevelPack*), classof(GlobalNamespace::IBeatmapLevelPack*), 0);
        // that way the resolve should work correctly, but it should only happen like that for non-interfaces

        let resolve_metadata_slot_lines = if method.slot != u16::MAX {
            let self_classof_call = "";
            let declaring_classof_call = "";
            let slot = &method.slot;

            vec![format!(
                "auto* {METHOD_INFO_VAR_NAME} = THROW_UNLESS((::il2cpp_utils::ResolveVtableSlot(
                    {self_classof_call},
                    {declaring_classof_call},
                    {slot}
                )));"
            )]
        } else {
            vec![]
        };

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

        // instance methods should resolve slots if this is an interface, or if this is a virtual/abstract method, and not a final method
        // static methods can't be virtual or interface anyway so checking for that here is irrelevant
        let should_resolve_slot = cpp_type.is_interface || ((method.is_virtual_method() || method.is_abstract_method()) && !method.is_final_method());

        let method_body = match should_resolve_slot {
            true => resolve_instance_slot_lines
                .iter()
                .chain(method_body_lines.iter())
                .cloned()
                .map(|l| -> Arc<dyn Writable> { Arc::new(CppLine::make(l)) })
                .collect_vec(),
            false => method_info_lines
                .iter()
                .chain(method_body_lines.iter())
                .cloned()
                .map(|l| -> Arc<dyn Writable> { Arc::new(CppLine::make(l)) })
                .collect_vec(),
        };

        let method_impl = CppMethodImpl {
            body: method_body,
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
        if let Some(method_calc) = method_calc
            && template.is_none()
            && !has_template_args
            && !is_generic_method_inst
        {
            cpp_type
                .nonmember_implementations
                .push(Rc::new(CppNonMember::SizeStruct(
                    CppMethodSizeStruct {
                        ret_ty: method_decl.return_type.clone(),
                        cpp_method_name: method_decl.cpp_name.clone(),
                        method_name: m_name.to_string(),
                        declaring_type_name: method_impl.declaring_cpp_full_name.clone(),
                        declaring_classof_call,
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
                    }
                    .into(),
                )));
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
            false => "nullptr".to_string(),
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
    ) -> NameComponents {
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

    /// [declaring_generic_inst_types] the generic instantiation of the declaring type
    fn cppify_name_il2cpp_recurse(
        &self,
        requirements: &mut CppTypeRequirements,
        ctx_collection: &CppContextCollection,
        metadata: &Metadata,
        typ: &Il2CppType,
        include_depth: usize,
        declaring_generic_inst_types: Option<&Vec<usize>>,
    ) -> NameComponents {
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
            // Commented so types use System.Object
            // might revert

            // Il2CppTypeEnum::Object => {
            //     requirements.need_wrapper();
            //     OBJECT_WRAPPER_TYPE.to_string()
            // }
            Il2CppTypeEnum::Object
            | Il2CppTypeEnum::Valuetype
            | Il2CppTypeEnum::Class
            | Il2CppTypeEnum::Typedbyref => {
                let typ_cpp_tag: CppTypeTag = typ_tag.into();
                // Self

                // we add :: here since we can't add it to method ddefinitions
                // e.g void ::Foo::method() <- not allowed
                if typ_cpp_tag == cpp_type.self_tag {
                    return cpp_type.cpp_name_components.clone();
                }

                if let TypeData::TypeDefinitionIndex(tdi) = typ.data {
                    let td = &metadata.metadata.global_metadata.type_definitions[tdi];

                    // TODO: Do we need generic inst types here? Hopefully not!
                    let _size = offsets::get_sizeof_type(td, tdi, None, metadata);

                    if metadata.blacklisted_types.contains(&tdi) {
                        // classes should return Il2CppObject*
                        if typ.ty == Il2CppTypeEnum::Class {
                            return NameComponents {
                                name: IL2CPP_OBJECT_TYPE.to_string(),
                                is_pointer: true,
                                generics: None,
                                namespace: None,
                                declaring_types: None,
                            };
                        }
                        return wrapper_type_for_tdi(td).to_string().into();
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

                let typedef_incl = CppInclude::new_context_typedef(to_incl);
                let typeimpl_incl = CppInclude::new_context_typeimpl(to_incl);
                let to_incl_cpp_ty = ctx_collection
                    .get_cpp_type(typ.data.into())
                    .unwrap_or_else(|| panic!("Unable to get type to include {:?}", typ.data));

                let own_context = other_context_ty == own_context_ty;

                // - Include it
                // Skip including the context if we're already in it
                if !own_context {
                    match add_include {
                        // add def include
                        true => {
                            requirements.add_def_include(Some(to_incl_cpp_ty), typedef_incl.clone());
                            requirements.add_impl_include(Some(to_incl_cpp_ty), typeimpl_incl.clone());
                        }
                        // TODO: Remove?
                        // ignore nested types
                        // false if to_incl_cpp_ty.nested => {
                        // TODO: What should we do here?
                        // error!("Can't forward declare nested type! Including!");
                        // requirements.add_include(Some(to_incl_cpp_ty), inc);
                        // }
                        // forward declare
                        false => {
                            requirements.add_forward_declare((
                                CppForwardDeclare::from_cpp_type(to_incl_cpp_ty),
                                typedef_incl,
                            ));
                        }
                    }
                }

                to_incl_cpp_ty.cpp_name_components.clone()

                // match to_incl_cpp_ty.is_enum_type || to_incl_cpp_ty.is_value_type {
                //     true => ret,
                //     false => format!("{ret}*"),
                // }
            }
            // Single dimension array
            Il2CppTypeEnum::Szarray => {
                requirements.needs_arrayw_include();

                let generic = match typ.data {
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

                let generic_formatted = generic.combine_all();

                NameComponents {
                    name: "ArrayW".into(),
                    namespace: Some("".into()),
                    generics: Some(vec![
                        generic_formatted.clone(),
                        format!("::Array<{generic_formatted}>*"),
                    ]),
                    is_pointer: false,
                    ..Default::default()
                }
            }
            // multi dimensional array
            Il2CppTypeEnum::Array => {
                // FIXME: when stack further implements the TypeData::ArrayType we can actually implement this fully to be a multidimensional array, whatever that might mean
                warn!("Multidimensional array was requested but this is not implemented, typ: {typ:?}, instead returning Il2CppObject!");
                NameComponents {
                    name: IL2CPP_OBJECT_TYPE.to_string(),
                    is_pointer: true,
                    generics: None,
                    namespace: None,
                    declaring_types: None,
                }
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
                        return gen_param.name(metadata.metadata).to_string().into();
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
                            true => gen_name.to_string().into(),
                            false => panic!("/* TODO: FIX THIS, THIS SHOULDN'T HAPPEN! NO GENERIC INST ARGS FOUND HERE */ {gen_name}"),
                        };
                    }

                    let ty_var = &metadata.metadata_registration.types[ty_idx_opt.unwrap()];

                    let generics = &cpp_type
                        .cpp_name_components
                        .generics
                        .as_ref()
                        .expect("Generic instantiation args not made yet!");

                    let resolved_var = generics
                        .get(generic_param.num as usize)
                        .expect("No generic parameter at index found!")
                        .clone();

                    let is_pointer = !ty_var.valuetype
                    // if resolved_var exists in generic template, it can't be a pointer!
                        && (cpp_type.cpp_template.is_none()
                            || !cpp_type
                                .cpp_template
                                .as_ref()
                                .is_some_and(|t| t.just_names().any(|s| s == &resolved_var)));

                    NameComponents {
                        is_pointer,
                        name: resolved_var,
                        ..Default::default()
                    }

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
                        let generic_tag = CppTypeTag::from_type_data(typ.data, metadata.metadata);

                        // depend on both tdi and generic instantiation
                        requirements.add_dependency_tag(tdi.into());
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
                        .map(|n| n.combine_all())
                        .collect_vec();

                    let generic_type_def = &mr.types[generic_class.type_index];
                    let type_def_name_components = cpp_type.cppify_name_il2cpp_recurse(
                        requirements,
                        ctx_collection,
                        metadata,
                        generic_type_def,
                        include_depth,
                        Some(new_generic_inst_types),
                    );

                    // add generics to type def
                    NameComponents {
                        generics: Some(generic_types_formatted),
                        ..type_def_name_components
                    }
                }

                _ => panic!("Unknown type data for generic inst {typ:?}!"),
            },
            Il2CppTypeEnum::I1 => "int8_t".to_string().into(),
            Il2CppTypeEnum::I2 => "int16_t".to_string().into(),
            Il2CppTypeEnum::I4 => "int32_t".to_string().into(),
            Il2CppTypeEnum::I8 => "int64_t".to_string().into(),
            Il2CppTypeEnum::I => "void*".to_string().into(),
            Il2CppTypeEnum::U1 => "uint8_t".to_string().into(),
            Il2CppTypeEnum::U2 => "uint16_t".to_string().into(),
            Il2CppTypeEnum::U4 => "uint32_t".to_string().into(),
            Il2CppTypeEnum::U8 => "uint64_t".to_string().into(),
            Il2CppTypeEnum::U => "void*".to_string().into(),

            // https://learn.microsoft.com/en-us/nimbusml/concepts/types
            // https://en.cppreference.com/w/cpp/types/floating-point
            Il2CppTypeEnum::R4 => "float_t".to_string().into(),
            Il2CppTypeEnum::R8 => "double_t".to_string().into(),

            Il2CppTypeEnum::Void => "void".to_string().into(),
            Il2CppTypeEnum::Boolean => "bool".to_string().into(),
            Il2CppTypeEnum::Char => "char16_t".to_string().into(),
            Il2CppTypeEnum::String => {
                requirements.needs_stringw_include();
                "::StringW".to_string().into()
            }
            Il2CppTypeEnum::Ptr => {
                let generic = match typ.data {
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

                let generic_formatted = generic.combine_all();

                NameComponents {
                    namespace: Some("cordl_internals".into()),
                    generics: Some(vec![generic_formatted]),
                    name: "Ptr".into(),
                    ..Default::default()
                }
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
            _ => format!("/* UNKNOWN TYPE! {typ:?} */").into(),
        };

        ret
    }

    fn classof_cpp_name(&self) -> String {
        format!(
            "::il2cpp_utils::il2cpp_type_check::il2cpp_no_arg_class<{}>::get",
            self.get_cpp_type().cpp_name_components.combine_all()
        )
    }

    fn type_name_byref_fixup(ty: &Il2CppType, name: &str) -> String {
        match ty.valuetype {
            true => name.to_string(),
            false => format!("{name}*"),
        }
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

    IL2CPP_OBJECT_TYPE
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
) -> NameComponents {
    // If reference type, we use a template and add a requirement
    if !t.valuetype {
        template_args.push((
            CORDL_REFERENCE_TYPE_CONSTRAINT.to_string(),
            gen_name.clone(),
        ));
        return gen_name.into();
    }

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
        let inner_enum_type_cpp = cpp_type
            .cppify_name_il2cpp(ctx_collection, metadata, &inner_enum_type, 0)
            .combine_all();

        template_args.push((
            format!("{CORDL_NUM_ENUM_TYPE_CONSTRAINT}<{inner_enum_type_cpp}>",),
            gen_name.clone(),
        ));

        return gen_name.into();
    }

    let inner_type = cpp_type.cppify_name_il2cpp(ctx_collection, metadata, t, 0);

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
                .map(|n| n.combine_all())
                .collect_vec();

            NameComponents {
                generics: Some(inner_generic_params),
                ..non_generic_inner_type
            }
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
