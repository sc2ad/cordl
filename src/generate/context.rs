use std::{
    collections::{hash_map::Entry, HashMap, HashSet},
    fs::{create_dir_all, remove_file, File},
    io::Write,
    path::{Path, PathBuf},
};

use color_eyre::eyre::ContextCompat;
use il2cpp_binary::{Type, TypeData};
use il2cpp_metadata_raw::TypeDefinitionIndex;

use super::{
    config::GenerationConfig,
    cpp_type::CppType,
    metadata::Metadata,
    writer::{CppWriter, Writable},
};

#[derive(Debug, Eq, Hash, PartialEq, Clone)]
pub struct CppCommentedString {
    pub data: String,
    pub comment: Option<String>,
}

impl Writable for CppCommentedString {
    fn write(&self, writer: &mut CppWriter) -> color_eyre::Result<()> {
        if let Some(val) = &self.comment {
            writeln!(writer, "// {val}")?;
        }
        writeln!(writer, "{}", self.data)?;
        Ok(())
    }
}

// Holds the contextual information for creating a C++ file
// Will hold various metadata, such as includes, type definitions, and extraneous writes
#[derive(Debug)]
pub struct CppContext {
    // type declaration
    // fields
    // method declarations
    typedef_includes: HashSet<CppCommentedString>,

    // method definitions
    typeimpl_includes: HashSet<CppCommentedString>,

    // IDK, for typedef
    typedef_declarations: Vec<CppCommentedString>,

    typedef_forward_declares: HashSet<TypeTag>,

    typedef_path: PathBuf,
    type_impl_path: PathBuf,

    // combined header
    fundamental_path: PathBuf,

    // Types to write, typedef
    typedef_types: HashMap<TypeTag, CppType>,

    // IDK
    included_contexts: Vec<CppContext>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypeTag {
    TypeDefinition(u32),
    Type(usize),
    GenericParameter(u32),
    GenericClass(usize),
    Array,
}

impl TypeTag {
    pub fn from(ty: TypeData) -> TypeTag {
        match ty {
            TypeData::TypeDefinitionIndex(tdi) => TypeTag::TypeDefinition(tdi),
            TypeData::TypeIndex(ti) => TypeTag::Type(ti),
            TypeData::GenericClassIndex(gci) => TypeTag::GenericClass(gci),
            TypeData::GenericParameterIndex(gpi) => TypeTag::GenericParameter(gpi),
            TypeData::ArrayType => TypeTag::Array,
        }
    }
}

impl CppContext {
    pub fn get_include_path(&self) -> &PathBuf {
        &self.typedef_path
    }
    pub fn add_include(&mut self, inc: String) {
        self.typedef_includes.insert(CppCommentedString {
            data: "#include \"".to_owned() + &inc + "\"",
            comment: None,
        });
    }
    pub fn add_typeimpl_include(&mut self, inc: String) {
        self.typeimpl_includes.insert(CppCommentedString {
            data: inc,
            comment: None,
        });
    }
    pub fn add_include_comment(&mut self, inc: String, comment: String) {
        self.typedef_includes.insert(CppCommentedString {
            data: "#include \"".to_owned() + &inc + "\"",
            comment: Some(comment),
        });
    }
    pub fn add_include_ctx(&mut self, inc: &CppContext, comment: String) {
        self.add_include_comment(
            inc.get_include_path().to_str().unwrap().to_string(),
            comment,
        )
    }
    pub fn add_forward_declare(&mut self, ty: TypeTag) {
        self.typedef_forward_declares.insert(ty);
    }
    pub fn need_wrapper(&mut self) {
        self.add_include("beatsaber-hook/shared/utils/base-wrapper-type.hpp".to_string());
    }
    pub fn get_cpp_type_name(&self, ty: &Type) -> String {
        let tag = TypeTag::from(ty.data);
        if let Some(result) = self.typedef_types.get(&tag) {
            // We found a valid type that we have defined for this idx!
            // TODO: We should convert it here.
            // Ex, if it is a generic, convert it to a template specialization
            // If it is a normal type, handle it accordingly, etc.
            match tag {
                TypeTag::TypeDefinition(_) => {
                    format!("{}::{}", result.namespace_fixed(), result.name())
                }
                _ => panic!("Unsupported type conversion for type: {tag:?}!"),
            }
        } else {
            panic!("Could not find type: {ty:?} in context: {self:?}!");
        }
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

        const header_path: &str = "./codegen/include";

        let mut x = CppContext {
            typedef_path: format!(
                "{}/{}__{}_def.hpp",
                header_path,
                path,
                &config.path_name(name.to_string())
            )
            .into(),
            type_impl_path: format!(
                "{}/{}__{}_impl.hpp",
                header_path,
                path,
                &config.path_name(name.to_string())
            )
            .into(),
            fundamental_path: format!(
                "{}/{}{}.hpp",
                header_path,
                path,
                &config.path_name(name.to_string())
            )
            .into(),
            typedef_includes: Default::default(),
            typeimpl_includes: Default::default(),
            typedef_declarations: Default::default(),
            typedef_types: Default::default(),
            included_contexts: Default::default(),
            typedef_forward_declares: Default::default(),
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
        };
        let _typeimpl_writer = CppWriter {
            stream: File::create(self.type_impl_path.as_path())?,
            indent: 0,
        };
        let _fundamental_writer = CppWriter {
            stream: File::create(self.fundamental_path.as_path())?,
            indent: 0,
        };

        // Write includes for typedef
        self.typedef_includes
            .iter()
            .for_each(|inc| inc.write(&mut typedef_writer).unwrap());
        self.typedef_declarations
            .iter()
            .for_each(|dec| dec.write(&mut typedef_writer).unwrap());
        self.typedef_types.iter().for_each(|(k, v)| {
            writeln!(typedef_writer, "/* {:?} */", k).unwrap();
            v.write(&mut typedef_writer).unwrap();
        });

        // TODO: Write type impl and fundamental files here
        Ok(())
    }

