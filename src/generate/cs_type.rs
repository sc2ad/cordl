use core::panic;
use std::{
    collections::HashMap,
    io::{Cursor, Read},
    rc::Rc,
};

use brocolib::{
    global_metadata::{
        FieldIndex, Il2CppMethodDefinition, Il2CppTypeDefinition, MethodIndex, ParameterIndex,
        TypeDefinitionIndex, TypeIndex,
    },
    runtime_metadata::{
        Il2CppMethodSpec, Il2CppType, Il2CppTypeDefinitionSizes, Il2CppTypeEnum, TypeData,
    },
};
use byteorder::{LittleEndian, ReadBytesExt};

use itertools::Itertools;

use crate::{
    generate::members::CppUsingAlias, helpers::cursor::ReadBytesExtensions, STATIC_CONFIG,
};

use super::{
    config::GenerationConfig,
    context_collection::{CppContextCollection, CppTypeTag},
    cpp_type::{self, CppType},
    members::{
        CppCommentedString, CppConstructorDecl, CppConstructorImpl, CppFieldDecl, CppFieldImpl,
        CppForwardDeclare, CppInclude, CppLine, CppMember, CppMethodData, CppMethodDecl,
        CppMethodImpl, CppMethodSizeStruct, CppParam, CppProperty, CppStaticAssert, CppTemplate,
    },
    metadata::Metadata,
    type_extensions::{
        MethodDefintionExtensions, ParameterDefinitionExtensions, TypeDefinitionExtensions,
        TypeExtentions, OBJECT_WRAPPER_TYPE, TYPE_ATTRIBUTE_INTERFACE,
    },
};

type Endian = LittleEndian;

