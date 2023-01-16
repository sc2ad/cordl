use std::{
    collections::{HashMap, HashSet},
    fs::{create_dir_all, remove_file, File},
    path::{Path, PathBuf},
};

use color_eyre::eyre::ContextCompat;

use il2cpp_binary::TypeData;
use il2cpp_metadata_raw::TypeDefinitionIndex;
use itertools::Itertools;

use crate::generate::members::CppInclude;

use super::{
    config::GenerationConfig,
    cpp_type::{self, CppType},
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
    typedef_types: HashMap<TypeTag, CppType>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypeTag {
    TypeDefinition(u32),
    Type(usize),
    GenericParameter(u32),
    GenericClass(usize),
    Array,
}

impl From<TypeData> for TypeTag {
    fn from(ty: TypeData) -> TypeTag {
        match ty {
            TypeData::TypeDefinitionIndex(tdi) => TypeTag::TypeDefinition(tdi),
            TypeData::TypeIndex(ti) => TypeTag::Type(ti),
            TypeData::GenericClassIndex(gci) => TypeTag::GenericClass(gci),
            TypeData::GenericParameterIndex(gpi) => TypeTag::GenericParameter(gpi),
            TypeData::ArrayType => TypeTag::Array,
        }
    }
}

impl From<TypeTag> for TypeData {
    fn from(ty: TypeTag) -> TypeData {
        match ty {
            TypeTag::TypeDefinition(tdi) => TypeData::TypeDefinitionIndex(tdi),
            TypeTag::Type(ti) => TypeData::TypeIndex(ti),
            TypeTag::GenericClass(gci) => TypeData::GenericClassIndex(gci),
            TypeTag::GenericParameter(gpi) => TypeData::GenericParameterIndex(gpi),
            TypeTag::Array => TypeData::ArrayType,
        }
    }
}

impl CppContext {
    pub fn get_cpp_type_mut(&mut self, t: TypeTag) -> Option<&mut CppType> {
        self.typedef_types.get_mut(&t)
    }
    // pub fn get_cpp_type(&mut self, t: TypeTag) -> Option<&CppType> {
    //     self.typedef_types.get(&t)
    // }

    pub fn get_include_path(&self) -> &PathBuf {
        &self.typedef_path
    }

    pub fn get_types(&self) -> &HashMap<TypeTag, CppType> {
        &self.typedef_types
    }

    // TODO: Move out, this is CSContext
    fn make(
        metadata: &Metadata,
        config: &GenerationConfig,
        tdi: TypeDefinitionIndex,
        tag: impl Into<TypeTag>,
    ) -> CppContext {
        let t = metadata
            .metadata
            .type_definitions
            .get(tdi as usize)
            .unwrap();
        let ns = metadata.metadata.get_str(t.namespace_index).unwrap();
        let name = metadata.metadata.get_str(t.name_index).unwrap();

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
        match CppType::make_cpp_type(metadata, config, tag) {
            Some(cpptype) => {
                x.typedef_types
                    .insert(TypeTag::TypeDefinition(tdi), cpptype);
            }
            None => {
                println!(
                    "Unable to create valid CppContext for type: {}::{}!",
                    ns, name
                );
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
    all_contexts: HashMap<TypeTag, CppContext>,
    alias_context: HashMap<TypeTag, TypeTag>,
    filled_types: HashSet<TypeTag>,
    filling_types: HashSet<TypeTag>,
}

impl CppContextCollection {
    pub fn fill(&mut self, metadata: &Metadata, config: &GenerationConfig, ty: impl Into<TypeTag>) {
        let type_tag: TypeTag = ty.into();
        let context_tag = self.get_context_tag(type_tag);

        if self.filled_types.contains(&type_tag) {
            return;
        }

        self.make_from(metadata, config, type_tag);

        let tdi = CppType::get_tag_tdi(type_tag);

        let cpp_type_entry = self
            .all_contexts
            .get_mut(&context_tag)
            .expect("No cpp context")
            .typedef_types
            .remove_entry(&type_tag);
        self.filling_types.insert(type_tag);

        if let Some((t, mut cpp_type)) = cpp_type_entry {
            cpp_type.fill_from_il2cpp(metadata, config, self, tdi);

            // Now do children

            self.fill_nested_types(metadata, config, type_tag, &mut cpp_type);

            self.all_contexts
                .get_mut(&context_tag)
                .expect("No cpp context")
                .typedef_types
                .insert(t, cpp_type);
        }

        self.filled_types.insert(type_tag);
        self.filling_types.remove(&type_tag);
    }

    fn fill_nested_types(
        &mut self,
        metadata: &Metadata,
        config: &GenerationConfig,
        ty: impl Into<TypeTag>,
        owner: &mut CppType,
    ) {
        let type_tag = ty.into();

        let nested_tags = owner
            .nested_types
            .iter()
            .map(|n| n.self_tag.clone())
            .collect_vec();

        nested_tags.into_iter().for_each(|nested_tag| {
            self.filling_types.insert(nested_tag);
            self.alias_context.insert(nested_tag, type_tag);

            {
                let index = owner
                    .nested_types
                    .iter()
                    .position(|n| n.self_tag == nested_tag)
                    .unwrap();
                let mut nested_type = owner.nested_types.remove(index);
                let tdi = CppType::get_tag_tdi(nested_tag);

                nested_type.fill_from_il2cpp(metadata, config, self, tdi);

                self.fill_nested_types(metadata, config, nested_tag, &mut nested_type);
                owner.nested_types.insert(index, nested_type);
            }
            self.filled_types.insert(nested_tag);
            self.filling_types.remove(&nested_tag);
        });
    }

    pub fn get_context_tag(&self, ty: impl Into<TypeTag>) -> TypeTag {
        let tag = ty.into();
        self.alias_context
            .get(&tag)
            .map(|t| self.get_context_tag(*t))
            .unwrap_or(tag)
    }

    pub fn make_from(
        &mut self,
        metadata: &Metadata,
        config: &GenerationConfig,
        ty: impl Into<TypeTag>,
    ) -> &mut CppContext {
        let type_tag = ty.into();
        let context_tag = self.get_context_tag(type_tag);

        if self.filling_types.contains(&context_tag) {
            panic!("Currently filling type {context_tag:?}, cannot fill")
        }

        self.all_contexts.entry(context_tag).or_insert_with(|| {
            let tdi = CppType::get_tag_tdi(context_tag);
            CppContext::make(metadata, config, tdi, context_tag)
        })
    }

    pub fn get_cpp_type(
        &mut self,
        metadata: &Metadata,
        config: &GenerationConfig,
        ty: impl Into<TypeTag>,
    ) -> Option<&mut CppType> {
        let tag = ty.into();
        let context_tag = self.get_context_tag(tag);
        let context = self.make_from(metadata, config, tag);

        match context.typedef_types.get_mut(&context_tag) {
            Some(context_ty) => {
                if context_ty.self_tag == tag {
                    return Some(context_ty);
                }

                context_ty.get_nested_type_mut(tag)
            }
            None => None,
        }
    }

    pub fn get_context(&self, type_tag: TypeTag) -> Option<&CppContext> {
        self.all_contexts.get(&type_tag)
    }

    pub fn new() -> CppContextCollection {
        CppContextCollection {
            all_contexts: Default::default(),
            filled_types: Default::default(),
            filling_types: Default::default(),
            alias_context: Default::default(),
        }
    }
    pub fn get(&self) -> &HashMap<TypeTag, CppContext> {
        &self.all_contexts
    }
}
