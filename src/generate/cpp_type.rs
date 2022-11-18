use byteorder::{BigEndian, ReadBytesExt};
use std::{
    collections::HashSet,
    intrinsics::size_of,
    io::{Cursor, Write},
};

use color_eyre::eyre::Context;
use il2cpp_binary::{Type, TypeData, TypeEnum};
use il2cpp_metadata_raw::{Il2CppGenericParameter, TypeDefinitionIndex};
use itertools::Itertools;

use super::{
    config::GenerationConfig,
    constants::{MethodDefintionExtensions, TypeDefinitionExtensions, TypeExtentions},
    context::{CppContextCollection, TypeTag},
    members::{
        CppCommentedString, CppField, CppForwardDeclare, CppInclude, CppMember, CppMethodData,
        CppMethodDecl, CppMethodImpl, CppMethodSizeStruct, CppParam, CppProperty, CppTemplate,
    },
    metadata::Metadata,
    writer::Writable,
};

#[derive(Debug, Clone, Default)]
pub struct CppTypeRequirements {
    pub forward_declares: HashSet<(CppForwardDeclare, CppInclude)>,

    // Only value types or classes
    pub required_includes: HashSet<CppInclude>,
}

// Represents all of the information necessary for a C++ TYPE!
// A C# type will be TURNED INTO this
#[derive(Debug, Clone)]
pub struct CppType {
    self_tdi: TypeDefinitionIndex,
    prefix_comments: Vec<String>,
    namespace: String,
    cpp_namespace: String,
    name: String,
    cpp_name: String,
    pub declarations: Vec<CppMember>,
    pub implementations: Vec<CppMember>,

    pub is_struct: bool,
    pub requirements: CppTypeRequirements,

    pub inherit: Vec<String>,
    pub generic_args: CppTemplate, // Names of templates e.g T, TKey etc.
}

impl CppTypeRequirements {
    pub fn need_wrapper(&mut self) {
        self.required_includes.insert(CppInclude::new(
            "beatsaber-hook/shared/utils/base-wrapper-type.hpp".into(),
        ));
    }
    pub fn needs_int_include(&mut self) {
        self.required_includes
            .insert(CppInclude::new_system("cstdint".into()));
    }
    pub fn needs_stringw_include(&mut self) {
        self.required_includes.insert(CppInclude::new(
            "beatsaber-hook/shared/utils/typedefs-string.hpp".into(),
        ));
    }
    pub fn needs_arrayw_include(&mut self) {
        self.required_includes.insert(CppInclude::new(
            "beatsaber-hook/shared/utils/typedefs-array".into(),
        ));
    }
}

impl CppType {
    pub fn namespace(&self) -> &String {
        &self.namespace
    }

    pub fn cpp_namespace(&self) -> &str {
        &self.cpp_namespace
    }

    pub fn name(&self) -> &String {
        &self.name
    }

    pub fn cpp_name(&self) -> &String {
        &self.cpp_name
    }

