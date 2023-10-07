use core::panic;
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::Write,
};

use brocolib::{
    global_metadata::TypeDefinitionIndex,
    runtime_metadata::{Il2CppMethodSpec, TypeData},
};
use itertools::Itertools;
use log::{info, trace, warn};
use pathdiff::diff_paths;

use crate::{
    generate::{cpp_type::CppType, cs_type::CSType},
    STATIC_CONFIG,
};

use super::{
    config::GenerationConfig,
    context::CppContext,
    cpp_type_tag::{CppTypeTag, GenericInstantiation},
    metadata::Metadata,
    type_extensions::TypeDefinitionExtensions,
};

pub struct CppContextCollection {
    // Should always be a TypeDefinitionIndex
    all_contexts: HashMap<CppTypeTag, CppContext>,
    pub alias_context: HashMap<CppTypeTag, CppTypeTag>,
    pub alias_nested_type_to_parent: HashMap<CppTypeTag, CppTypeTag>,
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
    ) {
        let tag = cpp_type.self_tag;

        if self.filled_types.contains(&tag) {
            return;
        }
        if self.filling_types.contains(&tag) {
            panic!("Currently filling type {tag:?}, cannot fill")
        }

        // Move ownership to local
        self.filling_types.insert(tag);

        cpp_type.fill_from_il2cpp(metadata, config, self);

        self.filled_types.insert(tag);
        self.filling_types.remove(&tag.clone());
    }

    pub fn fill(&mut self, metadata: &Metadata, config: &GenerationConfig, type_tag: CppTypeTag) {
        let _tdi = CppType::get_cpp_tag_tdi(type_tag);

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
        if let Some((_t, mut cpp_type)) = cpp_type_entry {
            assert!(!cpp_type.nested, "Cannot fill a nested type!");

            self.fill_cpp_type(&mut cpp_type, metadata, config);

            // Move ownership back up
            self.all_contexts
                .get_mut(&context_tag)
                .expect("No cpp context")
                .insert_cpp_type(cpp_type);
        }
    }

    fn alias_nested_types(&mut self, owner: &CppType, root_tag: CppTypeTag, context_check: bool) {
        for (tag, nested_type) in &owner.nested_types {
            // info!(
            //     "Aliasing {:?} to {:?}",
            //     nested_type.self_tag, owner.self_tag
            // );
            self.alias_type_to_context(*tag, root_tag, context_check, false);
            self.alias_type_to_parent(*tag, owner.self_tag, context_check);

            self.alias_nested_types(nested_type, root_tag, context_check);
        }
    }

    pub fn alias_type_to_context(
        &mut self,
        src: CppTypeTag,
        dest: CppTypeTag,
        context_check: bool,
        overrid: bool,
    ) {
        if self.alias_context.contains_key(&dest) && !overrid {
            panic!("Aliasing an aliased type! {src:?} to {dest:?}");
        }
        if context_check && !self.all_contexts.contains_key(&dest) {
            panic!("Aliased context {src:?} to {dest:?} doesn't have a context");
        }
        if self.alias_context.contains_key(&src) && context_check {
            panic!("Already aliased this key!");
        }
        self.alias_context.insert(src, dest);
    }

    pub fn alias_type_to_parent(
        &mut self,
        src: CppTypeTag,
        dest: CppTypeTag,
        _context_check: bool,
    ) {
        // if context_check && !self.all_contexts.contains_key(&dest) {
        //     panic!("Aliased nested type {src:?} to {dest:?} doesn't have a parent");
        // }
        if src == dest {
            panic!("Self {src:?} can't point to dest!")
        }
        if self.alias_nested_type_to_parent.get(&dest) == Some(&src) {
            panic!("Parent {dest:?} can't be assigned to src {src:?}!")
        }
        self.alias_nested_type_to_parent.insert(src, dest);
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
        let nested_types: HashMap<CppTypeTag, CppType> = owner
            .nested_types
            .clone()
            .into_iter()
            .map(|(nested_tag, mut nested_type)| {
                self.fill_cpp_type(&mut nested_type, metadata, config);

                (nested_tag, nested_type)
            })
            .collect();

        self.get_cpp_type_mut(owner_type_tag).unwrap().nested_types = nested_types;
    }

    pub fn get_context_root_tag(&self, ty: CppTypeTag) -> CppTypeTag {
        self.alias_context
            .get(&ty)
            .cloned()
            // .map(|t| self.get_context_root_tag(*t))
            .unwrap_or(ty)
    }
    pub fn get_parent_or_self_tag(&self, ty: CppTypeTag) -> CppTypeTag {
        self.alias_nested_type_to_parent
            .get(&ty)
            .cloned()
            .map(|t| self.get_parent_or_self_tag(t))
            .unwrap_or(ty)
    }

    pub fn make_nested_from(
        &mut self,
        metadata: &Metadata<'_>,
        config: &GenerationConfig,
        tdi: TypeDefinitionIndex,
        generic_inst: Option<&Vec<usize>>,
    ) -> Option<&mut CppContext> {
        let ty_data = CppTypeTag::TypeDefinitionIndex(tdi);
        let ty_def = &metadata.metadata.global_metadata.type_definitions[tdi];
        let context_root_tag = self.get_context_root_tag(ty_data);

        if self.filling_types.contains(&context_root_tag) {
            panic!("Currently filling type {context_root_tag:?}, cannot fill")
        }

        // Why is the borrow checker so dumb?
        // Using entries causes borrow checker to die :(
        if self.filled_types.contains(&ty_data) {
            return Some(self.all_contexts.get_mut(&context_root_tag).unwrap());
        }

        if self.get_cpp_type(ty_data).is_some() {
            return self.get_context_mut(ty_data);
        }

        let context_tag = self.get_context_root_tag(ty_data);
        let context_type_data: TypeDefinitionIndex = context_tag.into();
        let context_td = &metadata.metadata.global_metadata.type_definitions[context_type_data];

        if metadata.blacklisted_types.contains(&tdi) {
            warn!(
                "Skipping nested type because it's blacklisted! {context_tag:?} {}",
                context_td.full_name(metadata.metadata, true)
            );
            return None;
        }

        let nested_inherits_declaring = ty_def.is_assignable_to(context_td, metadata.metadata);
        if nested_inherits_declaring {
            warn!(
                "Nested type \"{}\" inherits declaring type \"{}\"",
                ty_def.full_name(metadata.metadata, true),
                context_td.full_name(metadata.metadata, true)
            );
        }

        match nested_inherits_declaring {
            true => {
                // If a nested type inherits its declaring type, move it to its own CppContext

                let context = CppContext::make(metadata, config, tdi, ty_data, generic_inst);

                // Unnest type does not alias to another context or type
                self.alias_context.remove(&ty_data);
                self.alias_nested_type_to_parent.remove(&ty_data);

                self.all_contexts.insert(ty_data, context);
                self.all_contexts.get_mut(&ty_data)
            }
            false => {
                let new_cpp_type =
                    CppType::make_cpp_type(metadata, config, tdi, ty_data, generic_inst)
                        .expect("Failed to make nested type");

                let context = self.get_context_mut(ty_data).unwrap();
                // self.alias_type_to_context(new_cpp_type.self_tag, context_root_tag, true);

                // context.insert_cpp_type(stub);
                context.insert_cpp_type(new_cpp_type);

                Some(context)
            }
        }
    }

    /// Make a generic type
    /// based of an existing type definition
    /// and give it the generic args
    pub fn make_generic_from(
        &mut self,
        method_spec: &Il2CppMethodSpec,
        metadata: &mut Metadata,
        config: &GenerationConfig,
    ) -> Option<&mut CppContext> {
        // Not a generic class, no type needed
        if method_spec.class_inst_index == u32::MAX {
            return None;
        }
        // Skip generic methods?
        if method_spec.method_inst_index != u32::MAX {
            return None;
        }

        let method =
            &metadata.metadata.global_metadata.methods[method_spec.method_definition_index];
        let ty_def = &metadata.metadata.global_metadata.type_definitions[method.declaring_type];

        if ty_def.is_interface() {
            // Skip interface
            info!(
                "Skipping make interface for generic instantiation {}",
                ty_def.full_name(metadata.metadata, true)
            );
            return None;
        }

        let type_data = CppTypeTag::TypeDefinitionIndex(method.declaring_type);
        let tdi = method.declaring_type;
        let context_root_tag = self.get_context_root_tag(type_data);

        if metadata.blacklisted_types.contains(&tdi) {
            warn!(
                "Skipping generic instantiation {tdi:?} {} {}",
                method_spec.class_inst_index,
                ty_def.full_name(metadata.metadata, true)
            );
            return None;
        }

        if self.filling_types.contains(&context_root_tag) {
            panic!("Currently filling type {context_root_tag:?}, cannot fill")
        }

        let generic_class_ty_data = CppTypeTag::GenericInstantiation(GenericInstantiation {
            tdi,
            inst: method_spec.class_inst_index as usize,
        });

        let generic_inst =
            &metadata.metadata_registration.generic_insts[method_spec.class_inst_index as usize];

        // Why is the borrow checker so dumb?
        // Using entries causes borrow checker to die :(
        if self.filled_types.contains(&generic_class_ty_data) {
            return Some(self.all_contexts.get_mut(&context_root_tag).unwrap());
        }

        if self.get_cpp_type(generic_class_ty_data).is_some() {
            return self.get_context_mut(generic_class_ty_data);
        }

        // make original type a stub
        self.borrow_cpp_type(type_data, |_, mut cpptype| {
            cpptype.is_stub = true;

            cpptype
        });

        let mut new_cpp_type = CppType::make_cpp_type(
            metadata,
            config,
            tdi,
            generic_class_ty_data,
            Some(&generic_inst.types),
        )
        .expect("Failed to make generic type");
        new_cpp_type.self_tag = generic_class_ty_data;
        self.alias_type_to_context(new_cpp_type.self_tag, context_root_tag, true, false);

        // TODO: Not needed since making a cpp type will already be a stub in other passes?
        // this is the generic stub
        // this might cause problems, hopefully not
        // since two types can coexist with the TDI though only one is nested
        // let mut stub = new_cpp_type.clone();
        // stub.self_tag = type_data;

        new_cpp_type.requirements.add_dependency_tag(type_data);

        // if generic type is a nested type
        // put it under the parent's `nested_types` field
        // otherwise put it in the typedef's hashmap

        let context = self.get_context_mut(generic_class_ty_data).unwrap();

        // context.insert_cpp_type(stub);
        context.insert_cpp_type(new_cpp_type);

        Some(context)
    }

    ///
    /// It's important this gets called AFTER the type is filled
    ///
    pub fn fill_generic_method_inst(
        &mut self,
        method_spec: &Il2CppMethodSpec,
        metadata: &mut Metadata,
        config: &GenerationConfig,
    ) -> Option<&mut CppContext> {
        if method_spec.method_inst_index == u32::MAX {
            return None;
        }

        let method =
            &metadata.metadata.global_metadata.methods[method_spec.method_definition_index];

        // is reference type
        // only make generic spatialization
        let type_data = CppTypeTag::TypeDefinitionIndex(method.declaring_type);
        let tdi = method.declaring_type;

        let ty_def = &metadata.metadata.global_metadata.type_definitions[method.declaring_type];

        if metadata.blacklisted_types.contains(&tdi) {
            info!(
                "Skipping {tdi:?} {} since it is blacklisted",
                ty_def.full_name(metadata.metadata, true)
            );
            return None;
        }

        if ty_def.is_interface() {
            // Skip interface
            info!(
                "Skipping fill generic method interface for generic instantiation {}",
                ty_def.full_name(metadata.metadata, true)
            );
            return None;
        }

        let context_root_tag = self.get_context_root_tag(type_data);

        let generic_class_ty_data = if method_spec.class_inst_index != u32::MAX {
            CppTypeTag::GenericInstantiation(GenericInstantiation {
                tdi,
                inst: method_spec.class_inst_index as usize,
            })
        } else {
            type_data
        };

        self.borrow_cpp_type(generic_class_ty_data, |collection, mut cpp_type| {
            let method_index = method_spec.method_definition_index;
            cpp_type.add_method_generic_inst(method_spec, metadata);
            cpp_type.create_method(ty_def, method_index, metadata, collection, config, true);

            cpp_type
        });

        self.all_contexts.get_mut(&context_root_tag)
    }

    pub fn fill_generic_class_inst(
        &mut self,
        method_spec: &Il2CppMethodSpec,
        metadata: &mut Metadata,
        config: &GenerationConfig,
    ) -> Option<&mut CppContext> {
        if method_spec.class_inst_index == u32::MAX {
            return None;
        }
        // Skip generic methods?
        if method_spec.method_inst_index != u32::MAX {
            return None;
        }

        let method =
            &metadata.metadata.global_metadata.methods[method_spec.method_definition_index];

        let ty_def = &metadata.metadata.global_metadata.type_definitions[method.declaring_type];

        // only make generic spatialization
        let type_data = CppTypeTag::TypeDefinitionIndex(method.declaring_type);
        let tdi = method.declaring_type;

        if metadata.blacklisted_types.contains(&tdi) {
            info!(
                "Skipping {tdi:?} {} since it is blacklisted",
                ty_def.full_name(metadata.metadata, true)
            );
            return None;
        }

        if ty_def.is_interface() {
            // Skip interface
            info!(
                "Skipping fill class interface for generic instantiation {}",
                ty_def.full_name(metadata.metadata, true)
            );
            return None;
        }

        let context_root_tag = self.get_context_root_tag(type_data);

        let generic_class_ty_data = if method_spec.class_inst_index != u32::MAX {
            CppTypeTag::GenericInstantiation(GenericInstantiation {
                tdi,
                inst: method_spec.class_inst_index as usize,
            })
        } else {
            type_data
        };

        self.borrow_cpp_type(generic_class_ty_data, |collection, mut cpp_type| {
            // cpp_type.make_generics_args(metadata, collection);
            collection.fill_cpp_type(&mut cpp_type, metadata, config);

            cpp_type
        });

        self.all_contexts.get_mut(&context_root_tag)
    }

    pub fn make_from(
        &mut self,
        metadata: &Metadata,
        config: &GenerationConfig,
        type_tag: TypeData,
        generic_inst: Option<&Vec<usize>>,
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
        let context = CppContext::make(metadata, config, tdi, context_root_tag, generic_inst);
        // Now do children
        for cpp_type in context.typedef_types.values() {
            self.alias_nested_types(cpp_type, cpp_type.self_tag, false);
        }
        self.all_contexts.insert(context_root_tag, context);
        self.all_contexts.get_mut(&context_root_tag).unwrap()
    }

    ///
    /// By default will only look for nested types of the context, ignoring other CppTypes
    ///
    pub fn get_cpp_type(&self, ty: CppTypeTag) -> Option<&CppType> {
        let tag = ty;
        let context_root_tag = self.get_context_root_tag(tag);
        let parent_root_tag = self.get_parent_or_self_tag(tag);

        self.get_context(context_root_tag)
            .and_then(|c| c.get_cpp_type_recursive(parent_root_tag, tag))
    }

    ///
    /// By default will only look for nested types of the context, ignoring other CppTypes
    ///
    pub fn get_cpp_type_mut(&mut self, ty: CppTypeTag) -> Option<&mut CppType> {
        let tag = ty;
        let context_root_tag = self.get_context_root_tag(tag);
        let parent_root_tag = self.get_parent_or_self_tag(tag);
        self.get_context_mut(context_root_tag)
            .and_then(|c| c.get_cpp_type_recursive_mut(parent_root_tag, tag))
    }

    pub fn borrow_cpp_type<F>(&mut self, ty: CppTypeTag, func: F)
    where
        F: Fn(&mut Self, CppType) -> CppType,
    {
        let context_ty = self.get_context_root_tag(ty);
        if self.borrowing_types.contains(&context_ty) {
            panic!("Already borrowing this context!");
        }

        let declaring_ty = self.get_parent_or_self_tag(ty);

        let (result_cpp_type, old_tag);

        {
            let context = self.all_contexts.get_mut(&context_ty).unwrap();

            // TODO: Needed?
            // self.borrowing_types.insert(context_ty);

            // search in root
            // clone to avoid failing il2cpp_name
            let declaring_cpp_type = context.typedef_types.get(&declaring_ty).cloned();
            (result_cpp_type, old_tag) = match declaring_cpp_type {
                Some(old_cpp_type) => {
                    let old_tag = old_cpp_type.self_tag;
                    let new_cpp_ty = func(self, old_cpp_type);

                    (new_cpp_ty, Some(old_tag))
                }
                None => {
                    let mut declaring_ty = context
                        .typedef_types
                        .get(&declaring_ty)
                        .expect("Parent ty not found in context")
                        .clone();

                    let found = declaring_ty.borrow_nested_type_mut(ty, self, &func);

                    if !found {
                        panic!("No nested or parent type found for type {ty:?}!");
                    }

                    (declaring_ty, None)
                }
            };
        }

        // avoid the borrow checker's wrath
        let context = self.all_contexts.get_mut(&context_ty).unwrap();
        if let Some(old_tag) = old_tag {
            context.typedef_types.remove(&old_tag);
        }
        context.insert_cpp_type(result_cpp_type);
        self.borrowing_types.remove(&context_ty);
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
            alias_nested_type_to_parent: Default::default(),
            alias_context: Default::default(),
            borrowing_types: Default::default(),
        }
    }
    pub fn get(&self) -> &HashMap<CppTypeTag, CppContext> {
        &self.all_contexts
    }

    pub fn write_all(&self, config: &GenerationConfig) -> color_eyre::Result<()> {
        let amount = self.all_contexts.len() as f64;
        self.all_contexts
            .iter()
            .enumerate()
            .try_for_each(|(i, (_, c))| {
                trace!(
                    "Writing {:.4}% ({}/{}) {}",
                    (i as f64 / amount * 100.0),
                    i,
                    amount,
                    c.fundamental_path.display(),
                );
                c.write(config)
            })
    }

    pub fn write_namespace_headers(&self) -> color_eyre::Result<()> {
        self.all_contexts
            .iter()
            .into_group_map_by(|(_, c)| c.fundamental_path.parent())
            .into_iter()
            .try_for_each(|(dir, contexts)| -> color_eyre::Result<()> {
                let namespace = if dir.unwrap() == STATIC_CONFIG.header_path {
                    "GlobalNamespace"
                } else {
                    dir.unwrap().file_name().unwrap().to_str().unwrap()
                };

                let str = contexts
                    .iter()
                    // ignore empty contexts
                    .filter(|(_, c)| !c.typedef_types.is_empty())
                    // ignore weird named types
                    .filter(|(_, c)| {
                        !c.fundamental_path
                            .file_name()
                            .unwrap()
                            .to_str()
                            .unwrap()
                            .starts_with('_')
                    })
                    // add includes
                    .map(|(_, c)| {
                        let stripped_path =
                            diff_paths(&c.fundamental_path, &STATIC_CONFIG.header_path).unwrap();
                        format!("#include \"{}\"", stripped_path.display())
                    })
                    .sorted()
                    .unique()
                    .join("\n");

                let path = dir.unwrap().join(namespace).with_extension("hpp");

                info!(
                    "Creating namespace glob include {path:?} for {} files",
                    contexts.len()
                );

                let mut file = File::create(path)?;

                writeln!(file, "#pragma once")?;
                file.write_all(str.as_bytes())?;

                Ok(())
            })?;
        Ok(())
    }
}

