use core::panic;
use std::{
    collections::{HashMap, HashSet},
    fs::{create_dir_all, remove_file, File},
    path::{Path, PathBuf},
};

use brocolib::{
    global_metadata::{
        Il2CppMethodDefinition, Il2CppTypeDefinition, MethodIndex, TypeDefinitionIndex,
    },
    runtime_metadata::{Il2CppGenericClass, Il2CppGenericContext, Il2CppMethodSpec, Il2CppType},
};
use color_eyre::eyre::ContextCompat;

use brocolib::runtime_metadata::TypeData;
use itertools::Itertools;

use crate::generate::{
    constants::{TypeDefinitionExtensions, OBJECT_WRAPPER_TYPE},
    members::CppInclude,
};

use super::{
    config::GenerationConfig,
    cpp_type::CppType,
    cs_type::CSType,
    members::CppUsingAlias,
    metadata::Metadata,
    writer::{CppWriter, Writable},
};

// TODO:
type GenericClassIndex = usize;

// Unique identifier for a CppType
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum CppTypeTag {
    TypeDefinitionIndex(TypeDefinitionIndex),
    GenericInstantiation(GenericClassIndex),
}

impl From<TypeDefinitionIndex> for CppTypeTag {
    fn from(value: TypeDefinitionIndex) -> Self {
        CppTypeTag::TypeDefinitionIndex(value)
    }
}
impl From<TypeData> for CppTypeTag {
    fn from(value: TypeData) -> Self {
        match value {
            TypeData::TypeDefinitionIndex(i) => i.into(),
            TypeData::GenericClassIndex(i) => CppTypeTag::GenericInstantiation(i),
            _ => panic!("Can't use {value:?} for CppTypeTag"),
        }
    }
}

impl From<CppTypeTag> for TypeData {
    fn from(value: CppTypeTag) -> Self {
        match value {
            CppTypeTag::TypeDefinitionIndex(i) => TypeData::TypeDefinitionIndex(i),
            CppTypeTag::GenericInstantiation(i) => TypeData::GenericClassIndex(i),
        }
    }
}

impl From<CppTypeTag> for TypeDefinitionIndex {
    fn from(value: CppTypeTag) -> Self {
        match value {
            CppTypeTag::TypeDefinitionIndex(i) => i,
            _ => panic!("Type is not a TDI! {value:?}"),
        }
    }
}

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
    fn make(
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
                // Only stored temporary for metatdata, replaced later
                // TODO: Figure out a better way of doing this
                x.typedef_types.insert(tdi.into(), cpptype);
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

            CppInclude::new(self.typedef_path.to_path_buf()).write(&mut typeimpl_writer)?;
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

        CppInclude::new(self.typedef_path.to_path_buf()).write(&mut fundamental_writer)?;
        CppInclude::new(self.type_impl_path.to_path_buf()).write(&mut fundamental_writer)?;

        // TODO: Write type impl and fundamental files here
        Ok(())
    }
}

pub struct CppContextCollection {
    // Should always be a TypeDefinitionIndex
    all_contexts: HashMap<CppTypeTag, CppContext>,
    pub alias_context: HashMap<CppTypeTag, CppTypeTag>,
    filled_types: HashSet<CppTypeTag>,
    filling_types: HashSet<CppTypeTag>,
    borrowing_types: HashSet<CppTypeTag>,
}

impl CppContextCollection {
    pub fn fill_cpp_type(
        &mut self,
        cpp_type: &mut CppType,
        metadata: &Metadata,
        config: &GenerationConfig,
        tdi: TypeDefinitionIndex,
    ) {
        if self.filled_types.contains(&cpp_type.self_tag) {
            return;
        }

        // Move ownership to local
        self.filling_types.insert(cpp_type.self_tag);

        cpp_type.fill_from_il2cpp(metadata, config, self, tdi);

        self.filled_types.insert(cpp_type.self_tag);
        self.filling_types.remove(&cpp_type.self_tag);
    }

