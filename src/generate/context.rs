use std::{
    collections::{hash_map::Entry, HashMap, HashSet},
    fs::{create_dir_all, remove_file, File},
    io::Write,
    path::{Path, PathBuf},
};

use color_eyre::eyre::ContextCompat;
use color_eyre::Result;
use il2cpp_binary::TypeData;
use il2cpp_metadata_raw::TypeDefinitionIndex;

use super::{
    config::GenerationConfig,
    cpp_type::{self, CppType},
    members::CppInclude,
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

    // IDK
    includes: HashSet<CppInclude>,
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
    pub fn get_cpp_type(&mut self, t: TypeTag) -> Option<&CppType> {
        self.typedef_types.get(&t)
    }

    pub fn get_include_path(&self) -> &PathBuf {
        &self.typedef_path
    }

    fn make(
        metadata: &Metadata,
        config: &GenerationConfig,
        tdi: TypeDefinitionIndex,
    ) -> CppContext {
        let t = metadata
            .metadata
            .type_definitions
            .get(tdi as usize)
            .unwrap();
        let ns = metadata.metadata.get_str(t.namespace_index).unwrap();
        let name = metadata.metadata.get_str(t.name_index).unwrap();

        let ns_path = config.namespace_path(ns.to_string());
        let path = if ns_path.is_empty() {
            "GlobalNamespace/".to_string()
        } else {
            ns_path + "/"
        };
        let mut x = CppContext {
            typedef_path: config.header_path.join(format!(
                "{}__{}_def.hpp",
                path,
                &config.path_name(name.to_string())
            )),
            type_impl_path: config.header_path.join(format!(
                "{}__{}_impl.hpp",
                path,
                &config.path_name(name.to_string())
            )),
            fundamental_path: config.header_path.join(format!(
                "{}{}.hpp",
                path,
                &config.path_name(name.to_string())
            )),
            typedef_types: Default::default(),
            includes: Default::default(),
        };
        if let Some(cpptype) = CppType::make(metadata, config, tdi) {
            x.typedef_types
                .insert(TypeTag::TypeDefinition(tdi), cpptype);
        } else {
            println!("Unable to create valid CppContext for type: {t:?}!");
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
        let _fundamental_writer = CppWriter {
            stream: File::create(self.fundamental_path.as_path())?,
            indent: 0,
            newline: true,
        };

        // Write includes for typedef
        for (_, t) in &self.typedef_types {
            t.write_def(&mut typedef_writer)?;
            t.write_impl(&mut typeimpl_writer)?;
        }

        // TODO: Write type impl and fundamental files here
        Ok(())
    }
}

pub struct CppContextCollection {
    all_contexts: HashMap<TypeTag, CppContext>,
    filled_types: HashSet<TypeTag>,
}

impl CppContextCollection {
    pub fn fill(&mut self, metadata: &Metadata, config: &GenerationConfig, ty: impl Into<TypeTag>) {
        let tag: TypeTag = ty.into();

        if self.filled_types.contains(&tag) {
            return;
        }

        self.make_from(metadata, config, tag);
        let mut cpp_type = self
            .all_contexts
            .get_mut(&tag)
            .unwrap()
            .typedef_types
            .remove(&tag)
            .unwrap();

        let tdi = match tag {
            TypeTag::TypeDefinition(tdi) => tdi,
            _ => panic!("What {:?}", tag),
        };
        cpp_type.fill(metadata, config, self, tdi);

        self.all_contexts
            .get_mut(&tag)
            .unwrap()
            .typedef_types
            .insert(tag, cpp_type);
    }

    pub fn make_from(
        &mut self,
        metadata: &Metadata,
        config: &GenerationConfig,
        ty: impl Into<TypeTag>,
    ) -> &mut CppContext {
        let tag = ty.into();

        self.all_contexts.entry(tag).or_insert_with(|| match tag {
            TypeTag::TypeDefinition(tdi) => CppContext::make(metadata, config, tdi),
            _ => panic!("Unsupported type: {tag:?}"),
        })
    }

    pub fn get_cpp_type(
        &mut self,
        metadata: &Metadata,
        config: &GenerationConfig,
        ty: impl Into<TypeTag>,
    ) -> Option<&mut CppType> {
        let tag = ty.into();
        let context = self.make_from(metadata, config, tag);

        context.typedef_types.get_mut(&tag)
    }

    pub fn get_context(&self, type_tag: TypeTag) -> Option<&CppContext> {
        self.all_contexts.get(&type_tag)
    }

    pub fn is_type_made(&self, tag: TypeTag) -> bool {
        self.all_contexts.contains_key(&tag)
    }

    pub fn new() -> CppContextCollection {
        CppContextCollection {
            all_contexts: Default::default(),
            filled_types: Default::default(),
        }
    }
    pub fn get(&self) -> &HashMap<TypeTag, CppContext> {
        &self.all_contexts
    }
}
