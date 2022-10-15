use std::{io::Write, path::PathBuf, sync::Arc};

use color_eyre::eyre::Context;
use il2cpp_binary::{Type, TypeData, TypeEnum};
use il2cpp_metadata_raw::TypeDefinitionIndex;

use super::{
    config::GenerationConfig,
    context::{CppCommentedString, CppContextCollection},
    metadata::Metadata,
    writer::Writable,
};

// Represents all of the information necessary for a C++ TYPE!
// A C# type will be TURNED INTO this
#[derive(Debug, Clone)]
pub struct CppType {
    prefix_comments: Vec<String>,
    namespace: String,
    name: String,
    declarations: Vec<Arc<CppCommentedString>>,

    pub needs_wrapper: bool,
    pub forward_declares: HashSet<u32>,
    pub required_includes: Vec<PathBuf>,

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

    // pub fn can_make(
    //     &self,
    //     metadata: &Metadata,
    //     ctx_collection: &CppContextCollection,
    //     tdi: TypeDefinitionIndex,
    // ) -> bool {
    //     if self.made {
    //         return true;
    //     }

    //     let t = self.get_type_definition(metadata, tdi);

    //     if let Some(parent_type) = metadata
    //         .metadata_registration
    //         .types
    //         .get(t.parent_index as usize)
    //     {
    //         // We have a parent, lets do something with it
    //         match parent_type.ty {
    //             TypeEnum::Class => {
    //                 if !ctx_collection.is_type_made(TypeTag::from(parent_type.data)) {
    //                     return false;
    //                 }
    //             }
    //             _ => (),
    //         }
    //     }

    //     // Then, for each field, write it out
    //     for i in 0..t.field_count {
    //         let field = metadata
    //             .metadata
    //             .fields
    //             .get((t.field_start + i as u32) as usize)
    //             .unwrap();
    //         let f_name = metadata.metadata.get_str(field.name_index).unwrap();
    //         let f_offset = metadata
    //             .metadata_registration
    //             .field_offsets
    //             .get(tdi as usize)
    //             .unwrap()
    //             .get(i as usize)
    //             .unwrap();
    //         let f_type = metadata
    //             .metadata_registration
    //             .types
    //             .get(field.type_index as usize)
    //             .unwrap();

    //         if !ctx_collection.is_type_made(TypeTag::from(f_type.data)) {
    //             return false;
    //         }
    //     }

    //     self.made = true;
    //     return true;
    // }

    pub fn field_cpp_name(
        &mut self,
        ctx_collection: &mut CppContextCollection,
        metadata: &Metadata,
        config: &GenerationConfig,
        typ: &Type,
        offset: u32,
    ) -> String {
        let cpp_name = self.cpp_name(ctx_collection, metadata, config, typ, false);

        let readonly = false;
        format!(
            "::bs_hook::InstanceField<{}, 0x{:x},{}>",
            cpp_name, offset, !readonly
        )
    }

    pub fn cpp_name(
        &mut self,
        ctx_collection: &mut CppContextCollection,
        metadata: &Metadata,
        config: &GenerationConfig,
        typ: &Type,
        include_ref: bool,
    ) -> String {
        match typ.ty {
            TypeEnum::Object => {
                self.needs_wrapper = true;
                "::bs_hook::Il2CppWrapperType".to_string()
            }
            TypeEnum::Class => {
                // In this case, just inherit the type
                // But we have to:
                // - Determine where to include it from
                let to_incl = ctx_collection.make_from(metadata, config, typ.data, false);
                // - Include it

                if include_ref {
                    self.required_includes
                        .push(to_incl.get_include_path().to_path_buf());
                }
                to_incl.get_cpp_type_name(typ)
            }
            TypeEnum::Valuetype => "/* UNKNOWN VALUE TYPE! */".to_string(),
            TypeEnum::Void => "void".to_string(),
            TypeEnum::Boolean => "boolean".to_string(),
            TypeEnum::Char => "char16_t".to_string(),
            TypeEnum::String => "::StringW".to_string(),
            // TODO: Void and the other primitives
            _ => format!("/* UNKNOWN TYPE! {:?} */", typ.ty),
        }
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

        // Then, handle fields
        if t.method_count > 0 {
            // Write comment for methods
            self.declarations.push(Arc::new(CppCommentedString {
                data: "".to_string(),
                comment: Some("Methods".to_string()),
            }));
            // Then, for each field, write it out
            for i in 0..t.method_count {
                // TODO: Get method size
                let method = metadata
                    .metadata
                    .methods
                    .get((t.method_start + i as u32) as usize)
                    .unwrap();
                let m_name = metadata.metadata.get_str(method.name_index).unwrap();
                let m_index = metadata
                    .metadata_registration
                    .method_specs
                    .get(tdi as usize)
                    .unwrap()
                    .method_definition_index;
                let m_ret_type = metadata
                    .metadata_registration
                    .types
                    .get(method.return_type as usize)
                    .unwrap();
                let mut m_params: Vec<&Type> = vec![];

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
                    m_params.push(param_type);
                }

                // Need to include this type
                let cpp_type_name =
                    self.cpp_name(ctx_collection, metadata, config, m_ret_type, false);
                let param_names: Vec<String> = m_params
                    .iter()
                    .map(|p| self.cpp_name(ctx_collection, metadata, config, p, false))
                    .collect();

                self.declarations.push(Arc::new(CppCommentedString {
                    data: "".to_string(), // TODO
                    comment: Some(format!(
                        "Method: {i}, name: {m_name}, Return Type Name: {cpp_type_name}, Index: {m_index} Parameters: {param_names:?}"
                    )),
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
            self.declarations.push(Arc::new(CppCommentedString {
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

                // Need to include this type
                let cpp_type_name =
                    self.field_cpp_name(ctx_collection, metadata, config, f_type, *f_offset);

                self.declarations.push(
                     Arc::new(CppCommentedString{
                                    data: format!("{} {};", cpp_type_name, f_name), // TODO
                                    comment: Some(format!("Field: {i}, name: {f_name}, Type Name: {cpp_type_name}, Offset: 0x{f_offset:x}"))
                                }));

                // forward declare only if field type is not the same type as the holder
                if let TypeData::TypeDefinitionIndex(f_tdi) = f_type.data && f_tdi != tdi {
                    self.forward_declares.insert(f_tdi);
                }
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
            needs_wrapper: Default::default(),
            forward_declares: Default::default(),
            required_includes: Default::default(),
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
        writeln!(writer, "}}")?;
        // TODO: Write additional meta-info here, perhaps to ensure correct conversions?
        Ok(())
    }
}
