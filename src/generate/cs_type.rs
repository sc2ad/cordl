use std::{
    collections::HashMap,
    io::{Cursor, Read},
    rc::Rc,
};

use brocolib::{
    global_metadata::{
        FieldIndex, Il2CppTypeDefinition, MethodIndex, ParameterIndex, TypeDefinitionIndex,
    },
    runtime_metadata::{Il2CppType, Il2CppTypeEnum, TypeData},
};
use byteorder::{LittleEndian, ReadBytesExt};
use itertools::Itertools;

use super::{
    config::GenerationConfig,
    constants::{
        MethodDefintionExtensions, TypeDefinitionExtensions, TypeExtentions,
        TYPE_ATTRIBUTE_INTERFACE, OBJECT_WRAPPER_TYPE,
    },
    context::CppContextCollection,
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

    fn get_tag_tdi(tag: TypeData) -> TypeDefinitionIndex {
        match tag {
            TypeData::TypeDefinitionIndex(tdi) => tdi,
            _ => panic!("Unsupported type: {tag:?}"),
        }
    }

    fn parent_joined_cpp_name(
        metadata: &Metadata,
        config: &GenerationConfig,
        tdi: TypeDefinitionIndex,
    ) -> String {
        let parent = metadata.child_to_parent_map.get(&tdi);
        let ty = &metadata.metadata.global_metadata.type_definitions[tdi];

        let self_name = ty.name(metadata.metadata);

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
        tag: TypeData,
    ) -> Option<CppType> {
        // let iface = metadata.interfaces.get(t.interfaces_start);
        // Then, handle interfaces

        // Then, handle methods
        // - This includes constructors
        // inherited methods will be inherited

        let tdi = Self::get_tag_tdi(tag);
        let parent_pair = metadata.child_to_parent_map.get(&tdi);

        let t = &metadata.metadata.global_metadata.type_definitions[tdi];

        // Generics
        let generics = t.generic_container_index.is_valid().then(|| {
            let generic_tdi = t.generic_container(metadata.metadata).owner_index;
            let generic_t = &metadata.metadata.global_metadata.type_definitions[tdi];

            println!("Generic TDI {generic_tdi} vs TDI {tdi:?}");
            println!("{generic_t:?}");

            t.generic_container(metadata.metadata)
                .generic_parameters(metadata.metadata)
                .iter()
                .map(|param| (param, param.constraints(metadata.metadata)))
                .collect_vec()
        });

        let cpp_template = CppTemplate {
            names: generics
                .unwrap_or_default()
                .iter()
                .map(|(g, _)| g.name(metadata.metadata).to_string())
                .collect(),
        };

        let ns = t.namespace(metadata.metadata);
        let name = t.name(metadata.metadata);
        let full_name = t.full_name(metadata.metadata, false);
        let cpp_full_name = if ns.is_empty() {
            format!("GlobalNamespace::{}", config.namespace_cpp(&full_name))
        } else {
            config.namespace_cpp(&full_name)
        };

        let mut cpptype = CppType {
            self_tag: tag,
            nested: parent_pair.is_some(),
            prefix_comments: vec![format!("Type: {ns}::{name}")],
            namespace: config.namespace_cpp(ns),
            cpp_namespace: config.namespace_cpp(ns),
            name: config.name_cpp(name),
            cpp_name: config.name_cpp(name),
            cpp_full_name,

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
        self.make_parents(metadata, ctx_collection, tdi);
        self.make_fields(metadata, ctx_collection, tdi);
        self.make_properties(metadata, ctx_collection, tdi);
        self.make_methods(metadata, config, ctx_collection, tdi);

        if let Some(func) = metadata.custom_type_handler.get(&tdi) {
            func(self.get_mut_cpp_type())
        }
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
            let fields = t
                .fields(metadata.metadata)
                .iter()
                .map(|field| {
                    let f_type = metadata
                        .metadata_registration
                        .types
                        .get(field.type_index as usize)
                        .unwrap();

                    let cpp_name =
                        cpp_type.cppify_name_il2cpp(ctx_collection, metadata, f_type, false);

                    CppParam {
                        name: field.name(metadata.metadata).to_string(),
                        ty: cpp_name,
                        modifiers: "".to_string(),
                        def_value: Some("{}".to_string()),
                    }
                })
                .collect_vec();
            cpp_type
                .declarations
                .push(CppMember::ConstructorImpl(CppConstructorImpl {
                    holder_cpp_ty_name: cpp_type.cpp_name().clone(),
                    parameters: fields,
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

            // 2 because each method gets a method struct and method decl
            // a constructor will add an additional one for each
            cpp_type.declarations.reserve(2 * (t.method_count as usize + 1));
            cpp_type.implementations.reserve(t.method_count as usize + 1);

            // Then, for each method, write it out
            for (i, method) in t.methods(metadata.metadata).iter().enumerate() {
                let method_index = MethodIndex::new(t.method_start.index() + i as u32);
                let m_name = method.name(metadata.metadata);

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

                for (pi, param) in method.parameters(metadata.metadata).iter().enumerate() {
                    let param_index =
                        ParameterIndex::new(method.parameter_start.index() + pi as u32);
                    let param_type = metadata
                        .metadata_registration
                        .types
                        .get(param.type_index as usize)
                        .unwrap();

                    let mut param_cpp_name =
                        cpp_type.cppify_name_il2cpp(ctx_collection, metadata, param_type, false);

                    if param_type.byref {
                        param_cpp_name = format!("ByRef<{param_cpp_name}>");
                        cpp_type.requirements.needs_byref_include();
                    }

                    let def_value = Self::param_default_value(metadata, param_index);

                    m_params.push(CppParam {
                        name: param.name(metadata.metadata).to_string(),
                        def_value,
                        ty: param_cpp_name,
                        modifiers: "".to_string(),
                    });
                }

                let generics = if method.generic_container_index.is_valid() {
                    method
                        .generic_container(metadata.metadata)
                        .unwrap()
                        .generic_parameters(metadata.metadata)
                        .iter()
                        .map(|param| param.name(metadata.metadata).to_string())
                        .collect_vec()
                } else {
                    vec![]
                };

                let template = CppTemplate { names: generics };

                // Need to include this type
                let m_ret_cpp_type_name =
                    cpp_type.cppify_name_il2cpp(ctx_collection, metadata, m_ret_type, false);

                let method_calc = &metadata.method_calculations[&method_index];

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
                            ty: cpp_type.formatted_complete_cpp_name().clone(),
                            parameters: m_params.clone(),
                            template: template.clone(),
                        }));
                }

                let declaring_type = method.declaring_type(metadata.metadata);
                let tag = TypeData::TypeDefinitionIndex(method.declaring_type);
                let declaring_cpp_type: Option<&CppType> = if tag == cpp_type.self_tag {
                    Some(cpp_type)
                } else {
                    ctx_collection.get_cpp_type(tag)
                };

                cpp_type
                    .nonmember_implementations
                    .push(Rc::new(CppMethodSizeStruct {
                        ret_ty: m_ret_cpp_type_name.clone(),
                        cpp_method_name: config.name_cpp(m_name),
                        complete_type_name: cpp_type.formatted_complete_cpp_name().clone(),
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
        cpp_type.declarations.reserve(t.field_count as usize);
        for (i, field) in t.fields(metadata.metadata).iter().enumerate() {
            let field_index = FieldIndex::new(t.field_start.index() + i as u32);
            let f_name = field.name(metadata.metadata);
            let f_offset = metadata
                .metadata_registration
                .field_offsets
                .as_ref()
                .unwrap()[tdi.index() as usize][i];
            let f_type = metadata
                .metadata_registration
                .types
                .get(field.type_index as usize)
                .unwrap();

            if let TypeData::TypeDefinitionIndex(tdi) = f_type.data && metadata.blacklisted_types.contains(&tdi) {
                if !cpp_type.is_value_type {
                    continue;
                }  
                println!("Value type uses {tdi:?} which is blacklisted! TODO");
            }

            let _f_type_data = f_type.data;

            let cpp_name = cpp_type.cppify_name_il2cpp(ctx_collection, metadata, f_type, false);

            let def_value = Self::field_default_value(metadata, field_index);

            // Need to include this type
            cpp_type.declarations.push(CppMember::Field(CppField {
                name: f_name.to_owned(),
                ty: cpp_name,
                offset: f_offset,
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

            // We have a parent, lets do something with it
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
                TypeData::TypeDefinitionIndex(nested_type_index),
            );

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
        ctx_collection: &CppContextCollection,
        tdi: TypeDefinitionIndex,
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

            let p_cpp_name = cpp_type.cppify_name_il2cpp(ctx_collection, metadata, p_type, false);

            let method_map = |p: MethodIndex| {
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
                setter: p_setter.map(|_| method_map(prop.set_method_index(t))),
                getter: p_getter.map(|_| method_map(prop.get_method_index(t))),
                abstr: p_getter.is_some_and(|p| p.is_abstract_method())
                    || p_setter.is_some_and(|p| p.is_abstract_method()),
                instance: !p_getter.or(p_setter).unwrap().is_static_method(),
            }));
        }
    }

    fn default_value_blob(metadata: &Metadata, ty: Il2CppTypeEnum, data_index: usize) -> String {
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
            Il2CppTypeEnum::Valuetype | Il2CppTypeEnum::I4 => {
                cursor.read_i32::<Endian>().unwrap().to_string()
            }
            // TODO: We assume 64 bit
            Il2CppTypeEnum::I | Il2CppTypeEnum::I8 => {
                cursor.read_i64::<Endian>().unwrap().to_string()
            }
            Il2CppTypeEnum::U1 => cursor.read_u8().unwrap().to_string(),
            Il2CppTypeEnum::U2 => cursor.read_u16::<Endian>().unwrap().to_string(),
            Il2CppTypeEnum::U4 => cursor.read_u32::<Endian>().unwrap().to_string(),
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
                let size = cursor.read_u32::<Endian>().unwrap();
                let mut str = String::with_capacity(size as usize);
                unsafe {
                    cursor.read_exact(str.as_bytes_mut()).unwrap();
                }
                str
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

                Self::default_value_blob(metadata, ty.ty, def.data_index.index() as usize)
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

                Self::default_value_blob(metadata, ty.ty, def.data_index.index() as usize)
            })
    }

    fn cppify_name_il2cpp(
        &mut self,
        ctx_collection: &CppContextCollection,
        metadata: &Metadata,
        typ: &Il2CppType,
        add_include: bool,
    ) -> String {
        let tag = typ.data;

        let _context_tag = ctx_collection.get_context_root_tag(tag);
        let cpp_type = self.get_mut_cpp_type();
        let mut nested_types: HashMap<TypeData, String> = cpp_type
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
            | Il2CppTypeEnum::U
            | Il2CppTypeEnum::R4
            | Il2CppTypeEnum::R8 => {
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
                // Self
                if tag == cpp_type.self_tag {
                    // TODO: println!("Warning! This is self referencing, handle this better in the future");
                    return cpp_type.formatted_complete_cpp_name().clone();
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

                to_incl_ty.formatted_complete_cpp_name().clone()
            }
            // TODO: MVAR and VAR
            Il2CppTypeEnum::Szarray => {
                requirements.needs_arrayw_include();

                let generic: String = match typ.data {
                    TypeData::TypeIndex(e) => {
                        let ty = &metadata.metadata_registration.types[e];
                        self.cppify_name_il2cpp(ctx_collection, metadata, ty, false)
                    }

                    _ => panic!("Unknown type data for array {typ:?}!"),
                };

                format!("::ArrayW<{generic}>")
            }
            Il2CppTypeEnum::Mvar | Il2CppTypeEnum::Var => match typ.data {
                // TODO: Alias to actual generic
                TypeData::GenericParameterIndex(index) => {
                    let generic_param =
                        &metadata.metadata.global_metadata.generic_parameters[index];

                    let name = generic_param.name(metadata.metadata);

                    name.to_string()
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

                    let types = generic_inst
                        .types
                        .iter()
                        .map(|t| mr.types.get(*t).unwrap())
                        .map(|t| self.cppify_name_il2cpp(ctx_collection, metadata, t, false));

                    let generic_types = types.collect_vec();

                    let generic_type = &mr.types[generic_class.type_index];
                    let owner_name =
                        self.cppify_name_il2cpp(ctx_collection, metadata, generic_type, false);

                    format!("{owner_name}<{}>", generic_types.join(","))
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
            Il2CppTypeEnum::R4 => "float32_t".to_string(),
            Il2CppTypeEnum::R8 => "float64_t".to_string(),

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

impl CSType for CppType {
    fn get_mut_cpp_type(&mut self) -> &mut CppType {
        self
    }

    fn get_cpp_type(&self) -> &CppType {
        self
    }
}
