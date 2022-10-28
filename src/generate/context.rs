use std::{
    collections::{hash_map::Entry, HashMap, HashSet},
    fs::{create_dir_all, remove_file, File},
    io::Write,
    path::{Path, PathBuf},
};

use color_eyre::{eyre::ContextCompat};
use color_eyre::Result;
use il2cpp_binary::TypeData;
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
    // Map of Namespace -> declared structs?
    typedef_declarations: HashMap<String, Vec<CppCommentedString>>,

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
    pub fn add_include(&mut self, inc: &str) {
        self.typedef_includes.insert(CppCommentedString {
            data: format!("#include \"{}\"", &inc),
            comment: None,
        });
    }
    pub fn add_system_include(&mut self, inc: &str) {
        self.typedef_includes.insert(CppCommentedString {
            data: format!("#include <{}>", &inc),
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
            inc.get_include_path()
                .strip_prefix("./")
                .unwrap()
                .to_str()
                .unwrap()
                .to_string(),
            comment,
        )
    }
    pub fn add_forward_declare(&mut self, namespace: String, name: &str) {
        self.typedef_declarations
            .entry(namespace)
            .or_default()
            .push(CppCommentedString {
                data: format!("struct {};", name),
                comment: None,
            });
    }
    pub fn need_wrapper(&mut self) {
        self.add_include("beatsaber-hook/shared/utils/base-wrapper-type.hpp");
    }
    pub fn needs_int_include(&mut self) {
        self.add_system_include("cstdint");
    }
    pub fn needs_stringw_include(&mut self) {
        self.add_include("beatsaber-hook/shared/utils/typedefs-string.hpp");
    }
    pub fn needs_arrayw_include(&mut self) {
        self.add_include("beatsaber-hook/shared/utils/typedefs-array.hpp");
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
            typedef_includes: Default::default(),
            typeimpl_includes: Default::default(),
            typedef_declarations: Default::default(),
            typedef_types: Default::default(),
            included_contexts: Default::default(),
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
        let mut _typeimpl_writer = CppWriter {
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
            .try_for_each(|inc| inc.write(&mut typedef_writer))?;

        // forward declares
        for (namespace, strings) in &self.typedef_declarations {
            writeln!(typedef_writer, "namespace {} {{", namespace)?;
            strings
                .iter()
                .try_for_each(|s| s.write(&mut typedef_writer))?;
            writeln!(typedef_writer, "}} // namespace {}", namespace)?;
        }
        self.typedef_types
            .iter()
            .try_for_each(|(k, v)| -> Result<()> {
                writeln!(typedef_writer, "/* {:?} */", k)?;
                v.write(&mut typedef_writer)?;
                v.write_impl(&mut _typeimpl_writer)?;

                Ok(())
            })?;

        // TODO: Write type impl and fundamental files here
        Ok(())
    }

    fn fill_type(
        &mut self,
        metadata: &Metadata,
        config: &GenerationConfig,
        ctx_collection: &mut CppContextCollection,
        tag: TypeTag,
    ) {
        if let TypeTag::TypeDefinition(tdi) = tag {
            let cpp_type_entry = self.typedef_types.entry(tag);

            if let Entry::Occupied(occupied) = cpp_type_entry {
                let mut cpp_type = occupied.remove();
                cpp_type.fill(metadata, config, ctx_collection, tdi);

                self.handle_requirements(&cpp_type, metadata, config, ctx_collection);

                self.typedef_types.entry(tag).insert_entry(cpp_type);
            }
        }
    }

    fn handle_requirements(
        &mut self,
        cpp_type: &CppType,
        metadata: &Metadata,
        config: &GenerationConfig,
        ctx_collection: &mut CppContextCollection,
    ) {
        if cpp_type.requirements.needs_wrapper {
            self.need_wrapper();
        }

        if cpp_type.requirements.needs_int_include {
            self.needs_int_include();
        }

        if cpp_type.requirements.needs_stringw_include {
            self.needs_stringw_include();
        }
        if cpp_type.requirements.needs_arrayw_include {
            self.needs_arrayw_include();
        }

        for include_type in &cpp_type.requirements.required_includes {
            let context = ctx_collection.make_from(metadata, config, *include_type, false);

            self.add_include_ctx(context, "Including required context".to_string());
        }

        for fd_tdi in &cpp_type.requirements.forward_declare_tids {
            // - Include it
            let fd_type_opt = ctx_collection.get_cpp_type(metadata, config, *fd_tdi, false);

            if let Some(fd_type) = fd_type_opt {
                self.add_forward_declare(fd_type.namespace_fixed().to_owned(), fd_type.name());
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
        ty: impl Into<TypeTag>,
        fill: bool,
    ) -> &mut CppContext {
        let tag = ty.into();
        if self.all_contexts.contains_key(&tag) {
            if !fill {
                return self.all_contexts.get_mut(&tag).unwrap();
            }

            // Take ownership, modify and then replace
            let mut res = self.all_contexts.remove(&tag).unwrap();

            if fill {
                res.fill_type(metadata, config, self, tag);
            }

            return self.all_contexts.entry(tag).insert_entry(res).into_mut();
        }

        let value = match tag {
            TypeTag::TypeDefinition(tdi) => {
                let mut ret = CppContext::make(metadata, config, tdi);
                if fill {
                    ret.fill_type(metadata, config, self, tag);
                }

                ret
            }
            _ => panic!("Unsupported type: {tag:?}"),
        };

        self.all_contexts.entry(tag).or_insert(value)
    }

    pub fn get_cpp_type(
        &mut self,
        metadata: &Metadata,
        config: &GenerationConfig,
        ty: impl Into<TypeTag>,
        fill: bool,
    ) -> std::option::Option<&mut CppType> {
        let tag = ty.into();
        let context = self.make_from(metadata, config, tag, fill);

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
            all_contexts: HashMap::new(),
        }
    }
    pub fn get(&self) -> &HashMap<TypeTag, CppContext> {
        &self.all_contexts
    }
}
