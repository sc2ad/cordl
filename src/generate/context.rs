use std::cmp::Ordering;
use std::io::Write;
use std::{
    collections::{HashMap, HashSet},
    fs::{create_dir_all, remove_file, File},
    path::{Path, PathBuf},
};

use brocolib::global_metadata::TypeDefinitionIndex;

use color_eyre::eyre::ContextCompat;

use itertools::Itertools;
use log::{info, trace};
use pathdiff::diff_paths;

use crate::generate::cs_type::CORDL_NO_INCLUDE_IMPL_DEFINE;
use crate::generate::members::CppForwardDeclare;
use crate::generate::{members::CppInclude, type_extensions::TypeDefinitionExtensions};
use crate::helpers::sorting::DependencyGraph;
use crate::STATIC_CONFIG;

use super::cpp_type_tag::CppTypeTag;
use super::cs_type::OBJECT_WRAPPER_TYPE;
use super::{
    config::GenerationConfig,
    cpp_type::CppType,
    cs_type::CSType,
    members::CppUsingAlias,
    metadata::Metadata,
    writer::{CppWriter, Writable},
};

// Holds the contextual information for creating a C++ file
// Will hold various metadata, such as includes, type definitions, and extraneous writes
#[derive(Debug, Clone)]
pub struct CppContext {
    pub typedef_path: PathBuf,
    pub type_impl_path: PathBuf,

    // combined header
    pub fundamental_path: PathBuf,

    // Types to write, typedef
    pub typedef_types: HashMap<CppTypeTag, CppType>,

    // Namespace -> alias
    pub typealias_types: HashSet<(String, CppUsingAlias)>,
}

impl CppContext {
    pub fn get_cpp_type_recursive_mut(
        &mut self,
        root_tag: CppTypeTag,
        child_tag: CppTypeTag,
    ) -> Option<&mut CppType> {
        let ty = self.typedef_types.get_mut(&root_tag);
        if root_tag == child_tag {
            return ty;
        }

        ty.and_then(|ty| ty.get_nested_type_mut(child_tag))
    }
    pub fn get_cpp_type_recursive(
        &self,
        root_tag: CppTypeTag,
        child_tag: CppTypeTag,
    ) -> Option<&CppType> {
        let ty = self.typedef_types.get(&root_tag);
        // if a root type
        if root_tag == child_tag {
            return ty;
        }

        ty.and_then(|ty| ty.get_nested_type(child_tag))
    }

    pub fn get_include_path(&self) -> &PathBuf {
        &self.typedef_path
    }

    pub fn get_types(&self) -> &HashMap<CppTypeTag, CppType> {
        &self.typedef_types
    }

    // TODO: Move out, this is CSContext
    pub fn make(
        metadata: &Metadata,
        config: &GenerationConfig,
        tdi: TypeDefinitionIndex,
        tag: CppTypeTag,
        generic_inst: Option<&Vec<usize>>,
    ) -> CppContext {
        let t = &metadata.metadata.global_metadata.type_definitions[tdi];

        let components = t.get_name_components(metadata.metadata);

        let ns = &components.namespace;
        let name = &components.name;

        let cpp_namespace = config.namespace_cpp(ns);
        let cpp_name = config.namespace_cpp(name);

        let ns_path = config.namespace_path(ns);
        let path = if ns_path.is_empty() {
            "GlobalNamespace/".to_string()
        } else {
            ns_path + "/"
        };
        let path_name = match t.declaring_type_index != u32::MAX {
            true => {
                let name = config.path_name(name);
                let base_name = components.declaring_types.join("_");

                format!("{base_name}_{name}")
            }
            false => config.path_name(name),
        };

        let mut x = CppContext {
            typedef_path: config
                .header_path
                .join(format!("{path}zzzz__{path_name}_def.hpp")),
            type_impl_path: config
                .header_path
                .join(format!("{path}zzzz__{path_name}_impl.hpp")),
            fundamental_path: config.header_path.join(format!("{path}{path_name}.hpp")),
            typedef_types: Default::default(),
            typealias_types: Default::default(),
        };

        if metadata.blacklisted_types.contains(&tdi) {
            if !t.is_value_type() {
                x.typealias_types.insert((
                    cpp_namespace,
                    CppUsingAlias {
                        alias: cpp_name.to_string(),
                        result: OBJECT_WRAPPER_TYPE.to_string(),
                        template: Default::default(),
                    },
                ));
            }
            return x;
        }

        match CppType::make_cpp_type(metadata, config, tdi, tag, generic_inst) {
            Some(cpptype) => {
                x.insert_cpp_type(cpptype);
            }
            None => {
                info!(
                    "Unable to create valid CppContext for type: {}!",
                    t.full_name(metadata.metadata, true)
                );
            }
        }

        x
    }

