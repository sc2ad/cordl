use std::{collections::HashSet, io::Write};

use color_eyre::eyre::Context;
use il2cpp_binary::{Type, TypeData, TypeEnum};
use il2cpp_metadata_raw::TypeDefinitionIndex;

use super::{
    config::GenerationConfig,
    constants::{MethodDefintionExtensions, TypeDefinitionExtensions, TypeExtentions},
    context::{CppCommentedString, CppContextCollection, TypeTag},
    members::{
        CppField, CppMember, CppMethod, CppMethodData, CppMethodSizeStruct, CppParam, CppProperty,
    },
    metadata::Metadata,
    writer::Writable,
};

#[derive(Debug, Clone, Default)]
pub struct CppTypeRequirements {
    pub needs_wrapper: bool,
    pub needs_int_include: bool,
    pub needs_stringw_include: bool,
    pub needs_arrayw_include: bool,

    pub forward_declare_tids: HashSet<TypeTag>,

    // Only value types or classes
    pub required_includes: Vec<TypeTag>,
}

// Represents all of the information necessary for a C++ TYPE!
// A C# type will be TURNED INTO this
#[derive(Debug, Clone)]
pub struct CppType {
    ty: TypeTag,
    prefix_comments: Vec<String>,
    namespace: String,
    name: String,
    declarations: Vec<CppMember>,
    implementations: Vec<CppMember>,

    pub value_type: bool,
    pub requirements: CppTypeRequirements,

    pub inherit: Vec<String>,
    pub template_line: Option<CppCommentedString>,
    pub generic_args: Vec<String>,
    made: bool, // We want to handle metadata generation for stuff too, maybe that goes in the context?
}

impl CppType {
    pub fn namespace(&self) -> &String {
        &self.namespace
    }

    pub fn namespace_fixed(&self) -> &str {
        if self.namespace.is_empty() {
            "GlobalNamespace"
        } else {
            &self.namespace
        }
    }

