use std::{
    collections::{HashMap, HashSet},
    io::Write,
    rc::Rc,
};

use color_eyre::eyre::Context;

use brocolib::global_metadata::{MethodIndex, TypeIndex};
use itertools::Itertools;

use crate::data::name_components::NameComponents;

use super::{
    context_collection::CppContextCollection,
    cpp_type_tag::CppTypeTag,
    members::{CppForwardDeclare, CppInclude, CppMember, CppNonMember, CppTemplate},
    writer::{CppWriter, Sortable, Writable},
};

pub const CORDL_TYPE_MACRO: &str = "CORDL_TYPE";
pub const __CORDL_IS_VALUE_TYPE: &str = "__IL2CPP_IS_VALUE_TYPE";
pub const __CORDL_BACKING_ENUM_TYPE: &str = "__CORDL_BACKING_ENUM_TYPE";

pub const CORDL_REFERENCE_TYPE_CONSTRAINT: &str = "::il2cpp_utils::il2cpp_reference_type";
pub const CORDL_NUM_ENUM_TYPE_CONSTRAINT: &str = "::cordl_internals::is_or_is_backed_by";
pub const CORDL_METHOD_HELPER_NAMESPACE: &str = "::cordl_internals";

#[derive(Debug, Clone, Default)]
pub struct CppTypeRequirements {
    pub forward_declares: HashSet<(CppForwardDeclare, CppInclude)>,

    // Only value types or classes
    pub required_def_includes: HashSet<CppInclude>,
    pub required_impl_includes: HashSet<CppInclude>,

    // Lists both types we forward declare or include
    pub depending_types: HashSet<CppTypeTag>,
}

impl CppTypeRequirements {
    pub fn add_forward_declare(&mut self, cpp_data: (CppForwardDeclare, CppInclude)) {
        // self.depending_types.insert(cpp_type.self_tag);
        self.forward_declares.insert(cpp_data);
    }

    pub fn add_def_include(&mut self, cpp_type: Option<&CppType>, cpp_include: CppInclude) {
        if let Some(cpp_type) = cpp_type {
            self.depending_types.insert(cpp_type.self_tag);
        }
        self.required_def_includes.insert(cpp_include);
    }
    pub fn add_impl_include(&mut self, cpp_type: Option<&CppType>, cpp_include: CppInclude) {
        if let Some(cpp_type) = cpp_type {
            self.depending_types.insert(cpp_type.self_tag);
        }
        self.required_impl_includes.insert(cpp_include);
    }
    pub fn add_dependency(&mut self, cpp_type: &CppType) {
        self.depending_types.insert(cpp_type.self_tag);
    }
    pub fn add_dependency_tag(&mut self, tag: CppTypeTag) {
        self.depending_types.insert(tag);
    }
}

// Represents all of the information necessary for a C++ TYPE!
// A C# type will be TURNED INTO this
#[derive(Debug, Clone)]
pub struct CppType {
    pub self_tag: CppTypeTag,
    pub nested: bool,

    pub(crate) prefix_comments: Vec<String>,

    pub calculated_size: Option<usize>,
    pub packing: Option<usize>,

    // Computed by TypeDefinition.full_name()
    // Then fixed for generic types in CppContextCollection::make_generic_from/fill_generic_inst
    pub cpp_name_components: NameComponents,
    pub cs_name_components: NameComponents,

    pub declarations: Vec<Rc<CppMember>>,
    pub implementations: Vec<Rc<CppMember>>,
    /// Outside of the class declaration
    /// Move to CsType/CppType?
    pub nonmember_implementations: Vec<Rc<CppNonMember>>,
    pub nonmember_declarations: Vec<Rc<CppNonMember>>,

    pub is_value_type: bool,
    pub is_enum_type: bool,
    pub is_reference_type: bool,
    pub requirements: CppTypeRequirements,

    pub inherit: Vec<String>,
    pub cpp_template: Option<CppTemplate>, // Names of templates e.g T, TKey etc.

    /// contains the array of generic Il2CppType indexes
    pub generic_instantiations_args_types: Option<Vec<usize>>, // GenericArg -> Instantiation Arg
    pub method_generic_instantiation_map: HashMap<MethodIndex, Vec<TypeIndex>>, // MethodIndex -> Generic Args
    pub is_stub: bool,
    pub is_interface: bool,
    pub is_hidden: bool,

    pub nested_types: HashMap<CppTypeTag, CppType>,
}

impl CppTypeRequirements {
    pub fn need_wrapper(&mut self) {
        self.add_def_include(
            None,
            CppInclude::new_exact("beatsaber-hook/shared/utils/base-wrapper-type.hpp"),
        );
    }
    pub fn needs_int_include(&mut self) {
        self.add_def_include(None, CppInclude::new_system("cstdint"));
    }
    pub fn needs_byte_include(&mut self) {
        self.add_def_include(None, CppInclude::new_system("cstddef"));
    }
    pub fn needs_math_include(&mut self) {
        self.add_def_include(None, CppInclude::new_system("cmath"));
    }
    pub fn needs_stringw_include(&mut self) {
        self.add_def_include(
            None,
            CppInclude::new_exact("beatsaber-hook/shared/utils/typedefs-string.hpp"),
        );
    }
    pub fn needs_arrayw_include(&mut self) {
        self.add_def_include(
            None,
            CppInclude::new_exact("beatsaber-hook/shared/utils/typedefs-array.hpp"),
        );
    }