    pub fn insert_cpp_type(&mut self, cpp_type: CppType) {
        if cpp_type.nested {
            panic!(
                "Cannot have a root type as a nested type! {}",
                &cpp_type.cpp_name_components.combine_all(true)
            );
        }
        self.typedef_types.insert(cpp_type.self_tag, cpp_type);
    }

    pub fn write(&self, config: &GenerationConfig) -> color_eyre::Result<()> {
        // Write typedef file first
        if Path::exists(self.typedef_path.as_path()) {
            remove_file(self.typedef_path.as_path())?;
        }
        if !Path::is_dir(
            self.typedef_path
                .parent()
                .context("parent is not a directory!")?,
        ) {
            // Assume it's never a file
            create_dir_all(
                self.typedef_path
                    .parent()
                    .context("Failed to create all directories!")?,
            )?;
        }

        let base_path = &config.header_path;

        trace!("Writing {:?}", self.typedef_path.as_path());
        let mut typedef_writer = CppWriter {
            stream: File::create(self.typedef_path.as_path())?,
            indent: 0,
            newline: true,
        };
        let mut typeimpl_writer = CppWriter {
            stream: File::create(self.type_impl_path.as_path())?,
            indent: 0,
            newline: true,
        };
        let mut fundamental_writer = CppWriter {
            stream: File::create(self.fundamental_path.as_path())?,
            indent: 0,
            newline: true,
        };

        writeln!(typedef_writer, "#pragma once")?;
        writeln!(typeimpl_writer, "#pragma once")?;
        writeln!(fundamental_writer, "#pragma once")?;

        // Include cordl config
        // this is so confusing but basically gets the relative folder
        // navigation for `_config.hpp`
        let dest_path = diff_paths(
            &STATIC_CONFIG.dst_header_internals_file,
            self.typedef_path.parent().unwrap(),
        )
        .unwrap();

        CppInclude::new_exact(dest_path).write(&mut typedef_writer)?;

        // alphabetical sorted
        let typedef_types = self
            .typedef_types
            .values()
            .flat_map(|t: &CppType| -> Vec<&CppType> {
                t.nested_types_flattened().values().copied().collect_vec()
            })
            .chain(self.typedef_types.values())
            .sorted_by(|a, b| a.cpp_name_components.cmp(&b.cpp_name_components))
            // Enums go after stubs
            .sorted_by(|a, b| {
                if a.is_enum_type == b.is_enum_type {
                    return Ordering::Equal;
                }

                if a.is_enum_type {
                    Ordering::Less
                } else if b.is_enum_type {
                    Ordering::Greater
                } else {
                    Ordering::Equal
                }
            })
            // Stubs are first
            .sorted_by(|a, b| {
                if a.is_stub == b.is_stub {
                    return Ordering::Equal;
                }

                if a.is_stub {
                    Ordering::Less
                } else if b.is_stub {
                    Ordering::Greater
                } else {
                    Ordering::Equal
                }
            })
            // Value types are last
            .sorted_by(|a, b| {
                let a_strictly_vt = a.is_value_type && !a.is_enum_type;
                let b_strictly_vt = b.is_value_type && !b.is_enum_type;

                if a_strictly_vt == b_strictly_vt {
                    return Ordering::Equal;
                }

                if a_strictly_vt {
                    Ordering::Greater
                } else if b_strictly_vt {
                    Ordering::Less
                } else {
                    Ordering::Equal
                }
            })
            .collect_vec();

        let typedef_root_types = typedef_types
            .iter()
            .cloned()
            .filter(|t: &&CppType| self.typedef_types.contains_key(&t.self_tag))
            .collect_vec();

        let mut ts = DependencyGraph::<CppTypeTag, _>::new(|a, b| a.cmp(b));
        for cpp_type in &typedef_root_types {
            ts.add_root_dependency(&cpp_type.self_tag);

            for dep in cpp_type.requirements.depending_types.iter().sorted() {
                ts.add_dependency(&cpp_type.self_tag, dep);

                // add dependency for generic instantiations
                // for all types with the same TDI
                if let CppTypeTag::TypeDefinitionIndex(tdi) = dep {
                    // find all generic tags that have the same TDI
                    let generic_tags_in_context =
                        typedef_root_types.iter().filter(|t| match t.self_tag {
                            CppTypeTag::TypeDefinitionIndex(_) => false,
                            CppTypeTag::GenericInstantiation(gen_inst) => gen_inst.tdi == *tdi,
                        });

                    generic_tags_in_context.for_each(|generic_dep| {
                        ts.add_dependency(&cpp_type.self_tag, &generic_dep.self_tag);
                    })
                }
            }
        }

        // types that don't depend on anyone
        // we take these because they get undeterministically sorted
        // and can be first anyways
        let mut undepended_cpp_types = vec![];

        // currently sorted from root to dependencies
        // aka least depended to most depended
        let mut typedef_root_types_sorted = ts
            .topological_sort()
            .into_iter()
            .filter_map(|t| self.typedef_types.get(t))
            .collect_vec();

        // add the items with no dependencies at the tail
        // when reversed these will be first and can be allowed to be first
        typedef_root_types_sorted.append(&mut undepended_cpp_types);
        // typedef_root_types_sorted.reverse();

        // Write includes for typedef
        typedef_types
            .iter()
            .flat_map(|t| &t.requirements.required_includes)
            .unique()
            .sorted()
            .try_for_each(|i| i.write(&mut typedef_writer))?;

        // Write includes for typeimpl
        typedef_types
            .iter()
            .flat_map(|t| &t.requirements.required_impl_includes)
            .unique()
            .sorted()
            .try_for_each(|i| i.write(&mut typeimpl_writer))?;

        // anonymous namespace
        if STATIC_CONFIG.use_anonymous_namespace {
            writeln!(typedef_writer, "namespace {{")?;
            writeln!(typeimpl_writer, "namespace {{")?;
        }

        // write forward declares
        // and includes for impl
        {
            CppInclude::new_exact(diff_paths(&self.typedef_path, base_path).unwrap())
                .write(&mut typeimpl_writer)?;

            let forward_declare_and_includes = || {
                typedef_types
                    .iter()
                    .flat_map(|t| &t.requirements.forward_declares)
            };

            forward_declare_and_includes()
                .map(|(_fd, inc)| inc)
                .unique()
                // TODO: Check forward declare is not of own type
                .try_for_each(|i| -> color_eyre::Result<()> {
                    writeln!(typeimpl_writer, "#ifndef {CORDL_NO_INCLUDE_IMPL_DEFINE}")?;
                    i.write(&mut typeimpl_writer)?;
                    writeln!(typeimpl_writer, "#endif")?;
                    Ok(())
                })?;

            forward_declare_and_includes()
                .map(|(fd, _inc)| fd)
                .unique()
                .try_for_each(|fd| fd.write(&mut typedef_writer))?;

            writeln!(typedef_writer, "// Forward declare root types")?;
            //Forward declare all types
            typedef_root_types
                .iter()
                .map(|t| CppForwardDeclare::from_cpp_type(t))
                // TODO: Check forward declare is not of own type
                .try_for_each(|fd| {
                    // Forward declare and include
                    fd.write(&mut typedef_writer)
                })?;

            writeln!(typedef_writer, "// Write type traits")?;
            typedef_root_types
                .iter()
                .try_for_each(|cpp_type| -> color_eyre::Result<()> {
                    if cpp_type.generic_instantiations_args_types.is_none() {
                        cpp_type.write_type_trait(&mut typedef_writer)?;
                    }
                    Ok(())
                })?;

            // This is likely not necessary
            // self.typedef_types
            //     .values()
            //     .flat_map(|t| &t.requirements.forward_declares)
            //     .map(|(_, i)| i)
            //     .unique()
            //     // TODO: Check forward declare is not of own type
            //     .try_for_each(|i| i.write(&mut typeimpl_writer))?;
        }

        for t in &typedef_root_types_sorted {
            if t.nested {
                panic!(
                    "Cannot have a root type as a nested type! {}",
                    &t.cpp_name_components.combine_all(true)
                );
            }
            // if t.generic_instantiation_args.is_none() || true {
            //     t.write_def(&mut typedef_writer)?;
            //     t.write_impl(&mut typeimpl_writer)?;
            // } else {
            //     t.write_def(&mut typeimpl_writer)?;
            //     t.write_impl(&mut typeimpl_writer)?;
            // }

            t.write_def(&mut typedef_writer)?;
            t.write_impl(&mut typeimpl_writer)?;
        }

        // end anonymous namespace
        if STATIC_CONFIG.use_anonymous_namespace {
            writeln!(typedef_writer, "}} // end anonymous namespace")?;
            writeln!(typeimpl_writer, "}} // end anonymous namespace")?;
        }

        // write macros
        typedef_types
            .iter()
            .try_for_each(|t| Self::write_il2cpp_arg_macros(t, &mut typedef_writer))?;

        // Fundamental
        {
            CppInclude::new_exact(diff_paths(&self.typedef_path, base_path).unwrap())
                .write(&mut fundamental_writer)?;
            CppInclude::new_exact(diff_paths(&self.type_impl_path, base_path).unwrap())
                .write(&mut fundamental_writer)?;
        }

        // TODO: Write type impl and fundamental files here
        Ok(())
    }