    fn get_type_definition<'a>(
        &self,
        metadata: &'a Metadata,
        tdi: TypeDefinitionIndex,
    ) -> &'a il2cpp_metadata_raw::Il2CppTypeDefinition {
        metadata
            .metadata
            .type_definitions
            .get(tdi as usize)
            .unwrap()
    }

    pub fn get_cpp_ty_name(
        &mut self,
        ctx_collection: &mut CppContextCollection,
        metadata: &Metadata,
        config: &GenerationConfig,
        typ: &Type,
        add_include: bool,
    ) -> String {
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
                self.requirements.needs_int_include();
            }
            _ => (),
        };

        match typ.ty {
            TypeEnum::Object => {
                self.requirements.need_wrapper();
                "::bs_hook::Il2CppWrapperType".to_string()
            }
            TypeEnum::Valuetype | TypeEnum::Class => {
                // Self
                if let TypeData::TypeDefinitionIndex(tdi) = typ.data && tdi == self.self_tdi {
                    // TODO: println!("Warning! This is self referencing, handle this better in the future");
                    return self.formatted_complete_cpp_name();
                }

                // In this case, just inherit the type
                // But we have to:
                // - Determine where to include it from
                let to_incl = ctx_collection.make_from(metadata, config, typ.data);

                // - Include it
                if add_include {
                    self.requirements
                        .required_includes
                        .insert(CppInclude::new_context(to_incl));
                }
                let inc = CppInclude::new_context(to_incl);
                let to_incl_ty = to_incl.get_cpp_type(typ.data.into()).unwrap();

                // Forward declare it
                if !add_include {
                    self.requirements
                        .forward_declares
                        .insert((CppForwardDeclare::from_cpp_type(to_incl_ty), inc));
                }

                to_incl_ty.formatted_complete_cpp_name()
            }
            TypeEnum::Szarray => {
                // In this case, just inherit the type
                // But we have to:
                // - Determine where to include it from
                self.requirements.needs_arrayw_include();

                // let to_incl = ctx_collection.make_from(metadata, config, typ.data, false);
                // let to_incl_ty = to_incl.get_cpp_type(TypeTag::from(typ.data)).unwrap();
                let generic_type = match typ.data.into() {
                    TypeTag::TypeDefinition(e) | TypeTag::GenericParameter(e) => {
                        metadata.metadata_registration.types.get(e as usize)
                    }
                    TypeTag::GenericClass(e) | TypeTag::Type(e) => {
                        metadata.metadata_registration.types.get(e)
                    }
                    _ => panic!("Unknown type data for array!"),
                }
                .unwrap();
                format!(
                    "::ArrayW<{}>",
                    self.get_cpp_ty_name(ctx_collection, metadata, config, generic_type, false)
                )
            }
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
                self.requirements.needs_stringw_include();
                "::StringW".to_string()
            }
            TypeEnum::Ptr => "void*".to_owned(),
            // TODO: Void and the other primitives
            _ => format!("/* UNKNOWN TYPE! {:?} */", typ),
        }
    }

    pub fn formatted_complete_cpp_name(&self) -> String {
        // We found a valid type that we have defined for this idx!
        // TODO: We should convert it here.
        // Ex, if it is a generic, convert it to a template specialization
        // If it is a normal type, handle it accordingly, etc.
        format!("{}::{}", self.cpp_namespace(), self.cpp_name())
    }

    pub fn classof_call(&self) -> String {
        format!(
            "&::il2cpp_utils::il2cpp_type_check::il2cpp_no_arg_class<{}>::get",
            self.formatted_complete_cpp_name()
        )
    }

    pub fn write_impl(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        // Write all declarations within the type here
        self.implementations.iter().for_each(|d| {
            d.write(writer).unwrap();
        });

        Ok(())
    }

    pub fn write_def(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        self.prefix_comments.iter().for_each(|pc| {
            writeln!(writer, "// {pc}")
                .context("Prefix comment")
                .unwrap();
        });
        // Forward declare
        writeln!(
            writer,
            "// Forward declaring type: {}::{}",
            self.cpp_namespace(),
            self.name()
        )?;
        writeln!(writer, "namespace {} {{", self.cpp_namespace())?;
        writer.indent();
        self.generic_args.write(writer)?;
        writeln!(writer, "struct {};", self.name())?;

        // Write type definition
        self.generic_args.write(writer)?;
        writeln!(writer, "// Is value type: {}", self.is_struct)?;
        // Type definition plus inherit lines
        writeln!(
            writer,
            "struct {} : public {} {{",
            self.name(),
            self.inherit.join(", ")
        )?;
        writer.indent();
        // Write all declarations within the type here
        self.declarations.iter().for_each(|d| {
            d.write(writer).unwrap();
        });
        // Type complete
        writer.dedent();
        writeln!(writer, "}};")?;
        // Namespace complete
        writer.dedent();
        writeln!(writer, "}} // namespace {}", self.cpp_namespace())?;
        // TODO: Write additional meta-info here, perhaps to ensure correct conversions?
        Ok(())
    }
}

