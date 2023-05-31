use std::{
    collections::{HashMap, HashSet},
    fs::{create_dir_all, remove_file, File},
    path::{Path, PathBuf},
};

use brocolib::global_metadata::TypeDefinitionIndex;
use color_eyre::eyre::ContextCompat;

use brocolib::runtime_metadata::TypeData;
use itertools::Itertools;

use crate::generate::members::CppInclude;

use super::{
    config::GenerationConfig,
    cpp_type::CppType,
    cs_type::CSType,
    metadata::Metadata,
    writer::{CppWriter, Writable},
};

// Holds the contextual information for creating a C++ file
// Will hold various metadata, such as includes, type definitions, and extraneous writes
#[derive(Debug)]
pub struct CppContext {
    pub typedef_path: PathBuf,
    pub type_impl_path: PathBuf,

    // combined header
    pub fundamental_path: PathBuf,

    // Types to write, typedef
    typedef_types: HashMap<TypeData, CppType>,
}

impl CppContext {
    pub fn get_cpp_type_recursive_mut(
        &mut self,
        root_tag: TypeData,
        child_tag: TypeData,
    ) -> Option<&mut CppType> {
        let ty = self.typedef_types.get_mut(&root_tag);
        if root_tag == child_tag {
            return ty;
        }

        ty.and_then(|ty| ty.get_nested_type_mut(child_tag))
    }
    pub fn get_cpp_type_recursive(
        &self,
        root_tag: TypeData,
        child_tag: TypeData,
    ) -> Option<&CppType> {
        let ty = self.typedef_types.get(&root_tag);
        if root_tag == child_tag {
            return ty;
        }

        ty.and_then(|ty| ty.get_nested_type(child_tag))
    }
    // pub fn get_cpp_type(&mut self, t: TypeData) -> Option<&CppType> {
    //     self.typedef_types.get(&t)
    // }

    pub fn get_include_path(&self) -> &PathBuf {
        &self.typedef_path
    }

    pub fn get_types(&self) -> &HashMap<TypeData, CppType> {
        &self.typedef_types
    }

    // TODO: Move out, this is CSContext
    fn make(
        metadata: &Metadata,
        config: &GenerationConfig,
        tdi: TypeDefinitionIndex,
        tag: TypeData,
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
        };

        if metadata.blacklisted_types.contains(&tdi) {
            println!("Skipping {ns}::{name} ({tdi:?}) because it's blacklisted");
            // TODO: Make this create a struct with matching size or a using statement appropiately
            return x;
        }

        match CppType::make_cpp_type(metadata, config, tag) {
            Some(cpptype) => {
                x.typedef_types
                    .insert(TypeData::TypeDefinitionIndex(tdi), cpptype);
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

        // Write includes for typedef
        self.typedef_types
            .values()
            .flat_map(|t| &t.requirements.required_includes)
            .unique()
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

            CppInclude::new(self.type_impl_path.to_path_buf()).write(&mut typeimpl_writer)?;
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
            t.write_def(&mut typedef_writer)?;
            t.write_impl(&mut typeimpl_writer)?;
        }

        CppInclude::new(self.typedef_path.to_path_buf()).write(&mut fundamental_writer)?;
        CppInclude::new(self.type_impl_path.to_path_buf()).write(&mut fundamental_writer)?;

        // TODO: Write type impl and fundamental files here
        Ok(())
    }
}

pub struct CppContextCollection {
    all_contexts: HashMap<TypeData, CppContext>,
    alias_context: HashMap<TypeData, TypeData>,
    filled_types: HashSet<TypeData>,
    filling_types: HashSet<TypeData>,
}

impl CppContextCollection {
    pub fn fill(&mut self, metadata: &Metadata, config: &GenerationConfig, ty: TypeData) {
        let type_tag: TypeData = ty;
        let tdi = CppType::get_tag_tdi(type_tag);

        assert!(
            !metadata.child_to_parent_map.contains_key(&tdi),
            "Do not fill a child"
        );

        let context_tag = self.get_context_root_tag(type_tag);

        if self.filled_types.contains(&type_tag) {
            return;
        }

        // Move ownership to local
        let cpp_type_entry = self
            .all_contexts
            .get_mut(&context_tag)
            .expect("No cpp context")
            .typedef_types
            .remove_entry(&type_tag);

        self.filling_types.insert(type_tag);

        // In some occasions, the CppContext can be empty
        if let Some((t, mut cpp_type)) = cpp_type_entry {
            assert!(!cpp_type.nested, "Cannot fill a nested type!");

            cpp_type.fill_from_il2cpp(metadata, config, self, tdi);

            // Move ownership back up
            self.all_contexts
                .get_mut(&context_tag)
                .expect("No cpp context")
                .typedef_types
                .insert(t, cpp_type);
        }

        self.filled_types.insert(type_tag);
        self.filling_types.remove(&type_tag);
    }

    fn alias_nested_types(&mut self, owner: &CppType, root_tag: TypeData) {
        for nested_type in &owner.nested_types {
            // println!(
            //     "Aliasing {:?} to {:?}",
            //     nested_type.self_tag, owner.self_tag
            // );
            self.alias_context.insert(nested_type.self_tag, root_tag);
            self.alias_nested_types(nested_type, root_tag);
        }
    }