    pub fn name(&self) -> &String {
        &self.name
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

    pub fn cpp_name(
        &mut self,
        ctx_collection: &mut CppContextCollection,
        metadata: &Metadata,
        config: &GenerationConfig,
        typ: &Type,
        include_ref: bool,
    ) -> String {
        self.basic_cpp_name(
            ctx_collection,
            metadata,
            config,
            typ.ty,
            Some(TypeTag::from(typ.data)),
            include_ref,
        )
    }

    pub fn basic_cpp_name(
        &mut self,
        ctx_collection: &mut CppContextCollection,
        metadata: &Metadata,
        config: &GenerationConfig,
        typ: TypeEnum,
        data: Option<TypeTag>,
        include_ref: bool,
    ) -> String {
        match typ {
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
                self.requirements.needs_int_include = true;
            }
            _ => (),
        };

        match typ {
            TypeEnum::Object => {
                self.requirements.needs_wrapper = true;
                "::bs_hook::Il2CppWrapperType".to_string()
            }
            TypeEnum::Valuetype | TypeEnum::Class => {
                // In this case, just inherit the type
                // But we have to:
                // - Determine where to include it from
                let to_incl = ctx_collection.make_from(metadata, config, data.unwrap(), false);

                // - Include it
                if include_ref {
                    self.requirements.required_includes.push(data.unwrap());
                }
                let to_incl_ty = to_incl.get_cpp_type(data.unwrap()).unwrap();
                to_incl_ty.self_cpp_type_name()
            }
            TypeEnum::Szarray => {
                // In this case, just inherit the type
                // But we have to:
                // - Determine where to include it from
                self.requirements.needs_arrayw_include = true;

                // let to_incl = ctx_collection.make_from(metadata, config, typ.data, false);
                // let to_incl_ty = to_incl.get_cpp_type(TypeTag::from(typ.data)).unwrap();
                let generic_type = match data.unwrap() {
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
                    self.cpp_name(ctx_collection, metadata, config, generic_type, false)
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
                self.requirements.needs_stringw_include = true;
                "::StringW".to_string()
            }
            // TODO: Void and the other primitives
            _ => format!("/* UNKNOWN TYPE! {:?} */", typ),
        }
    }

    pub fn self_cpp_type_name(&self) -> String {
        // We found a valid type that we have defined for this idx!
        // TODO: We should convert it here.
        // Ex, if it is a generic, convert it to a template specialization
        // If it is a normal type, handle it accordingly, etc.
        match self.ty {
            TypeTag::TypeDefinition(_) => {
                format!("{}::{}", self.namespace_fixed(), self.name())
            }
            _ => panic!("Unsupported type conversion for type: {:?}!", self.ty),
        }
    }

    pub fn classof_call(&self) -> String {
        format!(
            "&::il2cpp_utils::il2cpp_type_check::il2cpp_no_arg_class<{}>::get",
            self.self_cpp_type_name()
        )
    }

    pub fn fill(
        &mut self,
        metadata: &Metadata,
        config: &GenerationConfig,
        ctx_collection: &mut CppContextCollection,
        tdi: TypeDefinitionIndex,
    ) {
        if self.made {
            return;
        }

        self.make_methods(metadata, config, ctx_collection, tdi);
        self.make_fields(metadata, config, ctx_collection, tdi);
        self.make_properties(metadata, config, ctx_collection, tdi);
        self.make_parents(metadata, config, ctx_collection, tdi);
        self.made = true;
    }

    fn make_methods(
        &mut self,
        metadata: &Metadata,
        config: &GenerationConfig,
        ctx_collection: &mut CppContextCollection,
        tdi: TypeDefinitionIndex,
    ) {
        let t = self.get_type_definition(metadata, tdi);

        // Then, handle methods
        if t.method_count > 0 {
            // Write comment for methods
            self.declarations
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

                    // TODO: We need DECLARATIONS in the def, DEFINITIONS in the impl
                    m_params.push(CppParam {
                        name: metadata
                            .metadata
                            .get_str(param.name_index as u32)
                            .unwrap()
                            .to_string(),
                        ty: self.cpp_name(ctx_collection, metadata, config, param_type, false),
                        modifiers: if param_type.is_byref() {
                            String::from("byref")
                        } else {
                            String::from("")
                        },
                    });
                }

                // Need to include this type
                let cpp_type_name =
                    self.cpp_name(ctx_collection, metadata, config, m_ret_type, false);

                let method_calc = metadata
                    .method_calculations
                    .get(&(t.method_start + i as u32))
                    .unwrap();

                self.declarations
                    .push(CppMember::MethodSizeStruct(CppMethodSizeStruct {
                        name: m_name.to_owned(),
                        instance: true,
                        method_data: CppMethodData {
                            addrs: method_calc.addrs,
                            estimated_size: method_calc.estimated_size,
                        },
                        ty: self.self_cpp_type_name(),
                        params: m_params.clone(),
                    }));
                self.declarations.push(CppMember::Method(CppMethod {
                    name: m_name.to_owned(),
                    return_type: cpp_type_name,
                    parameters: m_params,
                    instance: true,
                    prefix_modifiers: Default::default(),
                    suffix_modifiers: Default::default(),
                    method_data: CppMethodData {
                        addrs: method_calc.addrs,
                        estimated_size: method_calc.estimated_size,
                    },
                }));
            }
        }
    }

    fn make_fields(
        &mut self,
        metadata: &Metadata,
        config: &GenerationConfig,
        ctx_collection: &mut CppContextCollection,
        tdi: TypeDefinitionIndex,
    ) {
        let t = self.get_type_definition(metadata, tdi);

        // Then, handle fields
        if t.field_count > 0 {
            // Write comment for fields
            self.declarations
                .push(CppMember::Comment(CppCommentedString {
                    data: "".to_string(),
                    comment: Some("Fields".to_string()),
                }));
            // Then, for each field, write it out
            for i in 0..t.field_count {
                let field = metadata
                    .metadata
                    .fields
                    .get((t.field_start + i as u32) as usize)
                    .unwrap();
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

                let cpp_name = self.cpp_name(ctx_collection, metadata, config, f_type, false);

                // Need to include this type
                self.declarations.push(CppMember::Field(CppField {
                    name: f_name.to_owned(),
                    ty: cpp_name,
                    offset: *f_offset,
                    instance: !f_type.is_static() && !f_type.is_const(),
                    readonly: f_type.is_const(),
                    classof_call: self.classof_call(),
                }));

                // forward declare only if field type is not the same type as the holder
                if let TypeData::TypeDefinitionIndex(f_tdi) = f_type.data && f_tdi != tdi {
                    self.requirements.forward_declare_tids.insert(TypeTag::TypeDefinition(f_tdi));
                } /*else if f_type_data != self.ty {
                      self.requirements.forward_declare_tids.insert(f_type_data);
                  }*/
            }
        }
    }

    fn make_parents(
        &mut self,
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
            let inherit_type = self.cpp_name(ctx_collection, metadata, config, parent_type, true);
            self.inherit.push(inherit_type);
        } else {
            panic!("NO PARENT! But valid index found: {}", t.parent_index);
        }
    }

    pub fn make(
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
        let ns = metadata.metadata.get_str(t.namespace_index).unwrap();
        let name = metadata.metadata.get_str(t.name_index).unwrap();
        let cpptype = CppType {
            prefix_comments: vec![format!("Type: {ns}::{name}")],
            namespace: config.namespace_cpp(ns.to_string()),
            name: config.name_cpp(name.to_string()),
            declarations: Default::default(),
            inherit: Default::default(),
            template_line: None,
            generic_args: Default::default(),
            made: false,
            requirements: Default::default(),
            value_type: t.is_value_type(),
            ty: TypeTag::TypeDefinition(tdi),
            implementations: Default::default(),
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
        &mut self,
        metadata: &Metadata,
        config: &GenerationConfig,
        ctx_collection: &mut CppContextCollection,
        tdi: u32,
    ) {
        let t = self.get_type_definition(metadata, tdi);

        // Then, handle properties
        if t.property_count > 0 {
            // Write comment for properties
            self.declarations
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

                let cpp_name = self.cpp_name(ctx_collection, metadata, config, p_type, false);

                let method_map = |p: u32| {
                    let method_calc = metadata.method_calculations.get(&p).unwrap();
                    CppMethodData {
                        estimated_size: method_calc.estimated_size,
                        addrs: method_calc.addrs,
                    }
                };

                // Need to include this type
                self.declarations.push(CppMember::Property(CppProperty {
                    name: p_name.to_owned(),
                    ty: cpp_name.clone(),
                    classof_call: self.classof_call(),
                    setter: p_setter.map(|_| method_map(prop.set)),
                    getter: p_getter.map(|_| method_map(prop.get)),
                    abstr: p_getter.or(p_setter).unwrap().is_abstract_method(),
                    instance: !p_getter.or(p_setter).unwrap().is_static_method(),
                }));

                // forward declare only if field type is not the same type as the holder
                if let TypeData::TypeDefinitionIndex(f_tdi) = p_type.data && f_tdi != tdi {
                    self.requirements.forward_declare_tids.insert(TypeTag::TypeDefinition(f_tdi));
                } /*else if f_type_data != self.ty {
                      self.requirements.forward_declare_tids.insert(f_type_data);
                  }*/
            }
        }
    }

    pub fn write_impl(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        // Write all declarations within the type here
        self.implementations.iter().for_each(|d| {
            d.write(writer).unwrap();
        });

        Ok(())
    }
}

impl Writable for CppType {
    fn write(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        self.prefix_comments.iter().for_each(|pc| {
            writeln!(writer, "// {pc}")
                .context("Prefix comment")
                .unwrap();
        });
        // Forward declare
        writeln!(
            writer,
            "// Forward declaring type: {}::{}",
            self.namespace_fixed(),
            self.name()
        )?;
        writeln!(writer, "namespace {} {{", self.namespace_fixed())?;
        writer.indent();
        if let Some(template) = &self.template_line {
            template.write(writer)?;
        }
        writeln!(writer, "struct {};", self.name())?;

        // Write type definition
        if let Some(template) = &self.template_line {
            template.write(writer).unwrap();
        }
        writeln!(writer, "// Is value type: {}", self.value_type)?;
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
        writeln!(writer, "}} // namespace {}", self.namespace_fixed())?;
        // TODO: Write additional meta-info here, perhaps to ensure correct conversions?
        Ok(())
    }
}