    pub fn fill(&mut self, metadata: &Metadata, config: &GenerationConfig, type_tag: CppTypeTag) {
        let tdi = CppType::get_cpp_tag_tdi(type_tag);

        assert!(
            !metadata.child_to_parent_map.contains_key(&tdi),
            "Do not fill a child"
        );

        let context_tag = self.get_context_root_tag(type_tag);

        if self.filled_types.contains(&type_tag) {
            return;
        }

        if self.borrowing_types.contains(&context_tag) {
            panic!("Borrowing context {context_tag:?}");
        }

        // Move ownership to local
        let cpp_type_entry = self
            .all_contexts
            .get_mut(&context_tag)
            .expect("No cpp context")
            .typedef_types
            .remove_entry(&type_tag);

        // In some occasions, the CppContext can be empty
        if let Some((t, mut cpp_type)) = cpp_type_entry {
            assert!(!cpp_type.nested, "Cannot fill a nested type!");

            self.fill_cpp_type(&mut cpp_type, metadata, config, tdi);

            // Move ownership back up
            self.all_contexts
                .get_mut(&context_tag)
                .expect("No cpp context")
                .typedef_types
                .insert(t, cpp_type);
        }
    }

    fn alias_nested_types(&mut self, owner: &CppType, root_tag: CppTypeTag, context_check: bool) {
        for nested_type in &owner.nested_types {
            // println!(
            //     "Aliasing {:?} to {:?}",
            //     nested_type.self_tag, owner.self_tag
            // );
            self.alias_type(nested_type.self_tag, root_tag, context_check);
            self.alias_nested_types(nested_type, root_tag, context_check);
        }
    }

    pub fn alias_type(&mut self, src: CppTypeTag, dest: CppTypeTag, context_check: bool) {
        if self.alias_context.contains_key(&dest) {
            panic!("Aliasing an aliased type!");
        }
        if context_check && !self.all_contexts.contains_key(&dest) {
            panic!("Aliased context {src:?} to {dest:?} doesn't have a context");
        }
        self.alias_context.insert(src, dest);
    }