    fn write_il2cpp_arg_macros(
        ty: &CppType,
        writer: &mut super::writer::CppWriter,
    ) -> color_eyre::Result<()> {
        let is_generic_instantiation = ty.generic_instantiations_args_types.is_some();
        if is_generic_instantiation {
            return Ok(());
        }

        let template_container_type = ty.is_stub
            || ty
                .cpp_template
                .as_ref()
                .is_some_and(|t| !t.names.is_empty());

        if !ty.is_value_type && !ty.is_stub && !template_container_type && !is_generic_instantiation
        {
            // reference types need no boxing
            writeln!(
                writer,
                "NEED_NO_BOX({});",
                ty.cpp_name_components.combine_all(false)
            )?;
        }

        if ty.nested {
            writeln!(
                writer,
                "// TODO: Nested type, check correct definition print!"
            )?;
        }

        let macro_arg_define = {
            match //ty.generic_instantiation_args.is_some() ||
                    template_container_type {
                    true => match ty.is_value_type {
                        true => "DEFINE_IL2CPP_ARG_TYPE_GENERIC_STRUCT",
                        false => "DEFINE_IL2CPP_ARG_TYPE_GENERIC_CLASS",
                    },
                    false => "DEFINE_IL2CPP_ARG_TYPE",
                }
        };

        // Essentially splits namespace.foo/nested_foo into (namespace, foo/nested_foo)

        let namespace = &ty.cs_name_components.namespace;
        let combined_name = match ty.cs_name_components.declaring_types.is_empty() {
            true => ty.cs_name_components.name.clone(),
            false => format!(
                "{}/{}",
                ty.cs_name_components.declaring_types.join("/"),
                ty.cs_name_components.name.clone()
            ),
        };

        writeln!(
            writer,
            "{macro_arg_define}({}, \"{namespace}\", \"{combined_name}\");",
            ty.cpp_name_components.combine_all(false)
        )?;

        Ok(())
    }
}