// Get root parent for a reference type, which is System.Object
// for generic sharing
fn get_root_parent<'a>(
    metadata: &mut Metadata<'a>,
    ty_def: &'a brocolib::global_metadata::Il2CppTypeDefinition,
) -> Option<&'a brocolib::global_metadata::Il2CppTypeDefinition> {
    // is reference type
    // only make generic spatialization
    if ty_def.is_value_type() || ty_def.is_enum_type() {
        return Some(ty_def);
    }

    let mut parent_index = ty_def.parent_index;
    loop {
        if parent_index == u32::MAX {
            break;
        }

        let parent_ty = metadata
            .metadata_registration
            .types
            .get(parent_index as usize)
            .unwrap();
        if let TypeData::TypeDefinitionIndex(parent_tdi) = parent_ty.data {
            let parent_ty_def = &metadata.metadata.global_metadata.type_definitions[parent_tdi];

            parent_index = parent_ty_def.parent_index;
        } else {
            break;
        }
    }
    if parent_index == u32::MAX {
        return Some(ty_def);
    }

    let parent_ty = metadata
        .metadata_registration
        .types
        .get(parent_index as usize)
        .unwrap();
    if let TypeData::TypeDefinitionIndex(parent_tdi) = parent_ty.data {
        Some(&metadata.metadata.global_metadata.type_definitions[parent_tdi])
    } else {
        Some(ty_def)
    }
}
