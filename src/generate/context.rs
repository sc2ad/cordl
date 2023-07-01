use core::panic;
use std::io::Write;
use std::{
    collections::{HashMap, HashSet},
    fs::{create_dir_all, remove_file, File},
    path::{Path, PathBuf},
};

use brocolib::global_metadata::TypeDefinitionIndex;
use color_eyre::eyre::ContextCompat;

use brocolib::runtime_metadata::TypeData;
use itertools::Itertools;
use pathdiff::diff_paths;

use crate::generate::{
    constants::{TypeDefinitionExtensions, OBJECT_WRAPPER_TYPE},
    members::CppInclude,
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

    pub typealias_types: HashSet<CppUsingAlias>,
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

        let ns_path = config.namespace_path(ns);
        let path = if ns_path.is_empty() {
            "GlobalNamespace/".to_string()
        } else {
            ns_path + "/"
        };
        let mut x = CppContext {
            typedef_path: config.header_path.join(format!(
                "{}__{}_def.hpp",
                path,
                &config.path_name(name)
            )),
            type_impl_path: config.header_path.join(format!(
                "{}__{}_impl.hpp",
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
                x.typealias_types.insert(CppUsingAlias {
                    alias: name.to_string(),
                    result: OBJECT_WRAPPER_TYPE.to_string(),
                    namespaze: if ns.is_empty() {
                        None
                    } else {
                        Some(ns.to_string())
                    },
                    template: Default::default(),
                });
            }
            // TODO: Make this create a struct with matching size or a using statement appropiately
            return x;
        }

        match CppType::make_cpp_type(metadata, config, tag, tdi) {
            Some(cpptype) => {
                x.typedef_types.insert(tag, cpptype);
            }
            None => {
                println!("Unable to create valid CppContext for type: {ns}::{name}!");
            }
        }

        x
    }

    pub fn write(&self) -> color_eyre::Result<()> {
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

        let base_path = &STATIC_CONFIG.header_path;

        // Write includes for typedef
        self.typedef_types
            .values()
            .flat_map(|t| &t.requirements.required_includes)
            .unique()
            .sorted()
            .try_for_each(|i| i.write(&mut typedef_writer))?;

        // write forward declares
        {
            self.typedef_types
                .values()
                .flat_map(|t| &t.requirements.forward_declares)
                .map(|(d, _)| d)
                .unique()
                // TODO: Check forward declare is not of own type
                .try_for_each(|i| i.write(&mut typedef_writer))?;

            CppInclude::new(diff_paths(&self.typedef_path, base_path).unwrap())
                .write(&mut typeimpl_writer)?;
            // This is likely not necessary
            // self.typedef_types
            //     .values()
            //     .flat_map(|t| &t.requirements.forward_declares)
            //     .map(|(_, i)| i)
            //     .unique()
            //     // TODO: Check forward declare is not of own type
            //     .try_for_each(|i| i.write(&mut typeimpl_writer))?;
        }

        for t in self.typedef_types.values() {
            if t.nested {
                continue;
            }
            if t.generic_instantiation_args.is_none() {
                t.write_def(&mut typedef_writer)?;
                t.write_impl(&mut typeimpl_writer)?;
            } else {
                t.write_def(&mut typeimpl_writer)?;
                t.write_impl(&mut typeimpl_writer)?;
            }
        }

        CppInclude::new(diff_paths(&self.typedef_path, base_path).unwrap())
            .write(&mut fundamental_writer)?;
        CppInclude::new(diff_paths(&self.type_impl_path, base_path).unwrap())
            .write(&mut fundamental_writer)?;

        // TODO: Write type impl and fundamental files here
        Ok(())
    }
}