pub fn fill_from_il2cpp(
    cpp_type: &mut CppType,
    metadata: &Metadata,
    config: &GenerationConfig,
    ctx_collection: &mut CppContextCollection,
    tdi: TypeDefinitionIndex,
) {
    make_methods(cpp_type, metadata, config, ctx_collection, tdi);
    make_fields(cpp_type, metadata, config, ctx_collection, tdi);
    make_properties(cpp_type, metadata, config, ctx_collection, tdi);
    make_parents(cpp_type, metadata, config, ctx_collection, tdi);
}

fn make_methods(
    cpp_type: &mut CppType,
    metadata: &Metadata,
    config: &GenerationConfig,
    ctx_collection: &mut CppContextCollection,
    tdi: TypeDefinitionIndex,
) {
    let t = cpp_type.get_type_definition(metadata, tdi);

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
            // TODO: Get method size
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
            let mut m_params: Vec<CppParam> = vec![];

            for p in 0..method.parameter_count {
                let param = metadata
                    .metadata
                    .parameters
                    .get((method.parameter_start + p as u32) as usize)
                    .unwrap();

                let param_type = metadata
                    .metadata_registration
                    .types
                    .get(param.type_index as usize)
                    .unwrap();

                let param_cpp_name =
                    cpp_type.get_cpp_ty_name(ctx_collection, metadata, config, param_type, false);

                m_params.push(CppParam {
                    name: metadata
                        .metadata
                        .get_str(param.name_index as u32)
                        .unwrap()
                        .to_string(),
                    ty: param_cpp_name,
                    modifiers: if param_type.is_byref() {
                        String::from("byref")
                    } else {
                        String::from("")
                    },
                });
            }

            // Need to include this type
            let m_ret_cpp_type_name =
                cpp_type.get_cpp_ty_name(ctx_collection, metadata, config, m_ret_type, false);

            let method_calc = metadata
                .method_calculations
                .get(&(t.method_start + i as u32))
                .unwrap();

            cpp_type
                .implementations
                .push(CppMember::MethodImpl(CppMethodImpl {
                    name: m_name.to_string(),
                    cpp_name: config.name_cpp(m_name),
                    return_type: m_ret_cpp_type_name.clone(),
                    parameters: m_params.clone(),
                    instance: true,
                    prefix_modifiers: Default::default(),
                    suffix_modifiers: Default::default(),
                    holder_namespaze: cpp_type.namespace.clone(),
                    holder_name: cpp_type.name.clone(),
                }));
            cpp_type
                .implementations
                .push(CppMember::MethodSizeStruct(CppMethodSizeStruct {
                    cpp_name: config.name_cpp(m_name),
                    instance: true,
                    method_data: CppMethodData {
                        addrs: method_calc.addrs,
                        estimated_size: method_calc.estimated_size,
                    },
                    ty: cpp_type.formatted_complete_cpp_name(),
                    params: m_params.clone(),
                }));
            cpp_type
                .declarations
                .push(CppMember::MethodDecl(CppMethodDecl {
                    cpp_name: config.name_cpp(m_name),
                    return_type: m_ret_cpp_type_name,
                    parameters: m_params,
                    instance: true,
                    prefix_modifiers: Default::default(),
                    suffix_modifiers: Default::default(),
                    method_data: CppMethodData {
                        addrs: method_calc.addrs,
                        estimated_size: method_calc.estimated_size,
                    },
                    is_virtual: method.is_virtual_method(),
                }));
        }
    }
}

