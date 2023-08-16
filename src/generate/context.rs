use std::cmp::Ordering;
use std::io::Write;
use std::{
    collections::{HashMap, HashSet},
    fs::{create_dir_all, remove_file, File},
    path::{Path, PathBuf},
};

use brocolib::global_metadata::TypeDefinitionIndex;
use brocolib::runtime_metadata::TypeData;
use color_eyre::eyre::ContextCompat;

use itertools::Itertools;
use pathdiff::diff_paths;
use topological_sort::TopologicalSort;

use crate::generate::members::CppForwardDeclare;
use crate::generate::{
    members::CppInclude,
    type_extensions::{TypeDefinitionExtensions, OBJECT_WRAPPER_TYPE},
};
use crate::STATIC_CONFIG;

use super::context_collection::CppTypeTag;
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
    ) -> CppContext {
        let t = &metadata.metadata.global_metadata.type_definitions[tdi];
        let ns = t.namespace(metadata.metadata);
        let name = t.name(metadata.metadata);

        let cpp_namespace = config.namespace_cpp(ns);
        let _cpp_name = config.namespace_cpp(name);

        let ns_path = config.namespace_path(ns);
        let path = if ns_path.is_empty() {
            "GlobalNamespace/".to_string()
        } else {
            ns_path + "/"
        };
        let mut x = CppContext {
            typedef_path: config.header_path.join(format!(
                "{}zzzz__{}_def.hpp",
                path,
                &config.path_name(name)
            )),
            type_impl_path: config.header_path.join(format!(
                "{}zzzz__{}_impl.hpp",
                path,
                &config.path_name(name)
            )),
            fundamental_path: config.header_path.join(format!(
                "{}{}.hpp",
                path,
                &config.path_name(name)
            )),
            typedef_types: Default::default(),
            typealias_types: Default::default(),
        };

        if metadata.blacklisted_types.contains(&tdi) {
            println!("Skipping {ns}::{name} ({tdi:?}) because it's blacklisted");
            if !t.is_value_type() {
                x.typealias_types.insert((
                    cpp_namespace,
                    CppUsingAlias {
                        alias: name.to_string(),
                        result: OBJECT_WRAPPER_TYPE.to_string(),
                        template: Default::default(),
                    },
                ));
            }
            // TODO: Make this create a struct with matching size or a using statement appropiately
            return x;
        }

        match CppType::make_cpp_type(metadata, config, tag, tdi) {
            Some(cpptype) => {
                x.insert_cpp_type(cpptype);
            }
            None => {
                println!("Unable to create valid CppContext for type: {ns}::{name}!");
            }
        }

        x
    }

    pub fn insert_cpp_type(&mut self, cpp_type: CppType) {
        if cpp_type.nested {
            panic!(
                "Cannot have a root type as a nested type! {}",
                &cpp_type.cpp_full_name
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

        println!("Writing {:?}", self.typedef_path.as_path());
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
        let base_path = &config.header_path;

        // Include cordl config
        // this is so confusing but basically gets the relative folder
        // navigation for `_config.hpp`
        let dest_path = diff_paths(
            &STATIC_CONFIG.dest_header_config_file,
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
            .sorted_by(|a, b| a.cpp_full_name.cmp(&b.cpp_full_name))
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
            .filter(|t| !t.nested)
            .collect_vec();

        let mut ts = TopologicalSort::<CppTypeTag>::new();
        for cpp_type in &typedef_root_types {
            for d in &cpp_type.requirements.depending_types {
                ts.add_dependency(*d, cpp_type.self_tag)
            }
        }

        let mut typedef_root_types_sorted =
            ts.filter_map(|t| self.typedef_types.get(&t)).collect_vec();

        // we want from most depended to least depended
        typedef_root_types_sorted.reverse();

        // Write includes for typedef
        typedef_types
            .iter()
            .flat_map(|t| &t.requirements.required_includes)
            .unique()
            .sorted()
            .try_for_each(|i| i.write(&mut typedef_writer))?;

        // write forward declares
        // and includes for impl
        {
            CppInclude::new_exact(diff_paths(&self.typedef_path, base_path).unwrap())
                .write(&mut typeimpl_writer)?;

            typedef_types
                .iter()
                .flat_map(|t| &t.requirements.forward_declares)
                .unique()
                // TODO: Check forward declare is not of own type
                .try_for_each(|(fd, i)| {
                    // Forward declare and include
                    i.write(&mut typeimpl_writer)?;
                    fd.write(&mut typedef_writer)
                })?;

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
                    &t.cpp_full_name
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

        // write macros
        typedef_types
            .iter()
            .try_for_each(|t| Self::write_il2cpp_arg_macros(t, &mut typedef_writer))?;

        CppInclude::new_exact(diff_paths(&self.typedef_path, base_path).unwrap())
            .write(&mut fundamental_writer)?;
        CppInclude::new_exact(diff_paths(&self.type_impl_path, base_path).unwrap())
            .write(&mut fundamental_writer)?;

        // TODO: Write type impl and fundamental files here
        Ok(())
    }

    fn write_il2cpp_arg_macros(
        ty: &CppType,
        writer: &mut super::writer::CppWriter,
    ) -> color_eyre::Result<()> {
        if !ty.is_value_type && !ty.is_stub {
            // reference types need no boxing
            writeln!(writer, "NEED_NO_BOX(::{});", ty.cpp_full_name)?;
        }

        if ty.nested {
            writeln!(
                writer,
                "// TODO: Nested type, check correct definition print!"
            )?;
        }

        let macro_arg_define = {
            match //ty.generic_instantiation_args.is_some() ||  
                    ty.is_stub  {
                    true => match ty.is_value_type {
                        true => "DEFINE_IL2CPP_ARG_TYPE_GENERIC_STRUCT",
                        false => "DEFINE_IL2CPP_ARG_TYPE_GENERIC_CLASS",
                    },
                    false => "DEFINE_IL2CPP_ARG_TYPE",
                }
        };

        // Essentially splits namespace.foo/nested_foo into (namespace, foo/nested_foo)
        let (namespace, name) = match ty.full_name.rsplit_once("::") {
            Some((declaring, name)) => {
                // (namespace, declaring/foo)
                let (namespace, declaring_name) =
                    declaring.rsplit_once('.').unwrap_or(("", declaring));

                let fixed_declaring_name = declaring_name.replace("::", "/");

                (namespace, format!("{fixed_declaring_name}/{name}"))
            }
            None => {
                let (namespace, name) = ty
                    .full_name
                    .rsplit_once('.')
                    .unwrap_or(("", ty.full_name.as_str()));

                (namespace, name.to_string())
            }
        };

        writeln!(
            writer,
            "{macro_arg_define}(::{}, \"{namespace}\", \"{name}\");",
            ty.cpp_full_name,
        )?;

        Ok(())
    }
}