// negative
const VALUE_TYPE_SIZE_OFFSET: u32 = 0x10;

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

    fn parent_joined_cpp_name(
        metadata: &Metadata,
        config: &GenerationConfig,
        tdi: TypeDefinitionIndex,
    ) -> String {
        let ty_def = &metadata.metadata.global_metadata.type_definitions[tdi];

        let name = ty_def.name(metadata.metadata);

        if ty_def.declaring_type_index != u32::MAX {
            let declaring_ty =
                metadata.metadata_registration.types[ty_def.declaring_type_index as usize];

            if let TypeData::TypeDefinitionIndex(declaring_tdi) = declaring_ty.data {
                return Self::parent_joined_cpp_name(metadata, config, declaring_tdi) + "/" + name;
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
        tag: CppTypeTag,
        tdi: TypeDefinitionIndex,
    ) -> Option<CppType> {
        // let iface = metadata.interfaces.get(t.interfaces_start);
        // Then, handle interfaces

        // Then, handle methods
        // - This includes constructors
        // inherited methods will be inherited

        let parent_pair = metadata.child_to_parent_map.get(&tdi);

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

        let cpp_template = generics.as_ref().map(|g| CppTemplate {
            names: g
                .iter()
                .map(|(g, _)| g.name(metadata.metadata).to_string())
                .collect(),
        });

        let ns = t.namespace(metadata.metadata);
        let name = t.name(metadata.metadata);
        let full_name = t.full_name(metadata.metadata, false);

        let nested = t.declaring_type_index != u32::MAX;
        let cpp_full_name = t.full_name_cpp(metadata.metadata, config, false);

        let mut cpptype = CppType {
            self_tag: tag,
            nested: parent_pair.is_some(),
            prefix_comments: vec![format!("Type: {ns}::{name}")],
            namespace: ns.to_string(),
            cpp_namespace: config.namespace_cpp(ns),
            name: name.to_string(),
            cpp_name: config.name_cpp(name),
            parent_ty_tdi: parent_pair.map(|p| p.tdi),

            parent_ty_name: parent_pair
                .map(|p| Self::parent_joined_cpp_name(metadata, config, p.tdi)),
            cpp_full_name,
            full_name,

            declarations: Default::default(),
            implementations: Default::default(),
            nonmember_implementations: Default::default(),
            nonmember_declarations: Default::default(),

            is_value_type: t.is_value_type(),
            requirements: Default::default(),

            inherit: Default::default(),
            cpp_template,

            generic_instantiations_args_types: Default::default(),
            generic_instantiation_args: Default::default(),
            method_generic_instantiation_map: Default::default(),

            is_stub: generics.is_some(),
            nested_types: Default::default(),
        };

        if t.parent_index == u32::MAX {
            if t.flags & TYPE_ATTRIBUTE_INTERFACE == 0 {
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

        cpptype.make_nested_types(metadata, config, tdi);
        if cpptype.is_value_type {
            // if let Some(size) = &get_size_of_type_table(metadata, tdi) {
            let size = Self::layout_fields_locked_size(t, tdi, metadata);
            cpptype
                .nonmember_declarations
                .push(Rc::new(CppStaticAssert {
                    condition: format!(
                        "sizeof({}) == 0x{:X}",
                        cpptype.cpp_full_name, size /* - VALUE_TYPE_SIZE_OFFSET */
                    ),
                    message: Some(format!(
                        "Type {} does not match expected size!",
                        cpptype.cpp_full_name
                    )),
                }))
            // }
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

        self.make_generics_args(metadata, ctx_collection);
        self.make_parents(metadata, ctx_collection, tdi);
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

    fn make_generics_args(&mut self, metadata: &Metadata, ctx_collection: &CppContextCollection) {
        let cpp_type = self.get_mut_cpp_type();

        if cpp_type.generic_instantiations_args_types.is_none() {
            return;
        }

        let generic_instantiations_args_types =
            cpp_type.generic_instantiations_args_types.clone().unwrap();

        let generic_instantiation_args: Vec<String> = generic_instantiations_args_types
            .iter()
            .map(|u| {
                metadata
                    .metadata_registration
                    .types
                    .get(*u as usize)
                    .unwrap()
            })
            .map(|t| cpp_type.cppify_name_il2cpp(ctx_collection, metadata, t, true))
            .collect();

        // Handle nested types
        // Assumes these nested types exist,
        // which are created in the make_generic type func
        // TODO: Base off a CppType the alias path
        {
            let aliases = cpp_type
                .nested_types
                .values()
                .filter(|c| c.cpp_template.is_some())
                .map(|n| {
                    let (literals, template) = match &n.cpp_template {
                        Some(template) => {
                            // Skip the first args as those aren't necessary
                            let extra_args = template
                                .names
                                .iter()
                                .skip(generic_instantiation_args.len())
                                .cloned()
                                .collect_vec();

                            let new_cpp_template = match !extra_args.is_empty() {
                                true => Some(CppTemplate { names: extra_args }),
                                false => None,
                            };

                            // Essentially, all nested types inherit their declaring type's generic params.
                            // Append the rest of the template params as generic parameters
                            match new_cpp_template {
                                Some(template) => (
                                    generic_instantiation_args
                                        .iter()
                                        .chain(&template.names)
                                        .cloned()
                                        .collect_vec(),
                                    Some(template),
                                ),
                                None => (generic_instantiation_args.clone(), None),
                            }
                        }
                        None => (generic_instantiation_args.clone(), None),
                    };

                    CppUsingAlias {
                        alias: n.cpp_name.clone(),
                        namespaze: None,
                        result: format!(
                            "{}::{}",
                            n.cpp_namespace,
                            STATIC_CONFIG.generic_nested_name(&n.cpp_full_name)
                        ),
                        template,
                        result_literals: literals,
                    }
                });

            aliases.for_each(|a| {
                cpp_type
                    .declarations
                    .insert(0, CppMember::CppUsingAlias(a).into())
            });
            // replaced by using statements
            // only for generic nested types
            cpp_type
                .nested_types
                .retain(|_, c| c.cpp_template.is_none());
        }

        cpp_type.generic_instantiation_args = Some(generic_instantiation_args);
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

        let cpp_name = cpp_type.cpp_name();
        // default ctor
        if t.is_value_type() {
            // let fields = t
            //     .fields(metadata.metadata)
            //     .iter()
            //     .filter_map(|field| {
            //         let f_type = metadata
            //             .metadata_registration
            //             .types
            //             .get(field.type_index as usize)
            //             .unwrap();

            //         if f_type.is_static() {
            //             return None;
            //         }

            //         let cpp_name =
            //             cpp_type.cppify_name_il2cpp(ctx_collection, metadata, f_type, false);

            //         Some(CppParam {
            //             name: field.name(metadata.metadata).to_string(),
            //             ty: cpp_name,
            //             modifiers: "".to_string(),
            //             def_value: Some("{}".to_string()),
            //         })
            //     })
            //     .collect_vec();
            // cpp_type.declarations.push(
            //     CppMember::ConstructorImpl(CppConstructorImpl {
            //         holder_cpp_ty_name: cpp_type.cpp_name().clone(),
            //         parameters: fields,
            //         is_constexpr: true,
            //         template: None,
            //     })
            //     .into(),
            // );

            cpp_type.declarations.push(
                CppMember::CppLine(CppLine {
                    line: format!(
                        "
                    constexpr {cpp_name}() = default;
                    constexpr {cpp_name}({cpp_name} const&) = default;
                    constexpr {cpp_name}({cpp_name}&&) = default;
                    constexpr {cpp_name}& operator=({cpp_name} const&) = default;
                    constexpr {cpp_name}& operator=({cpp_name}&&) noexcept = default;
                "
                    ),
                })
                .into(),
            );
        } else {
            cpp_type.declarations.push(
                CppMember::CppLine(CppLine {
                    line: format!("constexpr virtual ~{cpp_name}() = default;"),
                })
                .into(),
            );
        }

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
            for (i, method) in t.methods(metadata.metadata).iter().enumerate() {
                let method_index = MethodIndex::new(t.method_start.index() + i as u32);
                self.create_method(
                    method,
                    t,
                    method_index,
                    metadata,
                    ctx_collection,
                    config,
                    false,
                );
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
        for (i, field) in t.fields(metadata.metadata).iter().enumerate() {
            let f_type = metadata
                .metadata_registration
                .types
                .get(field.type_index as usize)
                .unwrap();

            // // use u32 here just to avoid casting issues
            // let pos_field_offset_offset: u32 =
            //     if t.is_value_type()&& !f_type.is_static() {
            //         // VALUE_TYPE_SIZE_OFFSET
            //         Self::layout_fields_locked_size(t, tdi, metadata)
            //     } else {
            //         0x0
            //     };

            let field_index = FieldIndex::new(t.field_start.index() + i as u32);
            let f_name = field.name(metadata.metadata);
            let f_offset = metadata
                .metadata_registration
                .field_offsets
                .as_ref()
                .unwrap()[tdi.index() as usize][i];
            //- pos_field_offset_offset;

            if let TypeData::TypeDefinitionIndex(tdi) = f_type.data && metadata.blacklisted_types.contains(&tdi) {
                if !cpp_type.is_value_type {
                    continue;
                }
                println!("Value type uses {tdi:?} which is blacklisted! TODO");
            }

            let field_ty_cpp_name =
                cpp_type.cppify_name_il2cpp(ctx_collection, metadata, f_type, false);

            // TODO: Check a flag to look for default values to speed this up
            let def_value = Self::field_default_value(metadata, field_index);

            let field_decl = CppFieldDecl {
                cpp_name: config.name_cpp(f_name),
                field_ty: field_ty_cpp_name,
                offset: f_offset,
                instance: !f_type.is_static() && !f_type.is_const(),
                readonly: f_type.is_const(),
                classof_call: cpp_type.classof_cpp_name(),
                literal_value: def_value,
                is_value_type: f_type.valuetype,
                declaring_is_reference: !t.is_value_type(),
            };

            cpp_type
                .declarations
                .push(CppMember::FieldDecl(field_decl.clone()).into());

            // TODO: improve how these are handled
            cpp_type.implementations.push(
                CppMember::FieldImpl(CppFieldImpl {
                    declaring_ty_cpp_full_name: cpp_type.cpp_full_name.clone(),
                    field_data: field_decl,
                })
                .into(),
            );
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
            if t.flags & TYPE_ATTRIBUTE_INTERFACE == 0 {
                println!("Skipping type: {ns}::{name} because it has parent index: {} and is not an interface!", t.parent_index);
            }
        } else if let Some(parent_type) = metadata
            .metadata_registration
            .types
            .get(t.parent_index as usize)
        {
            // We have a parent, lets do something with it
            let inherit_type =
                cpp_type.cppify_name_il2cpp(ctx_collection, metadata, parent_type, true);
            cpp_type.inherit.push(inherit_type);
        } else {
            panic!("NO PARENT! But valid index found: {}", t.parent_index);
        }

        for &interface_index in t.interfaces(metadata.metadata) {
            let int_ty = &metadata.metadata_registration.types[interface_index as usize];

            // We have an interface, lets do something with it
            let inherit_type = cpp_type.cppify_name_il2cpp(ctx_collection, metadata, int_ty, true);
            cpp_type.inherit.push(inherit_type);
        }
    }

    fn make_nested_types(
        &mut self,
        metadata: &Metadata,
        config: &GenerationConfig,
        tdi: TypeDefinitionIndex,
    ) {
        let cpp_type = self.get_mut_cpp_type();
        let t = &metadata.metadata.global_metadata.type_definitions[tdi];

        if t.nested_type_count == 0 {
            return;
        }

        let mut nested_types: Vec<CppType> = Vec::with_capacity(t.nested_type_count as usize);

        for &nested_type_index in t.nested_types(metadata.metadata) {
            let nt_ty = &metadata.metadata.global_metadata.type_definitions[nested_type_index];

            // We have a parent, lets do something with it
            let nested_type = CppType::make_cpp_type(
                metadata,
                config,
                CppTypeTag::TypeDefinitionIndex(nested_type_index),
                nested_type_index,
            );

            match nested_type {
                Some(unwrapped) => nested_types.push(unwrapped),
                None => println!("Failed to make nested CppType {nt_ty:?}"),
            };
        }

        cpp_type.nested_types = nested_types.into_iter().map(|t| (t.self_tag, t)).collect()
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

            let method_map = |p: MethodIndex| {
                let method_calc = metadata.method_calculations.get(&p).unwrap();
                CppMethodData {
                    estimated_size: method_calc.estimated_size,
                    addrs: method_calc.addrs,
                }
            };

            // Need to include this type
            cpp_type.declarations.push(
                CppMember::Property(CppProperty {
                    name: p_name.to_owned(),
                    cpp_name: config.name_cpp(p_name),
                    prop_ty: p_ty_cpp_name.clone(),
                    classof_call: cpp_type.classof_cpp_name(),
                    setter: p_setter.map(|_| method_map(prop.set_method_index(t))),
                    getter: p_getter.map(|_| method_map(prop.get_method_index(t))),
                    abstr: p_getter.is_some_and(|p| p.is_abstract_method())
                        || p_setter.is_some_and(|p| p.is_abstract_method()),
                    instance: !p_getter.or(p_setter).unwrap().is_static_method(),
                })
                .into(),
            );
        }
    }

    fn create_constructor(
        cpp_type: &mut CppType,
        m_params: &Vec<CppParam>,
        template: &Option<CppTemplate>,
    ) {
        cpp_type.implementations.push(
            CppMember::ConstructorImpl(CppConstructorImpl {
                holder_cpp_ty_name: cpp_type.formatted_complete_cpp_name().clone(),
                parameters: m_params.clone(),
                is_constexpr: false,
                template: template.clone(),
            })
            .into(),
        );
        cpp_type.declarations.push(
            CppMember::ConstructorDecl(CppConstructorDecl {
                cpp_name: cpp_type.cpp_name.clone(),
                parameters: m_params.clone(),
                template: template.clone(),
            })
            .into(),
        );
    }

    fn create_method(
        &mut self,
        method: &Il2CppMethodDefinition,
        declaring_type: &Il2CppTypeDefinition,
        method_index: MethodIndex,

        metadata: &Metadata,
        ctx_collection: &CppContextCollection,
        config: &GenerationConfig,
        is_generic_inst: bool,
    ) {
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

        let mut m_params: Vec<CppParam> = Vec::with_capacity(method.parameter_count as usize);

        for (pi, param) in method.parameters(metadata.metadata).iter().enumerate() {
            let param_index = ParameterIndex::new(method.parameter_start.index() + pi as u32);
            let param_type = metadata
                .metadata_registration
                .types
                .get(param.type_index as usize)
                .unwrap();

            let def_value = Self::param_default_value(metadata, param_index);
            let must_include = def_value.is_some();

            let make_param_cpp_type_name = |cpp_type: &mut CppType| {
                let name =
                    cpp_type.cppify_name_il2cpp(ctx_collection, metadata, param_type, must_include);
                cpp_type.il2cpp_byref(name, m_ret_type)
            };

            let param_cpp_name = match is_generic_inst {
                false => cpp_type.il2cpp_mparam_template_name(
                    metadata,
                    method_index,
                    make_param_cpp_type_name,
                    param_type,
                ),
                true => make_param_cpp_type_name(cpp_type),
            };

            m_params.push(CppParam {
                name: param.name(metadata.metadata).to_string(),
                def_value,
                ty: param_cpp_name,
                modifiers: "".to_string(),
            });
        }

        // TODO: Add template<typename ...> if a generic inst e.g
        // T UnityEngine.Component::GetComponent<T>() -> bs_hook::Il2CppWrapperType UnityEngine.Component::GetComponent()
        let template = if method.generic_container_index.is_valid() {
            match is_generic_inst {
                true => Some(CppTemplate { names: vec![] }),
                false => {
                    let generics = method
                        .generic_container(metadata.metadata)
                        .unwrap()
                        .generic_parameters(metadata.metadata)
                        .iter()
                        .map(|param| param.name(metadata.metadata).to_string())
                        .collect_vec();

                    Some(CppTemplate { names: generics })
                }
            }
        } else {
            None
        };

        let make_ret_cpp_type_name = |cpp_type: &mut CppType| {
            let name = cpp_type.cppify_name_il2cpp(ctx_collection, metadata, m_ret_type, false);
            cpp_type.il2cpp_byref(name, m_ret_type)
        };

        let m_ret_cpp_type_byref_name = match is_generic_inst {
            false => cpp_type.il2cpp_mparam_template_name(
                metadata,
                method_index,
                make_ret_cpp_type_name,
                m_ret_type,
            ),
            true => make_ret_cpp_type_name(cpp_type),
        };

        // Reference type constructor
        if m_name == ".ctor" && !declaring_type.is_value_type() {
            Self::create_constructor(cpp_type, &m_params, &template);
        }
        let declaring_type = method.declaring_type(metadata.metadata);
        let tag = CppTypeTag::TypeDefinitionIndex(method.declaring_type);

        let method_calc = metadata.method_calculations.get(&method_index);

        // generic methods don't have definitions if not an instantiation
        let stub = !is_generic_inst && template.is_some();

        // If a generic instantiation or not a template
        if !stub {
            cpp_type.implementations.push(
                CppMember::MethodImpl(CppMethodImpl {
                    cpp_method_name: config.name_cpp(m_name),
                    cs_method_name: m_name.to_string(),
                    holder_cpp_full_name: cpp_type.formatted_complete_cpp_name().to_string(),
                    return_type: m_ret_cpp_type_byref_name.clone(),
                    parameters: m_params.clone(),
                    instance: !method.is_static_method(),
                    suffix_modifiers: Default::default(),
                    prefix_modifiers: Default::default(),
                    template: template.clone(),
                })
                .into(),
            );
        }

        // if not a generic instantiation
        if !is_generic_inst {
            cpp_type.declarations.push(
                CppMember::MethodDecl(CppMethodDecl {
                    cpp_name: config.name_cpp(m_name),
                    return_type: m_ret_cpp_type_byref_name.clone(),
                    parameters: m_params.clone(),
                    template: template.clone(),
                    instance: !method.is_static_method(),
                    prefix_modifiers: Default::default(),
                    suffix_modifiers: Default::default(),
                    method_data: method_calc.map(|method_calc| CppMethodData {
                        addrs: method_calc.addrs,
                        estimated_size: method_calc.estimated_size,
                    }),
                    is_virtual: method.is_virtual_method() && !method.is_final_method(),
                })
                .into(),
            );
        }

        let declaring_cpp_type: Option<&CppType> = if tag == cpp_type.self_tag {
            Some(cpp_type)
        } else {
            ctx_collection.get_cpp_type(tag)
        };

        if let Some(method_calc) = method_calc && !stub {
            cpp_type
                .nonmember_implementations
                .push(Rc::new(CppMethodSizeStruct {
                    ret_ty: m_ret_cpp_type_byref_name,
                    cpp_method_name: config.name_cpp(m_name),
                    complete_type_name: cpp_type.formatted_complete_cpp_name().clone(),
                    instance: !method.is_static_method(),
                    params: m_params,
                    template,
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
    }

    fn default_value_blob(
        metadata: &Metadata,
        ty: Il2CppTypeEnum,
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

        match ty {
            Il2CppTypeEnum::Boolean => (if data[0] == 0 { "false" } else { "true" }).to_string(),
            Il2CppTypeEnum::I1 => cursor.read_i8().unwrap().to_string(),
            Il2CppTypeEnum::I2 => cursor.read_i16::<Endian>().unwrap().to_string(),
            Il2CppTypeEnum::I4 => {
                cursor.read_compressed_i32::<Endian>().unwrap().to_string()
            }
            // TODO: We assume 64 bit
            Il2CppTypeEnum::I | Il2CppTypeEnum::I8 => {
                cursor.read_i64::<Endian>().unwrap().to_string()
            }
            Il2CppTypeEnum::U1 => cursor.read_u8().unwrap().to_string(),
            Il2CppTypeEnum::U2 => cursor.read_u16::<Endian>().unwrap().to_string(),
            Il2CppTypeEnum::U4 => cursor.read_compressed_u32::<Endian>().unwrap().to_string(),
            // TODO: We assume 64 bit
            Il2CppTypeEnum::U | Il2CppTypeEnum::U8 => {
                cursor.read_u64::<Endian>().unwrap().to_string()
            }

            // https://learn.microsoft.com/en-us/nimbusml/concepts/types
            // https://en.cppreference.com/w/cpp/types/floating-point
            Il2CppTypeEnum::R4 => cursor.read_f32::<Endian>().unwrap().to_string(),
            Il2CppTypeEnum::R8 => cursor.read_f64::<Endian>().unwrap().to_string(),
            Il2CppTypeEnum::Char => {
                String::from_utf16_lossy(&[cursor.read_u16::<Endian>().unwrap()])
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
            Il2CppTypeEnum::Genericinst
            | Il2CppTypeEnum::Object
            | Il2CppTypeEnum::Class
            | Il2CppTypeEnum::Szarray => "nullptr".to_string(),

            _ => "unknown".to_string(),
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
                let ty = metadata
                    .metadata_registration
                    .types
                    .get(def.type_index as usize)
                    .unwrap();

                Self::default_value_blob(
                    metadata,
                    ty.ty,
                    def.data_index.index() as usize,
                    true,
                    true,
                )
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

                if !def.data_index.is_valid() {
                    return "nullptr".to_string();
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

                Self::default_value_blob(
                    metadata,
                    ty.ty,
                    def.data_index.index() as usize,
                    true,
                    true,
                )
            })
    }

    fn il2cpp_byref(&mut self, cpp_name: String, typ: &Il2CppType) -> String {
        let requirements = &mut self.get_mut_cpp_type().requirements;
        if typ.is_param_out() {
            requirements.needs_byref_include();
            return format!("ByRef<{cpp_name}>");
        }

        if typ.is_param_in() {
            requirements.needs_byref_include();

            return format!("ByRefConst<{cpp_name}>");
        }

        cpp_name
    }

    fn il2cpp_mparam_template_name<'a>(
        &mut self,
        metadata: &'a Metadata,
        method_index: MethodIndex,
        cpp_name: impl FnOnce(&mut CppType) -> String,
        typ: &'a Il2CppType,
    ) -> String {
        let tys = self
            .get_mut_cpp_type()
            .method_generic_instantiation_map
            .remove(&method_index);

        let ret = match typ.ty {
            Il2CppTypeEnum::Mvar => match typ.data {
                TypeData::GenericParameterIndex(index) => {
                    let generic_param: &brocolib::global_metadata::Il2CppGenericParameter =
                        &metadata.metadata.global_metadata.generic_parameters[index];

                    let owner = generic_param.owner(metadata.metadata);
                    assert!(owner.is_method != u32::MAX);

                    let gen_param = owner
                        .generic_parameters(metadata.metadata)
                        .iter()
                        .find(|&p| p.name_index == generic_param.name_index)
                        .unwrap();

                    gen_param.name(metadata.metadata).to_string()
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
        let mut nested_types: HashMap<CppTypeTag, String> = cpp_type
            .nested_types_flattened()
            .into_iter()
            .map(|(t, c)| (t, c.formatted_complete_cpp_name().clone()))
            .collect();

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

                // Skip nested classes
                if let Some(nested) = nested_types.remove(&typ_cpp_tag) {
                    return nested;
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

                let parent_context_ty = ctx_collection.get_context_root_tag(typ_cpp_tag);
                let cpp_type_context_ty = ctx_collection.get_context_root_tag(cpp_type.self_tag);

                let inc = CppInclude::new_context_typedef(to_incl);
                let to_incl_ty = ctx_collection
                    .get_cpp_type(typ.data.into())
                    .unwrap_or_else(|| panic!("Unable to get type to include {:?}", typ.data));

                let own_context = parent_context_ty == cpp_type_context_ty;

                // - Include it
                // Skip including the context if we're already in it
                if add_include && !own_context {
                    requirements.required_includes.insert(inc.clone());
                }

                // Forward declare it
                if !add_include && !own_context {
                    if to_incl_ty.nested {
                        // TODO: What should we do here?
                        eprintln!("Can't forward declare nested type! Including!");
                        requirements.required_includes.insert(inc);
                    } else {
                        requirements
                            .forward_declares
                            .insert((CppForwardDeclare::from_cpp_type(to_incl_ty), inc));
                    }
                }

                to_incl_ty.formatted_complete_cpp_name().clone()
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
                        return format!("{}", gen_param.name(metadata.metadata));
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

                    if cpp_type.generic_instantiations_args_types.is_none() {
                        return format!("/* TODO: FIX THIS, THIS SHOULDN'T HAPPEN! NO GENERIC INST ARGS FOUND HERE */ {}", generic_param.name(metadata.metadata));
                    }

                    let ty_idx = cpp_type.generic_instantiations_args_types.as_ref().unwrap()
                        [generic_param.num as usize];

                    let ty = metadata
                        .metadata_registration
                        .types
                        .get(ty_idx as usize)
                        .unwrap();
                    self.cppify_name_il2cpp(ctx_collection, metadata, ty, add_include)
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
            // TODO: We assume 64 bit
            Il2CppTypeEnum::I | Il2CppTypeEnum::I8 => "int64_t".to_string(),
            Il2CppTypeEnum::U1 => "uint8_t".to_string(),
            Il2CppTypeEnum::U2 => "uint16_t".to_string(),
            Il2CppTypeEnum::U4 => "uint32_t".to_string(),
            // TODO: We assume 64 bit
            Il2CppTypeEnum::U | Il2CppTypeEnum::U8 => "uint64_t".to_string(),

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

    fn layout_fields_locked_size<'a>(
        ty_def: &'a Il2CppTypeDefinition,
        tdi: TypeDefinitionIndex,
        metadata: &'a Metadata,
    ) -> u32 {
        // TODO:
        const SIZEOF_IL2CPP_OBJECT: u32 = 0x10;
        const IL2CPP_SIZEOF_STRUCT_WITH_NO_INSTANCE_FIELDS: u32 = 1;

        let mut instance_size: u32 = if ty_def.parent_index != u32::MAX {
            SIZEOF_IL2CPP_OBJECT
        } else {
            let parent_ty = &metadata.metadata_registration.types[ty_def.parent_index as usize];
            let TypeData::TypeDefinitionIndex(parent_tdi) = parent_ty.data else {
                todo!()
            };
            let parent_ty_def = &metadata.metadata.global_metadata.type_definitions[parent_tdi];

            Self::layout_fields_locked_size(parent_ty_def, parent_tdi, metadata)
        };

        if ty_def.field_count > 0 {
            let size_table = Self::get_size_of_type_table(metadata, tdi);

            if ty_def.is_value_type()
                && size_table.map(|t| t.instance_size).unwrap_or(0) == 0
                // if no field is instance
                && !ty_def.fields(metadata.metadata).iter().any(|f| {
                    // if instance
                    !metadata.metadata_registration.types[f.type_index as usize].is_static()
                })
            {
                instance_size = IL2CPP_SIZEOF_STRUCT_WITH_NO_INSTANCE_FIELDS + SIZEOF_IL2CPP_OBJECT;
            }

            instance_size =
                Self::update_instance_size_for_generic_class(ty_def, tdi, instance_size, metadata);
        } else {
            // need to set this in case there are no fields in a generic instance type
            instance_size =
                Self::update_instance_size_for_generic_class(ty_def, tdi, instance_size, metadata);
        }

        instance_size
    }

    fn update_instance_size_for_generic_class(
        ty_def: &Il2CppTypeDefinition,
        tdi: TypeDefinitionIndex,
        instance_size: u32,
        metadata: &Metadata<'_>,
    ) -> u32 {
        // need to set this in case there are no fields in a generic instance type
        if !ty_def.generic_container_index.is_valid() {
            return instance_size;
        }
        let size = Self::get_size_of_type_table(metadata, tdi)
            .map(|s| s.instance_size)
            .unwrap_or(0);

        // If the generic class has an instance size, it was explictly set
        if size > 0 && size > instance_size {
            return size;
        }

        instance_size
    }

    fn get_size_of_type_table<'a>(
        metadata: &'a Metadata<'a>,
        tdi: TypeDefinitionIndex,
    ) -> Option<&'a Il2CppTypeDefinitionSizes> {
        if let Some(size_table) = &metadata
            .metadata
            .runtime_metadata
            .metadata_registration
            .type_definition_sizes
        {
            size_table.get(tdi.index() as usize)
        } else {
            None
        }
    }
}

impl CSType for CppType {
    fn get_mut_cpp_type(&mut self) -> &mut CppType {
        self
    }

    fn get_cpp_type(&self) -> &CppType {
        self
    }
}