fn make_fields(
    cpp_type: &mut CppType,
    metadata: &Metadata,
    config: &GenerationConfig,
    ctx_collection: &mut CppContextCollection,
    tdi: TypeDefinitionIndex,
) {
    let t = cpp_type.get_type_definition(metadata, tdi);

    // Then, handle fields
    if t.field_count > 0 {
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
                cpp_type.get_cpp_ty_name(ctx_collection, metadata, config, f_type, false);

            // TODO: Move to function
            let def_value = if f_type.is_const() {
                metadata
                    .metadata
                    .field_default_values
                    .get(field_index)
                    .map(|def| {
                        let ty = metadata
                            .metadata_registration
                            .types
                            .get(field.type_index as usize)
                            .unwrap();
                        let _size = match ty.ty {
                            TypeEnum::String => *metadata
                                .metadata
                                .string_literal_data
                                .get(def.data_index as usize)
                                .unwrap() as usize,

                            TypeEnum::I1 => size_of::<i8>(),
                            TypeEnum::I2 => size_of::<i16>(),
                            TypeEnum::I4 => size_of::<i32>(),
                            // TODO: We assume 64 bit
                            TypeEnum::I | TypeEnum::I8 => size_of::<i64>(),
                            TypeEnum::U1 => size_of::<u8>(),
                            TypeEnum::U2 => size_of::<u16>(),
                            TypeEnum::U4 => size_of::<u32>(),
                            // TODO: We assume 64 bit
                            TypeEnum::U | TypeEnum::U8 => size_of::<u64>(),

                            // https://learn.microsoft.com/en-us/nimbusml/concepts/types
                            // https://en.cppreference.com/w/cpp/types/floating-point
                            TypeEnum::R4 => size_of::<f32>(),
                            TypeEnum::R8 => size_of::<f64>(),
                            TypeEnum::Boolean => size_of::<bool>(),
                            TypeEnum::Char => size_of::<char>() * 2,

                            // Figure out
                            // TypeEnum::Ptr => 8,
                            // TypeEnum::Valuetype => 4, // TODO:
                            // TypeEnum::Class => 4,
                            // TypeEnum::Szarray => 4, // TODO:
                            _ => {
                                // println!("Invalid type {:?}", ty);
                                4
                            }
                        };

                        let data = &metadata.metadata.field_and_parameter_default_value_data;
                        let mut cursor = Cursor::new(data);
                        cursor.set_position(def.data_index as u64);

                        match ty.ty {
                            TypeEnum::Boolean => {
                                (if data[0] != 0 { "false" } else { "true" }).to_string()
                            }
                            TypeEnum::I1 => cursor.read_i8().unwrap().to_string(),
                            TypeEnum::I2 => cursor.read_i16::<BigEndian>().unwrap().to_string(),
                            TypeEnum::I4 => cursor.read_i32::<BigEndian>().unwrap().to_string(),
                            // TODO: We assume 64 bit
                            TypeEnum::I | TypeEnum::I8 => {
                                cursor.read_i64::<BigEndian>().unwrap().to_string()
                            }
                            TypeEnum::U1 => cursor.read_u8().unwrap().to_string(),
                            TypeEnum::U2 => cursor.read_u16::<BigEndian>().unwrap().to_string(),
                            TypeEnum::U4 => cursor.read_u32::<BigEndian>().unwrap().to_string(),
                            // TODO: We assume 64 bit
                            TypeEnum::U | TypeEnum::U8 => {
                                cursor.read_u64::<BigEndian>().unwrap().to_string()
                            }

                            // https://learn.microsoft.com/en-us/nimbusml/concepts/types
                            // https://en.cppreference.com/w/cpp/types/floating-point
                            TypeEnum::R4 => cursor.read_f32::<BigEndian>().unwrap().to_string(),
                            TypeEnum::R8 => cursor.read_f64::<BigEndian>().unwrap().to_string(),
                            TypeEnum::Char => {
                                String::from_utf16_lossy(&[cursor.read_u16::<BigEndian>().unwrap()])
                            }
                            TypeEnum::String => String::from_utf16_lossy(
                                &data
                                    .chunks(2)
                                    .into_iter()
                                    .map(|e| u16::from_be_bytes(e.try_into().unwrap()))
                                    .collect_vec(),
                            ),

                            _ => "unknown".to_string(),
                        }
                    })
            } else {
                None
            };

            // Need to include this type
            cpp_type.declarations.push(CppMember::Field(CppField {
                name: f_name.to_owned(),
                ty: cpp_name,
                offset: *f_offset,
                instance: !f_type.is_static() && !f_type.is_const(),
                readonly: f_type.is_const(),
                classof_call: cpp_type.classof_call(),
                literal_value: def_value,
            }));
        }
    }
}

