use std::{collections::HashSet, io::Write, rc::Rc};

use color_eyre::eyre::Context;

use itertools::Itertools;

use super::{
    context::TypeTag,
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
    pub self_tag: TypeTag,

    pub(crate) prefix_comments: Vec<String>,
    pub(crate) namespace: String,
    pub(crate) cpp_namespace: String,
    pub(crate) name: String,
    pub(crate) cpp_name: String,

    pub declarations: Vec<CppMember>,
    pub implementations: Vec<CppMember>,
    /// Outside of the class declaration
    /// Move to CsType/CppType?
    pub nonmember_implementations: Vec<Rc<dyn Writable>>,
    pub nonmember_declarations: Vec<Rc<dyn Writable>>,

    pub is_value_type: bool,
    pub requirements: CppTypeRequirements,

    pub inherit: Vec<String>,
    pub generic_args: CppTemplate, // Names of templates e.g T, TKey etc.

    pub nested_types: Vec<CppType>,
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
        self.write_impl_internal(writer, Some(self.cpp_namespace()))
    }

    pub fn write_def(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        self.write_def_internal(writer, Some(self.cpp_namespace()))
    }

    pub fn write_impl_internal(
        &self,
        writer: &mut super::writer::CppWriter,
        namespace: Option<&str>,
    ) -> color_eyre::Result<()> {
        if let Some(namespace) = namespace {
            writeln!(writer, "namespace {} {{", namespace)?;
        }
        // Write all declarations within the type here
        self.implementations
            .iter()
            .try_for_each(|d| d.write(writer))?;
        self.nonmember_implementations
            .iter()
            .try_for_each(|d| d.write(writer))?;

        // TODO: Figure out
        self.nested_types
            .iter()
            .try_for_each(|n| n.write_impl_internal(writer, Some(self.cpp_name())))?;

        if let Some(namespace) = namespace {
            writeln!(writer, "}} // end namespace {}", namespace)?;
        }

        Ok(())
    }

    fn write_def_internal(
        &self,
        writer: &mut super::writer::CppWriter,
        namespace: Option<&str>,
    ) -> color_eyre::Result<()> {
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

        if let Some(n) = &namespace {
            writeln!(writer, "namespace {n} {{",)?;
            writer.indent();
        }

        self.generic_args.write(writer)?;
        writeln!(writer, "struct {};", self.name())?;

        // Write type definition
        self.generic_args.write(writer)?;
        writeln!(writer, "// Is value type: {}", self.is_value_type)?;
        // Type definition plus inherit lines
        match self.inherit.is_empty() {
            true => writeln!(writer, "struct {} {{", self.cpp_name())?,
            false => writeln!(
                writer,
                "struct {} : {} {{",
                self.cpp_name(),
                self.inherit
                    .iter()
                    .map(|s| format!("public {}", s))
                    .join(", ")
            )?,
        }

        writer.indent();

        self.nested_types
            .iter()
            .try_for_each(|n| writeln!(writer, "struct {};", n.cpp_name))?;

        // Write all declarations within the type here
        self.declarations.iter().for_each(|d| {
            d.write(writer).unwrap();
        });

        self.nested_types
            .iter()
            .try_for_each(|n| n.write_def_internal(writer, None))?;

        // Type complete
        writer.dedent();
        writeln!(writer, "}};")?;

        // NON MEMBER DECLARATIONS
        self.nonmember_declarations
            .iter()
            .try_for_each(|d| d.write(writer))?;

        // Namespace complete
        if let Some(n) = namespace {
            writer.dedent();
            writeln!(writer, "}} // namespace {}", self.cpp_namespace())?;
        }
        // TODO: Write additional meta-info here, perhaps to ensure correct conversions?
        Ok(())
    }
}