    pub fn fill_nested_types(
        &mut self,
        metadata: &Metadata,
        config: &GenerationConfig,
        owner_ty: TypeData,
    ) {
        let owner_type_tag = owner_ty;
        let owner = self
            .get_cpp_type_mut(owner_type_tag)
            .unwrap_or_else(|| panic!("Owner does not exist {owner_type_tag:?}"));

        // we clone, then write later
        // since we're modifying only 1 type exclusively
        // and we don't rely on any other type at this time
        // we can clone
        // sad inefficient memory usage but oh well
        let mut nested_types = owner.nested_types.clone();
        nested_types.iter_mut().for_each(|nested_type| {
            let nested_tag = nested_type.self_tag;
            self.filling_types.insert(nested_tag);
            let tdi = CppType::get_tag_tdi(nested_tag);

            nested_type.fill_from_il2cpp(metadata, config, self, tdi);

            self.filled_types.insert(nested_tag);
            self.filling_types.remove(&nested_tag);
        });
        // nested_tags.into_iter().for_each(|nested_tag| {
        //     self.filling_types.insert(nested_tag);

        //     let nested_type = nested_types
        //         .iter_mut()
        //         .find(|n| n.self_tag == nested_tag)
        //         .unwrap();
        //     let tdi = CppType::get_tag_tdi(nested_tag);

        //     nested_type.fill_from_il2cpp(metadata, config, self, tdi);

        //     self.filled_types.insert(nested_tag);
        //     self.filling_types.remove(&nested_tag);
        // });

        self.get_cpp_type_mut(owner_type_tag).unwrap().nested_types = nested_types;
    }

    pub fn get_context_root_tag(&self, ty: TypeData) -> TypeData {
        let tag = ty;
        self.alias_context
            .get(&tag)
            .cloned()
            // .map(|t| self.get_context_root_tag(*t))
            .unwrap_or(tag)
    }

    pub fn make_from(
        &mut self,
        metadata: &Metadata,
        config: &GenerationConfig,
        ty: TypeData,
    ) -> &mut CppContext {
        let type_tag = ty;
        assert!(
            !metadata
                .child_to_parent_map
                .contains_key(&CppType::get_tag_tdi(type_tag)),
            "Cannot create context for nested type",
        );
        let context_root_tag = self.get_context_root_tag(type_tag);

        if self.filling_types.contains(&context_root_tag) {
            panic!("Currently filling type {context_root_tag:?}, cannot fill")
        }

        // Why is the borrow checker so dumb?
        // Using entries causes borrow checker to die :(
        if self.all_contexts.contains_key(&context_root_tag) {
            return self.all_contexts.get_mut(&context_root_tag).unwrap();
        }

        let tdi = CppType::get_tag_tdi(context_root_tag);
        let context = CppContext::make(metadata, config, tdi, context_root_tag);
        // Now do children
        for cpp_type in context.typedef_types.values() {
            self.alias_nested_types(cpp_type, cpp_type.self_tag);
        }
        self.all_contexts.insert(context_root_tag, context);
        self.all_contexts.get_mut(&context_root_tag).unwrap()
        // self.all_contexts
        //     .entry(context_root_tag)
        //     .or_insert_with(|| {
        //         let tdi = CppType::get_tag_tdi(context_root_tag);
        //         let context = CppContext::make(metadata, config, tdi, context_root_tag);
        //         // Now do children
        //         for (_tag, cpp_type) in &context.typedef_types {
        //             self.alias_nested_types(&cpp_type, cpp_type.self_tag);
        //         }
        //         context
        //     })
    }

    pub fn get_cpp_type(&self, ty: TypeData) -> Option<&CppType> {
        let tag = ty;
        let context_root_tag = self.get_context_root_tag(tag);
        self.get_context(context_root_tag)
            .and_then(|c| c.get_cpp_type_recursive(context_root_tag, tag))
    }
    pub fn get_cpp_type_mut(&mut self, ty: TypeData) -> Option<&mut CppType> {
        let tag = ty;
        let context_root_tag = self.get_context_root_tag(tag);
        self.get_context_mut(context_root_tag)
            .and_then(|c| c.get_cpp_type_recursive_mut(context_root_tag, tag))
    }

    pub fn get_context(&self, type_tag: TypeData) -> Option<&CppContext> {
        self.all_contexts.get(&self.get_context_root_tag(type_tag))
    }
    pub fn get_context_mut(&mut self, type_tag: TypeData) -> Option<&mut CppContext> {
        self.all_contexts
            .get_mut(&self.get_context_root_tag(type_tag))
    }

    pub fn new() -> CppContextCollection {
        CppContextCollection {
            all_contexts: Default::default(),
            filled_types: Default::default(),
            filling_types: Default::default(),
            alias_context: Default::default(),
        }
    }
    pub fn get(&self) -> &HashMap<TypeData, CppContext> {
        &self.all_contexts
    }
}
