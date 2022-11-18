use std::{collections::HashSet, io::Write};

use color_eyre::eyre::Context;

use il2cpp_metadata_raw::TypeDefinitionIndex;
use itertools::Itertools;

use super::{
    members::{CppForwardDeclare, CppInclude, CppMember, CppTemplate},
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
    pub self_tdi: TypeDefinitionIndex,

    pub(crate) prefix_comments: Vec<String>,
    pub(crate) namespace: String,
    pub(crate) cpp_namespace: String,
    pub(crate) name: String,
    pub(crate) cpp_name: String,

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

    pub fn formatted_complete_cpp_name(&self) -> String {
        // We found a valid type that we have defined for this idx!
        // TODO: We should convert it here.
        // Ex, if it is a generic, convert it to a template specialization
        // If it is a normal type, handle it accordingly, etc.
        format!("{}::{}", self.cpp_namespace(), self.cpp_name())
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