    pub fn fill_nested_types(
        &mut self,
        metadata: &Metadata,
        config: &GenerationConfig,
        owner_ty: CppTypeTag,
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
            let tdi = nested_type.tdi;

            self.fill_cpp_type(nested_type, metadata, config, tdi);

            nested_type.fill_from_il2cpp(metadata, config, self, tdi);
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

    pub fn get_context_root_tag(&self, ty: CppTypeTag) -> CppTypeTag {
        let tag = ty;
        self.alias_context
            .get(&tag)
            .cloned()
            // .map(|t| self.get_context_root_tag(*t))
            .unwrap_or(tag)
    }

    /// Make a generic type
    /// based of an existing type definition
    /// and give it the generic args
    pub fn make_generic_from(
        &mut self,
        method_spec: &Il2CppMethodSpec,
        metadata: &mut Metadata,
    ) -> Option<&mut CppContext> {
        let method =
            &metadata.metadata.global_metadata.methods[method_spec.method_definition_index];
        let ty_def = &metadata.metadata.global_metadata.type_definitions[method.declaring_type];

        let type_data = CppTypeTag::TypeDefinitionIndex(method.declaring_type);
        let tdi = method.declaring_type;
        let context_root_tag = self.get_context_root_tag(type_data);

        if self.filling_types.contains(&context_root_tag) {
            panic!("Currently filling type {context_root_tag:?}, cannot fill")
        }

        // get parent type if required
        let parent_data = metadata.child_to_parent_map.get(&tdi).cloned();

        let (generic_class_ty_opt, generic_class) =
            find_generic_il2cpp_type_data(ty_def, method_spec, metadata, method);

        if generic_class_ty_opt.is_none() {
            println!("Skipping {}", method.full_name(metadata.metadata));
            println!("{generic_class:?}\n");
            return None;
        }

        let generic_class_ty = generic_class_ty_opt.unwrap();
        // Why is the borrow checker so dumb?
        // Using entries causes borrow checker to die :(
        if self.filled_types.contains(&generic_class_ty.data.into()) {
            return Some(self.all_contexts.get_mut(&context_root_tag).unwrap());
        }

        let template_cpp_type = self.get_cpp_type(type_data).unwrap();
        let mut new_cpp_type = template_cpp_type.clone();

        new_cpp_type.self_tag = generic_class_ty.data.into();
        self.alias_type(new_cpp_type.self_tag, context_root_tag, true);

        // if generic type is a nested type
        // put it under the parent's `nested_types` field
        // otherwise put it in the typedef's hashmap
        match parent_data {
            Some(parent) => {
                let parent_ty = CppTypeTag::TypeDefinitionIndex(parent.tdi);

                self.borrow_cpp_type(
                    parent_ty,
                    |_collection: &mut CppContextCollection, mut cpp_type| {
                        cpp_type.nested_types.push(new_cpp_type.clone());

                        cpp_type
                    },
                )
            }
            None => {
                let context = self.get_context_mut(generic_class_ty.data.into()).unwrap();

                context
                    .typedef_types
                    .insert(generic_class_ty.data.into(), new_cpp_type);
            }
        }

        return Some(self.all_contexts.get_mut(&context_root_tag).unwrap());
    }

    pub fn fill_generic_inst(
        &mut self,
        method_spec: &Il2CppMethodSpec,
        metadata: &mut Metadata,
        config: &GenerationConfig,
    ) -> Option<&mut CppContext> {
        let method =
            &metadata.metadata.global_metadata.methods[method_spec.method_definition_index];
        let ty_def = &metadata.metadata.global_metadata.type_definitions[method.declaring_type];

        let type_data = TypeData::TypeDefinitionIndex(method.declaring_type);
        let tdi = method.declaring_type;

        let context_root_tag = self.get_context_root_tag(type_data.into());

        if self.filling_types.contains(&context_root_tag) {
            panic!("Currently filling type {context_root_tag:?}, cannot fill")
        }

        let (generic_class_ty_opt, _generic_class) =
            find_generic_il2cpp_type_data(ty_def, method_spec, metadata, method);

        let generic_class_ty = generic_class_ty_opt?;

        self.borrow_cpp_type(generic_class_ty.data.into(), |collection, mut cpp_type| {
            if method_spec.class_inst_index != u32::MAX {
                if true || cpp_type.generic_instantiations_args_types.is_none() {
                    cpp_type.add_generic_inst(method_spec.class_inst_index, metadata);
                }
                collection.fill_cpp_type(&mut cpp_type, metadata, config, tdi);
            }

            let method_index = MethodIndex::new(method_spec.method_inst_index);
            cpp_type.create_method(method, ty_def, method_index, metadata, collection, config);

            cpp_type
        });

        let con = self.all_contexts.get_mut(&context_root_tag).unwrap();

        if ty_def.name(metadata.metadata) == "IList`1" {
            println!("{:?}", con.typedef_types);
        }

        return Some(con);
    }

    pub fn make_from(
        &mut self,
        metadata: &Metadata,
        config: &GenerationConfig,
        type_tag: TypeData,
    ) -> &mut CppContext {
        assert!(
            !metadata
                .child_to_parent_map
                .contains_key(&CppType::get_tag_tdi(type_tag)),
            "Cannot create context for nested type",
        );
        let context_root_tag = self.get_context_root_tag(type_tag.into());

        if self.filling_types.contains(&context_root_tag) {
            panic!("Currently filling type {context_root_tag:?}, cannot fill")
        }

        if self.borrowing_types.contains(&context_root_tag) {
            panic!("Currently borrowing context {context_root_tag:?}, cannot fill")
        }

        // Why is the borrow checker so dumb?
        // Using entries causes borrow checker to die :(
        if self.all_contexts.contains_key(&context_root_tag) {
            return self.all_contexts.get_mut(&context_root_tag).unwrap();
        }

        let tdi = CppType::get_cpp_tag_tdi(context_root_tag);
        let context = CppContext::make(metadata, config, tdi, context_root_tag);
        // Now do children
        for cpp_type in context.typedef_types.values() {
            self.alias_nested_types(cpp_type, cpp_type.self_tag, false);
        }
        self.all_contexts.insert(context_root_tag, context);
        self.all_contexts.get_mut(&context_root_tag).unwrap()
    }

    pub fn get_cpp_type(&self, ty: CppTypeTag) -> Option<&CppType> {
        let tag = ty;
        let context_root_tag = self.get_context_root_tag(tag);
        self.get_context(context_root_tag)
            .and_then(|c| c.get_cpp_type_recursive(context_root_tag, tag))
    }
    pub fn get_cpp_type_mut(&mut self, ty: CppTypeTag) -> Option<&mut CppType> {
        let tag = ty;
        let context_root_tag = self.get_context_root_tag(tag);
        self.get_context_mut(context_root_tag)
            .and_then(|c| c.get_cpp_type_recursive_mut(context_root_tag, tag))
    }

    pub fn borrow_cpp_type<F>(&mut self, ty: CppTypeTag, func: F)
    where
        F: Fn(&mut Self, CppType) -> CppType,
    {
        let context_ty = self.get_context_root_tag(ty);
        if self.borrowing_types.contains(&context_ty) {
            panic!("Already borrowing this context!");
        }

        // TODO: Do this without removing or cloning
        let mut context = self
            .all_contexts
            .get(&context_ty)
            .cloned()
            .expect("No context ty?");

        // TODO: Needed
        // self.borrowing_types.insert(context_ty);

        // search in root
        // clone to avoid failing il2cpp_name
        let in_context_cpp_type = context.typedef_types.get(&ty).cloned();
        match in_context_cpp_type {
            Some(cpp_type) => {
                context.typedef_types.insert(ty, func(self, cpp_type));
            }
            None => {
                let mut found = false;
                for nested_type in context.typedef_types.values_mut() {
                    if nested_type.borrow_nested_type_mut(ty, self, &func) {
                        found = true;
                        break;
                    }
                }

                if !found {
                    panic!("No nested or parent type found!");
                }
            }
        }
        self.borrowing_types.remove(&context_ty);

        self.all_contexts.insert(context_ty, context);
    }

    pub fn get_context(&self, type_tag: CppTypeTag) -> Option<&CppContext> {
        let context_tag = self.get_context_root_tag(type_tag);
        if self.borrowing_types.contains(&context_tag) {
            panic!("Borrowing this context! {context_tag:?}");
        }
        self.all_contexts.get(&context_tag)
    }
    pub fn get_context_mut(&mut self, type_tag: CppTypeTag) -> Option<&mut CppContext> {
        let context_tag = self.get_context_root_tag(type_tag);
        if self.borrowing_types.contains(&context_tag) {
            panic!("Borrowing this context! {context_tag:?}");
        }
        self.all_contexts
            .get_mut(&self.get_context_root_tag(context_tag))
    }

    pub fn new() -> CppContextCollection {
        CppContextCollection {
            all_contexts: Default::default(),
            filled_types: Default::default(),
            filling_types: Default::default(),
            alias_context: Default::default(),
            borrowing_types: Default::default(),
        }
    }
    pub fn get(&self) -> &HashMap<CppTypeTag, CppContext> {
        &self.all_contexts
    }
}

fn find_generic_il2cpp_type_data<'a>(
    ty_def: &Il2CppTypeDefinition,
    method_spec: &Il2CppMethodSpec,
    metadata: &'a mut Metadata,
    _method: &Il2CppMethodDefinition,
) -> (Option<&'a Il2CppType>, Il2CppGenericClass) {
    let generic_class: Il2CppGenericClass = Il2CppGenericClass {
        type_index: ty_def.byval_type_index as usize,
        context: Il2CppGenericContext {
            class_inst_idx: Some(method_spec.class_inst_index as usize),
            method_inst_idx: None,
        },
    };
    let generic_class_ty_opt = metadata
        .metadata_registration
        .types
        .iter()
        .find(|t| match t.data {
            TypeData::GenericClassIndex(index) => {
                let o = metadata
                    .metadata_registration
                    .generic_classes
                    .get(index)
                    .unwrap();

                *o == generic_class
            }
            _ => false,
        });

    (generic_class_ty_opt, generic_class)
}