fn make_parents(
    cpp_type: &mut CppType,
    metadata: &Metadata,
    config: &GenerationConfig,
    ctx_collection: &mut CppContextCollection,
    tdi: TypeDefinitionIndex,
) {
    let t = metadata
        .metadata
        .type_definitions
        .get(tdi as usize)
        .unwrap();
    let ns = metadata.metadata.get_str(t.namespace_index).unwrap();
    let name = metadata.metadata.get_str(t.name_index).unwrap();

    if t.parent_index == u32::MAX {
        if t.flags & 0x00000020 == 0 {
            println!("Skipping type: {ns}::{name} because it has parent index: {} and is not an interface!", t.parent_index);
        }
    } else if let Some(parent_type) = metadata
        .metadata_registration
        .types
        .get(t.parent_index as usize)
    {
        // We have a parent, lets do something with it
        let inherit_type =
            cpp_type.get_cpp_ty_name(ctx_collection, metadata, config, parent_type, true);
        cpp_type.inherit.push(inherit_type);
    } else {
        panic!("NO PARENT! But valid index found: {}", t.parent_index);
    }
}

pub fn make_cpp_type(
    metadata: &Metadata,
    config: &GenerationConfig,
    tdi: TypeDefinitionIndex,
) -> Option<CppType> {
    // let iface = metadata.interfaces.get(t.interfaces_start);
    // Then, handle interfaces

    // Then, handle methods
    // - This includes constructors
    // inherited methods will be inherited

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
    let cpptype = CppType {
        prefix_comments: vec![format!("Type: {ns}::{name}")],
        namespace: config.namespace_cpp(ns),
        name: config.name_cpp(name),
        declarations: Default::default(),
        inherit: Default::default(),
        generic_args: cpp_template,
        requirements: Default::default(),
        is_struct: t.is_value_type(),
        implementations: Default::default(),
        self_tdi: tdi,
        cpp_namespace: config.namespace_cpp(ns),
        cpp_name: config.name_cpp(name),
    };

    if t.parent_index == u32::MAX {
        if t.flags & 0x00000020 == 0 {
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

fn make_properties(
    cpp_type: &mut CppType,
    metadata: &Metadata,
    config: &GenerationConfig,
    ctx_collection: &mut CppContextCollection,
    tdi: u32,
) {
    let t = cpp_type.get_type_definition(metadata, tdi);

    // Then, handle properties
    if t.property_count > 0 {
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
            let p_setter = metadata.metadata.methods.get(prop.set as usize);

            let p_getter = metadata.metadata.methods.get(prop.get as usize);

            let p_type = metadata
                .metadata_registration
                .types
                .get(p_getter.or(p_setter).unwrap().return_type as usize)
                .unwrap();

            let cpp_name =
                cpp_type.get_cpp_ty_name(ctx_collection, metadata, config, p_type, false);

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
                ty: cpp_name.clone(),
                classof_call: cpp_type.classof_call(),
                setter: p_setter.map(|_| method_map(prop.set)),
                getter: p_getter.map(|_| method_map(prop.get)),
                abstr: p_getter.or(p_setter).unwrap().is_abstract_method(),
                instance: !p_getter.or(p_setter).unwrap().is_static_method(),
            }));
        }
    }
}
