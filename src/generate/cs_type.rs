use std::{
    collections::HashMap,
    io::{Cursor, Read},
    rc::Rc,
};

use byteorder::{LittleEndian, ReadBytesExt};
use il2cpp_binary::{Type, TypeData, TypeEnum};
use il2cpp_metadata_raw::{Il2CppGenericParameter, TypeDefinitionIndex};
use itertools::Itertools;

use super::{
    config::GenerationConfig,
    constants::{
        MethodDefintionExtensions, TypeDefinitionExtensions, TypeExtentions,
        TYPE_ATTRIBUTE_INTERFACE,
    },
    context::{CppContextCollection, TypeTag},
    cpp_type::CppType,
    members::{
        CppCommentedString, CppConstructorDecl, CppConstructorImpl, CppField, CppForwardDeclare,
        CppInclude, CppMember, CppMethodData, CppMethodDecl, CppMethodImpl, CppMethodSizeStruct,
        CppParam, CppProperty, CppTemplate,
    },
    metadata::Metadata,
};

type Endian = LittleEndian;

pub trait CSType: Sized {
    fn get_mut_cpp_type(&mut self) -> &mut CppType; // idk how else to do this
    fn get_cpp_type(&self) -> &CppType; // idk how else to do this

    fn get_tag_tdi(tag: impl Into<TypeTag>) -> u32 {
        let tag_convert = tag.into();
        match tag_convert {
            TypeTag::TypeDefinition(tdi) => tdi,
            _ => panic!("Unsupported type: {tag_convert:?}"),
        }
    }

    fn parent_joined_cpp_name(
        metadata: &Metadata,
        config: &GenerationConfig,
        tdi: TypeDefinitionIndex,
    ) -> String {
        let parent = metadata.child_to_parent_map.get(&tdi);
        let ty = metadata
            .metadata
            .type_definitions
            .get(tdi as usize)
            .unwrap();
        let self_name = metadata.metadata.get_str(ty.name_index).unwrap();

        match parent {
            Some(parent_ty_cpp_name) => {
                let parent_name =
                    Self::parent_joined_cpp_name(metadata, config, parent_ty_cpp_name.tdi);

                format!("{parent_name}::{self_name}")
            }
            None => config.name_cpp(self_name),
        }
    }

