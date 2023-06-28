use std::{
    collections::{HashMap, HashSet},
    io::Write,
    rc::Rc,
};

use color_eyre::eyre::Context;

use brocolib::global_metadata::{TypeDefinitionIndex, TypeIndex};
use itertools::Itertools;

use super::{
    context::{CppContextCollection, CppTypeTag},
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
    pub self_tag: CppTypeTag,
    pub tdi: TypeDefinitionIndex,
    pub nested: bool,

    pub(crate) prefix_comments: Vec<String>,
    pub(crate) namespace: String,
    pub(crate) cpp_namespace: String,
    pub(crate) name: String,
    pub(crate) cpp_name: String,

    pub(crate) parent_ty_tdi: Option<TypeDefinitionIndex>,
    pub(crate) parent_ty_cpp_name: Option<String>,

    pub cpp_full_name: String,

    pub declarations: Vec<CppMember>,
    pub implementations: Vec<CppMember>,
    /// Outside of the class declaration
    /// Move to CsType/CppType?
    pub nonmember_implementations: Vec<Rc<dyn Writable>>,
    pub nonmember_declarations: Vec<Rc<dyn Writable>>,

    pub is_value_type: bool,
    pub requirements: CppTypeRequirements,

    pub inherit: Vec<String>,
    pub cpp_template: Option<CppTemplate>, // Names of templates e.g T, TKey etc.

    pub generic_instantiations_args_types: Option<Vec<TypeIndex>>, // GenericArg -> Instantiation Arg
    pub generic_instantiation_args: Option<Vec<String>>, // generic_instantiations_args_types but formatted
    pub is_stub: bool,

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

    pub fn needs_byref_include(&mut self) {
        self.required_includes
            .insert(CppInclude::new("beatsaber-hook/shared/utils/byref".into()));
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

    pub fn nested_types_flattened(&self) -> HashMap<CppTypeTag, &CppType> {
        self.nested_types
            .iter()
            .flat_map(|n| n.nested_types_flattened())
            .chain(self.nested_types.iter().map(|n| (n.self_tag, n)))
            .collect()
    }
    pub fn get_nested_type_mut(&mut self, tag: CppTypeTag) -> Option<&mut CppType> {
        self.nested_types.iter_mut().find_map(|n| {
            if n.self_tag == tag {
                return Some(n);
            }

            // Recurse
            n.get_nested_type_mut(tag)
        })
    }
    pub fn get_nested_type(&self, tag: CppTypeTag) -> Option<&CppType> {
        self.nested_types.iter().find_map(|n| {
            if n.self_tag == tag {
                return Some(n);
            }

            // Recurse
            n.get_nested_type(tag)
        })
    }

    pub fn borrow_nested_type_mut<F>(
        &mut self,
        ty: CppTypeTag,
        context: &mut CppContextCollection,
        func: &F,
    ) -> bool
    where
        F: Fn(&mut CppContextCollection, CppType) -> CppType,
    {
        let nested_index = self.nested_types.iter().position(|n| n.self_tag == ty);

        match nested_index {
            None => {
                for nested_ty in &mut self.nested_types {
                    if nested_ty.borrow_nested_type_mut(ty, context, func) {
                        return true;
                    }
                }

                false
            }
            Some(index) => {
                // clone to avoid breaking il2cpp
                let nested_cpp_type = self.nested_types.get(index).unwrap().clone();
                let new_cpp_type = func(context, nested_cpp_type);
                self.nested_types.remove(index);
                self.nested_types.insert(index, new_cpp_type);

                true
            }
        }

        // let (nested_owner, index) = self
        //     .nested_types
        //     .iter_mut()
        //     .find_map(|n| {
        //         let nested_ty_index = n.nested_types.iter().position(ty);
        //         if let Some(i) = nested_ty_index {
        //             return Some((n, i));
        //         }

        //         // Recurse
        //         n.get_nested_type_mut(ty)
        //     })
        //     .unwrap();

        // let index = self
        //     .nested_types
        //     .iter()
        //     .position(|t| t.self_tag == ty)
        //     .unwrap();
    }

    pub fn formatted_complete_cpp_name(&self) -> &String {
        return &self.cpp_full_name;
        // We found a valid type that we have defined for this idx!
        // TODO: We should convert it here.
        // Ex, if it is a generic, convert it to a template specialization
        // If it is a normal type, handle it accordingly, etc.
        // match &self.parent_ty_cpp_name {
        //     Some(parent_ty) => {
        //         format!("{}::{parent_ty}::{}", self.cpp_namespace(), self.cpp_name())
        //     }
        //     None => format!("{}::{}", self.cpp_namespace(), self.cpp_name()),
        // }
    }

    pub fn write_impl(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        self.write_impl_internal(writer, Some(self.cpp_namespace()))
    }

    pub fn write_def(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        self.write_def_internal(writer, Some(self.cpp_namespace()), true)
    }

    pub fn write_impl_internal(
        &self,
        writer: &mut super::writer::CppWriter,
        namespace: Option<&str>,
    ) -> color_eyre::Result<()> {
        if let Some(namespace) = namespace {
            writeln!(writer, "namespace {namespace} {{")?;
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
            .try_for_each(|n| n.write_impl_internal(writer, None))?;

        if let Some(namespace) = namespace {
            writeln!(writer, "}} // end namespace {namespace}")?;
        }

        Ok(())
    }

    fn write_def_internal(
        &self,
        writer: &mut super::writer::CppWriter,
        namespace: Option<&str>,
        fd: bool,
    ) -> color_eyre::Result<()> {
        self.prefix_comments.iter().for_each(|pc| {
            writeln!(writer, "// {pc}")
                .context("Prefix comment")
                .unwrap();
        });

        // forward declare self
        {
            if fd {
                writeln!(
                    writer,
                    "// Forward declaring type: {}::{}",
                    namespace.unwrap_or(""),
                    self.name()
                )?;
            }

            if let Some(n) = &namespace {
                writeln!(writer, "namespace {n} {{",)?;
                writer.indent();
            }

            if fd && let Some(generic_args) = &self.cpp_template {
                // template<...>
                generic_args.write(writer)?;
                writeln!(writer, "struct {};", self.name())?;
            }
        }

        // Just forward declare
        if !self.is_stub {
            // Write type definition
            if let Some(generic_args) = &self.cpp_template {
                generic_args.write(writer)?;
            }
            writeln!(writer, "// Is value type: {}", self.is_value_type)?;
            // Type definition plus inherit lines

            let clazz_name = match &self.generic_instantiation_args {
                Some(literals) => format!(
                    "/* generic here lets go! */{}<{}>",
                    self.cpp_name(),
                    literals.join(",")
                ),
                None => self.cpp_name().to_string(),
            };

            if self.generic_instantiation_args.is_some() {
                writeln!(writer, "template<>")?;
            }

            match self.inherit.is_empty() {
                true => writeln!(writer, "struct {clazz_name} {{")?,
                false => writeln!(
                    writer,
                    "struct {clazz_name} : {} {{",
                    self.inherit
                        .iter()
                        .map(|s| format!("public {s}"))
                        .join(", ")
                )?,
            }

            writer.indent();

            self.nested_types
                .iter()
                .map(|n| &n.cpp_name)
                .unique()
                .try_for_each(|cpp_name| {
                    writeln!(
                        writer,
                        "// Forward declare nested type\nstruct {};",
                        cpp_name
                    )
                })?;

            self.nested_types
                .iter()
                .try_for_each(|n| n.write_def_internal(writer, None, false))?;
            // Write all declarations within the type here
            self.declarations.iter().for_each(|d| {
                d.write(writer).unwrap();
            });

            writeln!(
                writer,
                "static constexpr bool __CORDL_IS_VALUE_TYPE = {};",
                self.is_value_type
            )?;
            // Type complete
            writer.dedent();
            writeln!(writer, "}};")?;

            // NON MEMBER DECLARATIONS
            self.nonmember_declarations
                .iter()
                .try_for_each(|d| d.write(writer))?;
        }

        // Namespace complete
        if let Some(n) = namespace {
            writer.dedent();
            writeln!(writer, "}} // namespace {n}")?;
        }
        // TODO: Write additional meta-info here, perhaps to ensure correct conversions?
        Ok(())
    }
}