    fn fill_type(
        &mut self,
        metadata: &Metadata,
        config: &GenerationConfig,
        ctx_collection: &mut CppContextCollection,
        ty: TypeData,
    ) {
        let tag = TypeTag::from(ty);
        if let TypeData::TypeDefinitionIndex(tdi) = ty {
            let cpp_type_entry = self.typedef_types.entry(tag);

            if let Entry::Occupied(occupied) = cpp_type_entry {
                let mut cpp_type = occupied.remove();
                cpp_type.fill(metadata, config, ctx_collection, tdi);

                if cpp_type.needs_wrapper {
                    self.need_wrapper();
                }

                for include in &cpp_type.required_includes {
                    self.add_include_comment(
                        include.to_str().unwrap().to_string(),
                        "Including parent context".to_string(),
                    )
                }

                for fd_declare in &cpp_type.forward_declares {
                    // - Include it
                    self.add_forward_declare(TypeTag::from(fd_declare.data));
                }

                self.typedef_types.entry(tag).insert_entry(cpp_type);
            }
        }
    }
}

pub struct CppContextCollection {
    all_contexts: HashMap<TypeTag, CppContext>,
}

impl CppContextCollection {
    pub fn make_from(
        &mut self,
        metadata: &Metadata,
        config: &GenerationConfig,
        ty: TypeData,
        fill: bool,
    ) -> &mut CppContext {
        let tag = TypeTag::from(ty);
        if self.all_contexts.contains_key(&tag) {
            // TODO: Check if existing context is already filled
            if !fill {
                return self.all_contexts.get_mut(&tag).unwrap();
            }

            // Take ownership, modify and then replace
            let mut res = self.all_contexts.remove(&tag).unwrap();

            if fill {
                res.fill_type(metadata, config, self, ty);
            }

            return self.all_contexts.entry(tag).or_insert(res);
        }

        let value = match ty {
            TypeData::TypeDefinitionIndex(tdi) => {
                let mut ret = CppContext::make(metadata, config, tdi);
                if fill {
                    ret.fill_type(metadata, config, self, ty);
                }

                ret
            }
            _ => panic!("Unsupported type: {ty:?}"),
        };

        self.all_contexts.entry(tag).or_insert(value)
    }

    pub fn get_context(&self, type_tag: TypeTag) -> Option<&CppContext> {
        self.all_contexts.get(&type_tag)
    }

    pub fn is_type_made(&self, tag: TypeTag) -> bool {
        self.all_contexts.contains_key(&tag)
    }

    pub fn new() -> CppContextCollection {
        CppContextCollection {
            all_contexts: HashMap::new(),
        }
    }
    pub fn get(&self) -> &HashMap<TypeTag, CppContext> {
        &self.all_contexts
    }
}