    fn make_cpp_type(
        metadata: &Metadata,
        config: &GenerationConfig,
        tag: impl Into<TypeTag>,
    ) -> Option<CppType> {
        // let iface = metadata.interfaces.get(t.interfaces_start);
        // Then, handle interfaces

        // Then, handle methods
        // - This includes constructors
        // inherited methods will be inherited

        let tag_copy = tag.into();
        let tdi = Self::get_tag_tdi(tag_copy);
        let parent_pair = metadata.child_to_parent_map.get(&tdi);

        let t = metadata
            .metadata
            .type_definitions
            .get(tdi as usize)
            .unwrap();

        // Generics
        let generics = metadata
            .metadata
            .generic_containers
            .get(t.generic_container_index as usize)
            .map(|container| {
                let mut generics: Vec<(&Il2CppGenericParameter, Vec<u32>)> =
                    Vec::with_capacity(container.type_argc as usize);

                for i in 0..container.type_argc {
                    let generic_param_index = i + container.generic_parameter_start;
                    let generic_param = metadata
                        .metadata
                        .generic_parameters
                        .get(generic_param_index as usize)
                        .unwrap();

                    let mut generic_constraints: Vec<u32> =
                        Vec::with_capacity(generic_param.constraints_count as usize);
                    for j in 0..generic_param.constraints_count {
                        let generic_constraint_index = j + generic_param.constraints_start;
                        let generic_constraint = metadata
                            .metadata
                            .generic_parameter_constraints
                            .get(generic_constraint_index as usize)
                            .unwrap();

                        // TODO: figure out
                        generic_constraints.push(*generic_constraint);
                    }

                    generics.push((generic_param, generic_constraints));
                }
                generics
            });

        let cpp_template = CppTemplate {
            names: generics
                .unwrap_or_default()
                .iter()
                .map(|(g, _)| metadata.metadata.get_str(g.name_index).unwrap().to_string())
                .collect(),
        };

        let ns = metadata.metadata.get_str(t.namespace_index).unwrap();
        let name = metadata.metadata.get_str(t.name_index).unwrap();
        let mut cpptype = CppType {
            self_tag: tag_copy,
            nested: parent_pair.is_some(),
            prefix_comments: vec![format!("Type: {ns}::{name}")],
            namespace: config.namespace_cpp(ns),
            cpp_namespace: config.namespace_cpp(ns),
            name: config.name_cpp(name),
            cpp_name: config.name_cpp(name),
            parent_ty_tdi: parent_pair.map(|p| p.tdi),
            parent_ty_cpp_name: parent_pair
                .map(|p| Self::parent_joined_cpp_name(metadata, config, p.tdi)),

            declarations: Default::default(),
            implementations: Default::default(),
            nonmember_implementations: Default::default(),
            nonmember_declarations: Default::default(),
            is_value_type: t.is_value_type(),
            requirements: Default::default(),
            inherit: Default::default(),
            generic_args: cpp_template,
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

        Some(cpptype)
    }

    fn fill_from_il2cpp(
        &mut self,
        metadata: &Metadata,
        config: &GenerationConfig,
        ctx_collection: &CppContextCollection,
        tdi: TypeDefinitionIndex,
    ) {
        self.make_parents(metadata, config, ctx_collection, tdi);
        self.make_fields(metadata, config, ctx_collection, tdi);
        self.make_properties(metadata, config, ctx_collection, tdi);
        self.make_methods(metadata, config, ctx_collection, tdi);
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

        // default ctor
        if t.is_value_type() {
            let mut fields: Vec<CppParam> = Vec::with_capacity(t.field_count as usize);
            for field_index in t.field_start..t.field_start + t.field_count as u32 {
                let field = metadata.metadata.fields.get(field_index as usize).unwrap();
                let f_type = metadata
                    .metadata_registration
                    .types
                    .get(field.type_index as usize)
                    .unwrap();

                let cpp_name =
                    cpp_type.cppify_name_il2cpp(ctx_collection, metadata, config, f_type, false);

                fields.push(CppParam {
                    name: metadata
                        .metadata
                        .get_str(field.name_index)
                        .unwrap()
                        .to_owned(),
                    ty: cpp_name,
                    modifiers: "".to_string(),
                    def_value: Some("{}".to_string()),
                });
            }
            cpp_type
                .declarations
                .push(CppMember::ConstructorImpl(CppConstructorImpl {
                    holder_cpp_ty_name: cpp_type.cpp_name().clone(),
                    parameters: fields.clone(),
                    is_constexpr: true,
                    template: CppTemplate::default(),
                }));
        }

        // Then, handle methods
        if t.method_count > 0 {
            // Write comment for methods
            cpp_type
                .declarations
                .push(CppMember::Comment(CppCommentedString {
                    data: "".to_string(),
                    comment: Some("Methods".to_string()),
                }));
            // Then, for each method, write it out
            for i in 0..t.method_count {
                let method = metadata
                    .metadata
                    .methods
                    .get((t.method_start + i as u32) as usize)
                    .unwrap();
                let m_name = metadata.metadata.get_str(method.name_index).unwrap();

                // Skip class/static constructor
                // if method.is_special_name()
                // && !(m_name.starts_with("get") || m_name.starts_with("set") || m_name == (".ctor"))
                if m_name == ".cctor" {
                    // println!("Skipping {}", m_name);
                    continue;
                }
                let m_ret_type = metadata
                    .metadata_registration
                    .types
                    .get(method.return_type as usize)
                    .unwrap();
                let mut m_params: Vec<CppParam> =
                    Vec::with_capacity(method.parameter_count as usize);

                for p in 0..method.parameter_count {
                    let param_index = (method.parameter_start + p as u32) as usize;
                    let param = metadata.metadata.parameters.get(param_index).unwrap();

                    let param_type = metadata
                        .metadata_registration
                        .types
                        .get(param.type_index as usize)
                        .unwrap();

                    let param_cpp_name = cpp_type.cppify_name_il2cpp(
                        ctx_collection,
                        metadata,
                        config,
                        param_type,
                        false,
                    );

                    let def_value = Self::param_default_value(metadata, param_index as u32);

                    m_params.push(CppParam {
                        name: metadata
                            .metadata
                            .get_str(param.name_index as u32)
                            .unwrap()
                            .to_string(),
                        def_value,
                        ty: param_cpp_name,
                        modifiers: if param_type.is_byref() {
                            String::from("byref")
                        } else {
                            String::from("")
                        },
                    });
                }

                let generic_container = metadata
                    .metadata
                    .generic_containers
                    .get(method.generic_container_index as usize);

                let mut generics: Vec<String> = vec![];

                if let Some(generic_container) = generic_container {
                    generics = Vec::with_capacity(generic_container.type_argc as usize);

                    for param_index in generic_container.generic_parameter_start
                        ..generic_container.generic_parameter_start + generic_container.type_argc
                    {
                        let param = metadata
                            .metadata
                            .generic_parameters
                            .get(param_index as usize)
                            .unwrap();

                        let param_str = metadata.metadata.get_str(param.name_index).unwrap();
                        generics.push(param_str.to_string());
                    }
                }

                let template = CppTemplate { names: generics };

                // Need to include this type
                let m_ret_cpp_type_name = cpp_type.cppify_name_il2cpp(
                    ctx_collection,
                    metadata,
                    config,
                    m_ret_type,
                    false,
                );

                let method_calc = metadata
                    .method_calculations
                    .get(&(t.method_start + i as u32))
                    .unwrap();

                if m_name == ".ctor" && !t.is_value_type() {
                    cpp_type
                        .implementations
                        .push(CppMember::ConstructorImpl(CppConstructorImpl {
                            holder_cpp_ty_name: cpp_type.cpp_name().clone(),
                            parameters: m_params.clone(),
                            is_constexpr: false,
                            template: template.clone(),
                        }));
                    cpp_type
                        .declarations
                        .push(CppMember::ConstructorDecl(CppConstructorDecl {
                            ty: cpp_type.formatted_complete_cpp_name(),
                            parameters: m_params.clone(),
                            template: template.clone(),
                        }));
                }

                let declaring_type = metadata
                    .metadata
                    .type_definitions
                    .get(method.declaring_type as usize)
                    .unwrap();
                let tag = TypeTag::TypeDefinition(method.declaring_type);
                let declaring_cpp_type: Option<&CppType> = if tag == cpp_type.self_tag {
                    Some(cpp_type)
                } else {
                    ctx_collection
                        .get_cpp_type(tag)
                        .map(|c| &*c) // &mut -> &
                };

                cpp_type
                    .nonmember_implementations
                    .push(Rc::new(CppMethodSizeStruct {
                        ret_ty: m_ret_cpp_type_name.clone(),
                        cpp_method_name: config.name_cpp(m_name),
                        complete_type_name: cpp_type.formatted_complete_cpp_name(),
                        instance: !method.is_static_method(),
                        params: m_params.clone(),
                        template: template.clone(),
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
                cpp_type
                    .implementations
                    .push(CppMember::MethodImpl(CppMethodImpl {
                        cpp_method_name: config.name_cpp(m_name),
                        cs_method_name: m_name.to_string(),
                        holder_cpp_namespaze: cpp_type.cpp_namespace().to_string(),
                        holder_cpp_name: match &cpp_type.parent_ty_cpp_name {
                            Some(p) => format!("{p}::{}", cpp_type.cpp_name().clone()),
                            None => cpp_type.cpp_name().clone(),
                        },
                        return_type: m_ret_cpp_type_name.clone(),
                        parameters: m_params.clone(),
                        instance: !method.is_static_method(),
                        suffix_modifiers: Default::default(),
                        prefix_modifiers: Default::default(),
                        template: template.clone(),
                    }));
                cpp_type
                    .declarations
                    .push(CppMember::MethodDecl(CppMethodDecl {
                        cpp_name: config.name_cpp(m_name),
                        return_type: m_ret_cpp_type_name,
                        parameters: m_params,
                        instance: !method.is_static_method(),
                        prefix_modifiers: Default::default(),
                        suffix_modifiers: Default::default(),
                        method_data: CppMethodData {
                            addrs: method_calc.addrs,
                            estimated_size: method_calc.estimated_size,
                        },
                        is_virtual: method.is_virtual_method() && !method.is_final_method(),
                        template,
                    }));
            }
        }
    }

    fn make_fields(
        &mut self,
        metadata: &Metadata,
        config: &GenerationConfig,
        ctx_collection: &CppContextCollection,
        tdi: TypeDefinitionIndex,
    ) {
        let cpp_type = self.get_mut_cpp_type();
        let t = Self::get_type_definition(metadata, tdi);

        // Then, handle fields
        if t.field_count == 0 {
            return;
        }
        // Write comment for fields
        cpp_type
            .declarations
            .push(CppMember::Comment(CppCommentedString {
                data: "".to_string(),
                comment: Some("Fields".to_string()),
            }));
        // Then, for each field, write it out
        for i in 0..t.field_count {
            let field_index = (t.field_start + i as u32) as usize;
            let field = metadata.metadata.fields.get(field_index).unwrap();
            let f_name = metadata.metadata.get_str(field.name_index).unwrap();
            let f_offset = metadata
                .metadata_registration
                .field_offsets
                .get(tdi as usize)
                .unwrap()
                .get(i as usize)
                .unwrap();
            let f_type = metadata
                .metadata_registration
                .types
                .get(field.type_index as usize)
                .unwrap();

            let _f_type_data = TypeTag::from(f_type.data);

            let cpp_name =
                cpp_type.cppify_name_il2cpp(ctx_collection, metadata, config, f_type, false);

            let def_value = Self::field_default_value(metadata, field_index as u32);

            // Need to include this type
            cpp_type.declarations.push(CppMember::Field(CppField {
                name: f_name.to_owned(),
                ty: cpp_name,
                offset: *f_offset,
                instance: !f_type.is_static() && !f_type.is_const(),
                readonly: f_type.is_const(),
                classof_call: cpp_type.classof_cpp_name(),
                literal_value: def_value,
                use_wrapper: !t.is_value_type(),
            }));
        }
    }

    fn make_parents(
        &mut self,
        metadata: &Metadata,
        config: &GenerationConfig,
        ctx_collection: &CppContextCollection,
        tdi: TypeDefinitionIndex,
    ) {
        let cpp_type = self.get_mut_cpp_type();
        let t = metadata
            .metadata
            .type_definitions
            .get(tdi as usize)
            .unwrap();
        let ns = metadata.metadata.get_str(t.namespace_index).unwrap();
        let name = metadata.metadata.get_str(t.name_index).unwrap();

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
                cpp_type.cppify_name_il2cpp(ctx_collection, metadata, config, parent_type, true);
            cpp_type.inherit.push(inherit_type);
        } else {
            panic!("NO PARENT! But valid index found: {}", t.parent_index);
        }

        for interface_index in t.interfaces_start..t.interfaces_start + (t.interfaces_count as u32)
        {
            let int_ty = metadata
                .metadata_registration
                .types
                .get(interface_index as usize)
                .unwrap();

            // We have a parent, lets do something with it
            let inherit_type =
                cpp_type.cppify_name_il2cpp(ctx_collection, metadata, config, int_ty, true);
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
        let t = metadata
            .metadata
            .type_definitions
            .get(tdi as usize)
            .unwrap();

        if t.nested_type_count == 0 {
            return;
        }

        let mut nested_types: Vec<CppType> = Vec::with_capacity(t.nested_type_count as usize);

        for nested_type_index in
            t.nested_types_start..t.nested_types_start + (t.nested_type_count as u32)
        {
            let nt_tdi = *metadata
                .metadata
                .nested_types
                .get(nested_type_index as usize)
                .unwrap();
            let nt_ty = metadata
                .metadata
                .type_definitions
                .get(nt_tdi as usize)
                .unwrap();

            // We have a parent, lets do something with it
            let nested_type =
                CppType::make_cpp_type(metadata, config, TypeTag::TypeDefinition(nt_tdi));

            match nested_type {
                Some(unwrapped) => nested_types.push(unwrapped),
                None => println!("Failed to make nested CppType {nt_ty:?}"),
            };
        }

        cpp_type.nested_types = nested_types
    }

    fn make_properties(
        &mut self,
        metadata: &Metadata,
        config: &GenerationConfig,
        ctx_collection: &CppContextCollection,
        tdi: u32,
    ) {
        let cpp_type = self.get_mut_cpp_type();
        let t = Self::get_type_definition(metadata, tdi);

        // Then, handle properties
        if t.property_count == 0 {
            return;
        }
        // Write comment for properties
        cpp_type
            .declarations
            .push(CppMember::Comment(CppCommentedString {
                data: "".to_string(),
                comment: Some("Properties".to_string()),
            }));
        // Then, for each field, write it out
        for i in 0..t.property_count {
            let prop = metadata
                .metadata
                .properties
                .get((t.property_start + i as u32) as usize)
                .unwrap();
            let p_name = metadata.metadata.get_str(prop.name_index).unwrap();
            let p_setter = if prop.set != u32::MAX {
                metadata
                    .metadata
                    .methods
                    .get((t.method_start + prop.set) as usize)
            } else {
                None
            };

            let p_getter = if prop.get != u32::MAX {
                metadata
                    .metadata
                    .methods
                    .get((t.method_start + prop.get) as usize)
            } else {
                None
            };

            let p_type_index = match p_getter {
                Some(g) => g.return_type as usize,
                None => {
                    metadata
                        .metadata
                        .parameters
                        .get(p_setter.unwrap().parameter_start as usize)
                        .unwrap()
                        .type_index as usize
                }
            };

            let p_type = metadata
                .metadata_registration
                .types
                .get(p_type_index)
                .unwrap();

            let p_cpp_name =
                cpp_type.cppify_name_il2cpp(ctx_collection, metadata, config, p_type, false);

            let method_map = |p: u32| {
                let method_calc = metadata.method_calculations.get(&p).unwrap();
                CppMethodData {
                    estimated_size: method_calc.estimated_size,
                    addrs: method_calc.addrs,
                }
            };

            // Need to include this type
            cpp_type.declarations.push(CppMember::Property(CppProperty {
                name: p_name.to_owned(),
                ty: p_cpp_name.clone(),
                classof_call: cpp_type.classof_cpp_name(),
                setter: p_setter.map(|_| method_map(prop.set)),
                getter: p_getter.map(|_| method_map(prop.get)),
                abstr: p_getter.or(p_setter).unwrap().is_abstract_method(),
                instance: !p_getter.or(p_setter).unwrap().is_static_method(),
            }));
        }
    }

    fn default_value_blob(metadata: &Metadata, ty: TypeEnum, data_index: usize) -> String {
        let data = &metadata.metadata.field_and_parameter_default_value_data[data_index..];

        let mut cursor = Cursor::new(data);

        match ty {
            TypeEnum::Boolean => (if data[0] == 0 { "false" } else { "true" }).to_string(),
            TypeEnum::I1 => cursor.read_i8().unwrap().to_string(),
            TypeEnum::I2 => cursor.read_i16::<Endian>().unwrap().to_string(),
            TypeEnum::Valuetype | TypeEnum::I4 => cursor.read_i32::<Endian>().unwrap().to_string(),
            // TODO: We assume 64 bit
            TypeEnum::I | TypeEnum::I8 => cursor.read_i64::<Endian>().unwrap().to_string(),
            TypeEnum::U1 => cursor.read_u8().unwrap().to_string(),
            TypeEnum::U2 => cursor.read_u16::<Endian>().unwrap().to_string(),
            TypeEnum::U4 => cursor.read_u32::<Endian>().unwrap().to_string(),
            // TODO: We assume 64 bit
            TypeEnum::U | TypeEnum::U8 => cursor.read_u64::<Endian>().unwrap().to_string(),

            // https://learn.microsoft.com/en-us/nimbusml/concepts/types
            // https://en.cppreference.com/w/cpp/types/floating-point
            TypeEnum::R4 => cursor.read_f32::<Endian>().unwrap().to_string(),
            TypeEnum::R8 => cursor.read_f64::<Endian>().unwrap().to_string(),
            TypeEnum::Char => String::from_utf16_lossy(&[cursor.read_u16::<Endian>().unwrap()]),
            TypeEnum::String => {
                let size = cursor.read_u32::<Endian>().unwrap();
                let mut str = String::with_capacity(size as usize);
                unsafe {
                    cursor.read_exact(str.as_bytes_mut()).unwrap();
                }
                str
            }
            TypeEnum::Genericinst | TypeEnum::Object | TypeEnum::Class | TypeEnum::Szarray => {
                "nullptr".to_string()
            }

            _ => "unknown".to_string(),
        }
    }

    fn field_default_value(metadata: &Metadata, field_index: u32) -> Option<String> {
        metadata
            .metadata
            .field_default_values
            .iter()
            .find(|f| f.field_index == field_index)
            .map(|def| {
                let ty = metadata
                    .metadata_registration
                    .types
                    .get(def.type_index as usize)
                    .unwrap();

                Self::default_value_blob(metadata, ty.ty, def.data_index as usize)
            })
    }
    fn param_default_value(metadata: &Metadata, parameter_index: u32) -> Option<String> {
        metadata
            .metadata
            .parameter_default_values
            .iter()
            .find(|p| p.parameter_index == parameter_index)
            .map(|def| {
                let mut ty = metadata
                    .metadata_registration
                    .types
                    .get(def.type_index as usize)
                    .unwrap();

                if def.data_index as i64 == -1 || def.data_index == u32::MAX {
                    return "nullptr".to_string();
                }

                if let TypeEnum::Valuetype = ty.ty {
                    match ty.data {
                        TypeData::TypeDefinitionIndex(tdi) => {
                            let type_def = metadata
                                .metadata
                                .type_definitions
                                .get(tdi as usize)
                                .unwrap();

                            // System.Nullable`1
                            if metadata.metadata.get_str(type_def.name_index).unwrap()
                                == "Nullable`1"
                                && metadata.metadata.get_str(type_def.namespace_index).unwrap()
                                    == "System"
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

                Self::default_value_blob(metadata, ty.ty, def.data_index as usize)
            })
    }

    fn cppify_name_il2cpp(
        &mut self,
        ctx_collection: &CppContextCollection,
        metadata: &Metadata,
        config: &GenerationConfig,
        typ: &Type,
        add_include: bool,
    ) -> String {
        let tag = TypeTag::from(typ.data);

        let _context_tag = ctx_collection.get_context_root_tag(tag);
        let cpp_type = self.get_mut_cpp_type();
        let mut nested_types: HashMap<TypeTag, String> = cpp_type
            .nested_types_flattened()
            .into_iter()
            .map(|(t, c)| (t, c.formatted_complete_cpp_name()))
            .collect();

        let requirements = &mut cpp_type.requirements;
        match typ.ty {
            TypeEnum::I1
            | TypeEnum::U1
            | TypeEnum::I2
            | TypeEnum::U2
            | TypeEnum::I4
            | TypeEnum::U4
            | TypeEnum::I8
            | TypeEnum::U8
            | TypeEnum::I
            | TypeEnum::U
            | TypeEnum::R4
            | TypeEnum::R8 => {
                requirements.needs_int_include();
            }
            _ => (),
        };

        match typ.ty {
            TypeEnum::Object => {
                requirements.need_wrapper();
                "::bs_hook::Il2CppWrapperType".to_string()
            }
            TypeEnum::Valuetype | TypeEnum::Class => {
                // Self
                if tag == cpp_type.self_tag {
                    // TODO: println!("Warning! This is self referencing, handle this better in the future");
                    return cpp_type.formatted_complete_cpp_name();
                }

                // Skip nested classes
                if let Some(nested) = nested_types.remove(&tag) {
                    return nested;
                }

                // In this case, just inherit the type
                // But we have to:
                // - Determine where to include it from
                let to_incl = ctx_collection
                    .get_context(typ.data)
                    .unwrap_or_else(|| panic!("no context for type {typ:?}"));

                // - Include it
                if add_include {
                    requirements
                        .required_includes
                        .insert(CppInclude::new_context(to_incl));
                }
                let inc = CppInclude::new_context(to_incl);
                let to_incl_ty = ctx_collection
                    .get_cpp_type(typ.data)
                    .unwrap_or_else(|| panic!("Unable to get type to include {:?}", typ.data));

                // Forward declare it
                if !add_include {
                    requirements
                        .forward_declares
                        .insert((CppForwardDeclare::from_cpp_type(to_incl_ty), inc));
                }

                to_incl_ty.formatted_complete_cpp_name()
            }
            // TODO: MVAR and VAR
            TypeEnum::Szarray => {
                requirements.needs_arrayw_include();

                let generic: String = match typ.data.into() {
                    TypeTag::Type(e) => {
                        let ty = metadata.metadata_registration.types.get(e).unwrap();
                        self.cppify_name_il2cpp(ctx_collection, metadata, config, ty, false)
                    }

                    _ => panic!("Unknown type data for array {typ:?}!"),
                };

                format!("::ArrayW<{generic}>")
            }
            TypeEnum::Mvar | TypeEnum::Var => match typ.data {
                // TODO: Alias to actual generic
                TypeData::GenericParameterIndex(index) => {
                    let generic_param = metadata
                        .metadata
                        .generic_parameters
                        .get(index as usize)
                        .unwrap();

                    let name = metadata.metadata.get_str(generic_param.name_index).unwrap();

                    name.to_string()
                }
                _ => todo!(),
            },
            TypeEnum::Genericinst => match typ.data.into() {
                TypeTag::GenericClass(e) => {
                    let generic_class = metadata
                        .metadata_registration
                        .generic_classes
                        .get(e)
                        .unwrap();
                    let generic_inst = metadata
                        .metadata_registration
                        .generic_insts
                        .get(generic_class.context.class_inst_idx.unwrap())
                        .unwrap();

                    let types = generic_inst
                        .types
                        .iter()
                        .map(|t| metadata.metadata_registration.types.get(*t).unwrap())
                        .map(|t| {
                            self.cppify_name_il2cpp(ctx_collection, metadata, config, t, false)
                        });

                    let generic_types = types.collect_vec();

                    let generic_type_def = metadata
                        .metadata
                        .type_definitions
                        .get(generic_class.type_definition_index as usize)
                        .unwrap();

                    let generic_type = metadata
                        .metadata_registration
                        .types
                        .get(generic_type_def.byval_type_index as usize)
                        .unwrap();
                    let owner_name = self.cppify_name_il2cpp(
                        ctx_collection,
                        metadata,
                        config,
                        generic_type,
                        false,
                    );

                    format!("{owner_name}<{}>", generic_types.join(","))
                }

                _ => panic!("Unknown type data for generic inst {typ:?}!"),
            },
            TypeEnum::I1 => "int8_t".to_string(),
            TypeEnum::I2 => "int16_t".to_string(),
            TypeEnum::I4 => "int32_t".to_string(),
            // TODO: We assume 64 bit
            TypeEnum::I | TypeEnum::I8 => "int64_t".to_string(),
            TypeEnum::U1 => "uint8_t".to_string(),
            TypeEnum::U2 => "uint16_t".to_string(),
            TypeEnum::U4 => "uint32_t".to_string(),
            // TODO: We assume 64 bit
            TypeEnum::U | TypeEnum::U8 => "uint64_t".to_string(),

            // https://learn.microsoft.com/en-us/nimbusml/concepts/types
            // https://en.cppreference.com/w/cpp/types/floating-point
            TypeEnum::R4 => "float32_t".to_string(),
            TypeEnum::R8 => "float64_t".to_string(),

            TypeEnum::Void => "void".to_string(),
            TypeEnum::Boolean => "bool".to_string(),
            TypeEnum::Char => "char16_t".to_string(),
            TypeEnum::String => {
                requirements.needs_stringw_include();
                "::StringW".to_string()
            }
            TypeEnum::Ptr => "void*".to_owned(),
            // TODO: Void and the other primitives
            _ => format!("/* UNKNOWN TYPE! {typ:?} */"),
        }
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
    ) -> &'a il2cpp_metadata_raw::Il2CppTypeDefinition {
        metadata
            .metadata
            .type_definitions
            .get(tdi as usize)
            .unwrap()
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