    pub fn needs_byref_include(&mut self) {
        self.add_def_include(
            None,
            CppInclude::new_exact("beatsaber-hook/shared/utils/byref.hpp"),
        );
    }

    pub fn needs_enum_include(&mut self) {
        self.add_def_include(
            None,
            CppInclude::new_exact("beatsaber-hook/shared/utils/enum-type.hpp"),
        );
    }

    pub fn needs_value_include(&mut self) {
        self.add_def_include(
            None,
            CppInclude::new_exact("beatsaber-hook/shared/utils/value-type.hpp"),
        );
    }
}

impl CppType {
    pub fn namespace(&self) -> String {
        self.cs_name_components
            .namespace
            .clone()
            .unwrap_or_default()
    }

    pub fn cpp_namespace(&self) -> String {
        self.cpp_name_components
            .namespace
            .clone()
            .unwrap_or_default()
    }

    pub fn name(&self) -> &String {
        &self.cs_name_components.name
    }

    pub fn cpp_name(&self) -> &String {
        &self.cpp_name_components.name
    }

    pub fn nested_types_flattened(&self) -> HashMap<CppTypeTag, &CppType> {
        self.nested_types
            .iter()
            .flat_map(|(_, n)| n.nested_types_flattened())
            .chain(self.nested_types.iter().map(|(tag, n)| (*tag, n)))
            .collect()
    }
    pub fn get_nested_type_mut(&mut self, tag: CppTypeTag) -> Option<&mut CppType> {
        // sadly
        if self.nested_types.get_mut(&tag).is_some() {
            return self.nested_types.get_mut(&tag);
        }

        self.nested_types.values_mut().find_map(|n| {
            // Recurse
            n.get_nested_type_mut(tag)
        })
    }
    pub fn get_nested_type(&self, tag: CppTypeTag) -> Option<&CppType> {
        self.nested_types.get(&tag).or_else(|| {
            self.nested_types.iter().find_map(|(_, n)| {
                // Recurse
                n.get_nested_type(tag)
            })
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
        let nested_index = self.nested_types.get(&ty);

        match nested_index {
            None => {
                for nested_ty in self.nested_types.values_mut() {
                    if nested_ty.borrow_nested_type_mut(ty, context, func) {
                        return true;
                    }
                }

                false
            }
            Some(old_nested_cpp_type) => {
                // clone to avoid breaking il2cpp
                let old_nested_cpp_type_tag = old_nested_cpp_type.self_tag;
                let new_cpp_type = func(context, old_nested_cpp_type.clone());

                // Remove old type, which may have a new type tag
                self.nested_types.remove(&old_nested_cpp_type_tag);
                self.nested_types
                    .insert(new_cpp_type.self_tag, new_cpp_type);

                true
            }
        }
    }

    pub fn write_impl(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        self.write_impl_internal(writer)
    }

    pub fn write_def(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        self.write_def_internal(writer, Some(&self.cpp_namespace()))
    }

    pub fn write_impl_internal(
        &self,
        writer: &mut super::writer::CppWriter,
    ) -> color_eyre::Result<()> {
        self.nonmember_implementations
            .iter()
            .try_for_each(|d| d.write(writer))?;

        // Write all declarations within the type here
        self.implementations
            .iter()
            .sorted_by(|a, b| a.sort_level().cmp(&b.sort_level()))
            .try_for_each(|d| d.write(writer))?;

        // TODO: Figure out
        self.nested_types
            .iter()
            .try_for_each(|(_tag, n)| n.write_impl_internal(writer))?;

        Ok(())
    }

    fn write_def_internal(
        &self,
        writer: &mut super::writer::CppWriter,
        namespace: Option<&str>,
    ) -> color_eyre::Result<()> {
        self.prefix_comments
            .iter()
            .try_for_each(|pc| writeln!(writer, "// {pc}").context("Prefix comment"))?;

        let type_kind = match self.is_value_type {
            true => "struct",
            false => "class",
        };

        // Just forward declare
        if !self.is_stub {
            if let Some(n) = &namespace {
                writeln!(writer, "namespace {n} {{")?;
                writer.indent();
            }

            // Write type definition
            if let Some(generic_args) = &self.cpp_template {
                writeln!(writer, "// cpp template")?;
                generic_args.write(writer)?;
            }
            writeln!(writer, "// Is value type: {}", self.is_value_type)?;
            writeln!(
                writer,
                "// Dependencies: {:?}",
                self.requirements.depending_types
            )?;
            writeln!(writer, "// Self: {:?}", self.self_tag)?;

            let clazz_name = self
                .cpp_name_components
                .formatted_name(self.generic_instantiations_args_types.is_some());

            writeln!(
                writer,
                "// CS Name: {}",
                self.cs_name_components.combine_all()
            )?;

            // Type definition plus inherit lines

            // add_generic_inst sets template to []
            // if self.generic_instantiation_args.is_some() {
            //     writeln!(writer, "template<>")?;
            // }
            // start writing class
            let cordl_hide = match self.is_hidden {
                true => CORDL_TYPE_MACRO,
                false => "",
            };

            if let Some(packing) = &self.packing {
                writeln!(writer, "#pragma pack(push, {packing})")?;
            }

            match self.inherit.is_empty() {
                true => writeln!(writer, "{type_kind} {cordl_hide} {clazz_name} {{")?,
                false => writeln!(
                    writer,
                    "{type_kind} {cordl_hide} {clazz_name} : {} {{",
                    self.inherit
                        .iter()
                        .map(|s| format!("public {s}"))
                        .join(", ")
                )?,
            }

            writer.indent();

            // add public access
            writeln!(writer, "public:")?;

            self.nested_types
                .values()
                .map(|t| (t, CppForwardDeclare::from_cpp_type(t)))
                .unique_by(|(_, n)| n.clone())
                .try_for_each(|(t, nested_forward_declare)| {
                    writeln!(
                        writer,
                        "// nested type forward declare {} is stub {} {:?} {:?}\n//{:?}",
                        t.cs_name_components.combine_all(),
                        t.is_stub,
                        t.cs_name_components.generics,
                        t.generic_instantiations_args_types,
                        t.self_tag
                    )?;
                    nested_forward_declare.write(writer)
                })?;

            self.nested_types
                .iter()
                .try_for_each(|(_, n)| -> color_eyre::Result<()> {
                    writer.indent();
                    writeln!(
                        writer,
                        "// nested type {} is stub {}",
                        n.cs_name_components.combine_all(),
                        n.is_stub
                    )?;
                    n.write_def_internal(writer, None)?;
                    writer.dedent();
                    Ok(())
                })?;
            writeln!(writer, "// Declarations")?;
            // Write all declarations within the type here
            self.declarations
                .iter()
                .sorted_by(|a, b| a.sort_level().cmp(&b.sort_level()))
                .sorted_by(|a, b| {
                    // fields and unions need to be sorted by offset to work correctly

                    let a_offset = match a.as_ref() {
                        CppMember::FieldDecl(f) => f.offset.clone(),
                        CppMember::NestedUnion(u) => u.offset.clone(),
                        _ => u32::MAX
                    };

                    let b_offset = match b.as_ref() {
                        CppMember::FieldDecl(f) => f.offset.clone(),
                        CppMember::NestedUnion(u) => u.offset.clone(),
                        _ => u32::MAX
                    };

                    a_offset.cmp(&b_offset)
                })
                .try_for_each(|d| -> color_eyre::Result<()> {
                    d.write(writer)?;
                    writeln!(writer)?;
                    Ok(())
                })?;

            writeln!(
                writer,
                "static constexpr bool {__CORDL_IS_VALUE_TYPE} = {};",
                self.is_value_type
            )?;
            // Type complete
            writer.dedent();
            writeln!(writer, "}};")?;

            if self.packing.is_some() {
                writeln!(writer, "#pragma pack(pop)")?;
            }


            // NON MEMBER DECLARATIONS
            writeln!(writer, "// Non member Declarations")?;

            self.nonmember_declarations
                .iter()
                .try_for_each(|d| -> color_eyre::Result<()> {
                    d.write(writer)?;
                    writeln!(writer)?;
                    Ok(())
                })?;

            // Namespace complete
            if let Some(n) = namespace {
                writer.dedent();
                writeln!(writer, "}} // namespace end def {n}")?;
            }
        }

        // TODO: Write additional meta-info here, perhaps to ensure correct conversions?
        Ok(())
    }

    pub fn write_type_trait(&self, writer: &mut CppWriter) -> color_eyre::Result<()> {
        if self.cpp_template.is_some() {
            // generic
            // macros from bs hook
            let type_trait_macro = if self.is_enum_type || self.is_value_type {
                "MARK_GEN_VAL_T"
            } else {
                "MARK_GEN_REF_PTR_T"
            };

            writeln!(
                writer,
                "{type_trait_macro}({});",
                self.cpp_name_components
                    .clone()
                    .remove_generics()
                    .remove_pointer()
                    .combine_all()
            )?;
        } else {
            // non-generic
            // macros from bs hook
            let type_trait_macro = if self.is_enum_type || self.is_value_type {
                "MARK_VAL_T"
            } else {
                "MARK_REF_PTR_T"
            };

            writeln!(
                writer,
                "{type_trait_macro}({});",
                self.cpp_name_components.remove_pointer().combine_all()
            )?;
        }

        Ok(())
    }
}
