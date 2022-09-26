use anyhow::{Context, Result};
use std::{
    collections::{HashMap, HashSet},
    fs::{create_dir_all, remove_file, File},
    io::Write,
    path::{Path, PathBuf},
};

use il2cpp_binary::{Type, TypeData, TypeEnum};
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
    fn write(&self, writer: &mut CppWriter) {
        if let Some(val) = &self.comment {
            writeln!(writer, "// {val}").unwrap();
        }
        writeln!(writer, "{}", self.data).unwrap();
    }
}

// Holds the contextual information for creating a C++ file
// Will hold various metadata, such as includes, type definitions, and extraneous writes
#[derive(Debug)]
pub struct CppContext {
    typedef_includes: HashSet<CppCommentedString>,
    typeimpl_includes: HashSet<CppCommentedString>,
    declarations: Vec<CppCommentedString>,
    typedef_path: PathBuf,
    type_impl_path: PathBuf,
    fundamental_path: PathBuf,
    types: HashMap<TypeTag, CppType>,
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

pub struct CppContextCollection {
    all_contexts: HashMap<TypeTag, CppContext>,
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
    pub fn need_wrapper(&mut self) {
        self.add_include("beatsaber-hook/shared/utils/base-wrapper-type.hpp".to_string());
    }
    pub fn get_cpp_type_name(&self, ty: &Type) -> String {
        let tag = TypeTag::from(ty.data);
        if let Some(result) = self.types.get(&tag) {
            // We found a valid type that we have defined for this idx!
            // TODO: We should convert it here.
            // Ex, if it is a generic, convert it to a template specialization
            // If it is a normal type, handle it accordingly, etc.
            match tag {
                TypeTag::TypeDefinition(_) => result.namespace().to_owned() + "::" + result.name(),
                _ => panic!("Unsupported type conversion for type: {tag:?}!"),
            }
        } else {
            panic!("Could not find type: {ty:?} in context: {self:?}!");
        }
    }
    pub fn cpp_name(
        &mut self,
        ctx_collection: &mut CppContextCollection,
        metadata: &Metadata,
        config: &GenerationConfig,
        typ: &Type,
    ) -> String {
        match typ.ty {
            TypeEnum::Object => {
                self.need_wrapper();
                "::bs_hook::Il2CppWrapperType".to_string()
            }
            TypeEnum::Class => {
                // In this case, just inherit the type
                // But we have to:
                // - Determine where to include it from
                let to_incl = ctx_collection.make_from(metadata, config, typ.data, false);
                // - Include it
                self.add_include_ctx(to_incl, "Including parent context".to_string());
                to_incl.get_cpp_type_name(typ)
            }
            TypeEnum::Valuetype => "/* UNKNOWN VALUE TYPE! */".to_string(),
            _ => "/* UNKNOWN TYPE! */".to_string(),
        }
    }

    fn should_make_rest(&self) -> bool {
        return self.types.iter().any(|(_, t)| !t.made);
    }

    fn make_rest(
        &mut self,
        metadata: &Metadata,
        config: &GenerationConfig,
        ctx_collection: &mut CppContextCollection,
        tdi: TypeDefinitionIndex,
    ) {
        let mut new_types = self.types.clone();

        for cpp_type in &mut new_types.values_mut() {
            cpp_type.make_rest(metadata, config, ctx_collection, self, tdi)
        }
        self.types = new_types;
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
            typedef_path: PathBuf::from(
                path.clone() + "__" + &config.path_name(name.to_string()) + "_def.hpp",
            ),
            type_impl_path: PathBuf::from(
                path.clone() + "__" + &config.path_name(name.to_string()) + "_impl.hpp",
            ),
            fundamental_path: PathBuf::from(path + &config.path_name(name.to_string()) + ".hpp"),
            typedef_includes: HashSet::new(),
            typeimpl_includes: HashSet::new(),
            declarations: Vec::new(),
            types: HashMap::new(),
            included_contexts: Vec::new(),
        };
        if let Some(cpptype) = CppType::make(metadata, config, tdi) {
            x.types.insert(TypeTag::TypeDefinition(tdi), cpptype);
        } else {
            println!("Unable to create valid CppContext for type: {t:?}!");
        }
        x
    }
    pub fn write(&self) -> Result<()> {
        // Write typedef file first
        if Path::exists(self.typedef_path.as_path()) {
            remove_file(self.typedef_path.as_path()).unwrap();
        }
        if !Path::is_dir(
            self.typedef_path
                .parent()
                .context("parent is not a directory!")
                .unwrap(),
        ) {
            // Assume it's never a file
            create_dir_all(
                self.typedef_path
                    .parent()
                    .context("Failed to create all directories!")
                    .unwrap(),
            )
            .unwrap();
        }
        let mut writer = CppWriter {
            stream: File::create(self.typedef_path.as_path()).unwrap(),
            indent: 0,
        };
        // Write includes for typedef
        self.typedef_includes
            .iter()
            .for_each(|inc| inc.write(&mut writer));
        self.declarations
            .iter()
            .for_each(|dec| dec.write(&mut writer));
        self.types.iter().for_each(|(k, v)| {
            writeln!(writer, "/* {:?} */", k).unwrap();
            v.write(&mut writer);
        });

        // TODO: Write type impl and fundamental files here
        Ok(())
    }
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

            if let TypeData::TypeDefinitionIndex(tdi) = ty {
                if fill {
                    res.make_rest(metadata, config, self, tdi);
                }
            }

            return self.all_contexts.entry(tag).or_insert(res);
        }

        let value = match ty {
            TypeData::TypeDefinitionIndex(tdi) => {
                let mut ret = CppContext::make(metadata, config, tdi);
                if fill {
                    ret.make_rest(metadata, config, self, tdi);
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
