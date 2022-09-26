use std::io::Write;

use il2cpp_binary::{TypeEnum};
use il2cpp_metadata_raw::{TypeDefinitionIndex};

use super::{config::GenerationConfig, writer::Writable, context::{CppCommentedString, CppContext, CppContextCollection}, metadata::Metadata};

// Represents all of the information necessary for a C++ TYPE!
// A C# type will be TURNED INTO this
#[derive(Debug)]
pub struct CppType {
    prefix_comments: Vec<String>,
    namespace: String,
    name: String,
    declarations: Vec<Box<dyn Writable>>,
    inherit: Vec<String>,
    template_line: Option<CppCommentedString>,
    generic_args: Vec<String>,
    // We want to handle metadata generation for stuff too, maybe that goes in the context?
}

impl CppType {
    pub fn namespace(&self) -> &String {
        &self.namespace
    }
    pub fn name(&self) -> &String {
        &self.name
    }

    pub fn make(metadata: &Metadata, config: &GenerationConfig, ctx_collection: &mut CppContextCollection, ctx: &mut CppContext, tdi: TypeDefinitionIndex) -> Option<CppType> {
        let t = metadata.metadata.type_definitions.get(tdi as usize).unwrap();
        let ns = metadata.metadata.get_str(t.namespace_index).unwrap();
        let name = metadata.metadata.get_str(t.name_index).unwrap();
        let mut cpptype = CppType {
            prefix_comments: vec![format!("Type: {ns}::{name}")],
            namespace: config.namespace_cpp(ns.to_string()),
            name: config.name_cpp(name.to_string()),
            declarations: Vec::new(),
            inherit: Vec::new(),
            template_line: None,
            generic_args: Vec::new()
        };

        if t.parent_index == u32::MAX {
            if t.flags & 0x00000020 == 0 {
                println!("Skipping type: {ns}::{name} because it has parent index: {} and is not an interface!", t.parent_index);
                return None;
            }
        } else if let Some(parent_type) = metadata.metadata_registration.types.get(t.parent_index as usize) {
            // We have a parent, lets do something with it
            match parent_type.ty {
                TypeEnum::Object => {
                    ctx.need_wrapper();
                    cpptype.inherit.push("::bs_hook::Il2CppWrapperType".to_string());
                },
                TypeEnum::Class => {
                    // In this case, just inherit the type
                    // But we have to:
                    // - Determine where to include it from
                    let parent_ctx = ctx_collection.make_from(metadata, config, parent_type.data);
                    // - Include it
                    ctx.add_include_ctx(parent_ctx, "Including parent context".to_string());
                    // - Inherit it
                    cpptype.inherit.push(parent_ctx.get_cpp_type_name(parent_type));
                }
                TypeEnum::Valuetype => (),
                _ => ()
            }
        } else {
            panic!("NO PARENT! But valid index found: {}", t.parent_index);
        }
        // let iface = metadata.interfaces.get(t.interfaces_start);
        // Then, handle interfaces

        // Then, handle fields
        if t.field_count > 0 {
            // Write comment for fields
            cpptype.declarations.push(Box::new(CppCommentedString{
                data: "".to_string(),
                comment: Some("Fields".to_string())
            }));
            // Then, for each field, write it out
            for i in 0..t.field_count {
                let field = metadata.metadata.fields.get((t.field_start + i as u32) as usize).unwrap();
                let f_name = metadata.metadata.get_str(field.name_index).unwrap();
                let f_offset = metadata.metadata_registration.field_offsets.get(tdi as usize).unwrap().get(i as usize).unwrap();
                let f_type = metadata.metadata_registration.types.get(field.type_index as usize).unwrap();

                // Need to include this type
                let cpp_type_name = ctx.cpp_name(ctx_collection, metadata, config, f_type);

                cpptype.declarations.push(Box::new(CppCommentedString{
                    data: "".to_string(), // TODO
                    comment: Some(format!("Field: {i}, name: {f_name}, Type Name: {cpp_type_name}, Offset: {f_offset}"))
                }));
            }
        }
        // Then, handle methods
        // - This includes constructors
        // inherited methods will be inherited

        Some(cpptype)
    }
}

impl Writable for CppType {
    fn write(&self, writer: &mut super::writer::CppWriter) {
        self.prefix_comments.iter().for_each(|pc| {
            writeln!(writer, "// {pc}").unwrap();
        });
        // Forward declare
        writeln!(writer, "// Forward declaring type: {}::{}", self.namespace(), self.name()).unwrap();
        writeln!(writer, "namespace {} {{", self.namespace()).unwrap();
        writer.indent();
        if let Some(template) = &self.template_line {
            template.write(writer);
        }
        writeln!(writer, "struct {};", self.name()).unwrap();
        // Write type definition
        if let Some(template) = &self.template_line {
            template.write(writer);
        }
        // Type definition plus inherit lines
        writeln!(writer, "struct {} : {} {{", self.name(), self.inherit.join(", ")).unwrap();
        writer.indent();
        // Write all declarations within the type here
        self.declarations.iter().for_each(|d| {
            d.write(writer);
        });
        // Type complete
        writer.dedent();
        writeln!(writer, "}};").unwrap();
        // Namespace complete
        writer.dedent();
        writeln!(writer, "}}").unwrap();
        // TODO: Write additional meta-info here, perhaps to ensure correct conversions?
    }
}
