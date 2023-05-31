use std::collections::{HashMap, HashSet};

use brocolib::global_metadata::{Il2CppTypeDefinition, MethodIndex, TypeDefinitionIndex};
use itertools::Itertools;

use super::cpp_type::CppType;

pub struct MethodCalculations {
    pub estimated_size: usize,
    pub addrs: u64,
}

#[derive(Clone)]
pub struct TypeDefinitionPair<'a> {
    pub ty: &'a Il2CppTypeDefinition,
    pub tdi: TypeDefinitionIndex,
}

impl<'a> TypeDefinitionPair<'a> {
    fn new(ty: &'a Il2CppTypeDefinition, tdi: TypeDefinitionIndex) -> TypeDefinitionPair {
        TypeDefinitionPair { ty, tdi }
    }
}

pub type TypeHandlerFn = Box<dyn Fn(&mut CppType)>;
pub type Il2cppNamespace<'a> = &'a str;
pub type Il2cppName<'a> = &'a str;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Il2cppFullName<'a>(pub Il2cppNamespace<'a>, pub Il2cppName<'a>);

pub struct Metadata<'a> {
    pub metadata: &'a brocolib::Metadata<'a, 'a>,
    pub metadata_registration: &'a brocolib::runtime_metadata::Il2CppMetadataRegistration,
    pub code_registration: &'a brocolib::runtime_metadata::Il2CppCodeRegistration<'a>,

    // Method index in metadata
    pub method_calculations: HashMap<MethodIndex, MethodCalculations>,
    pub parent_to_child_map: HashMap<TypeDefinitionIndex, Vec<TypeDefinitionPair<'a>>>,
    pub child_to_parent_map: HashMap<TypeDefinitionIndex, TypeDefinitionPair<'a>>,

    //
    pub custom_type_handler: HashMap<TypeDefinitionIndex, TypeHandlerFn>,
    pub name_to_tdi: HashMap<Il2cppFullName<'a>, TypeDefinitionIndex>,
    pub blacklisted_types: HashSet<TypeDefinitionIndex>,
}

impl<'a> Metadata<'a> {
    pub fn parse(&mut self) {
        let gm = &self.metadata.global_metadata;
        self.parse_name_tdi(gm);
        self.parse_type_hierarchy(gm);
        self.parse_method_size(gm);
    }

    fn parse_type_hierarchy(&mut self, gm: &'a brocolib::global_metadata::GlobalMetadata) {
        // self.parentToChildMap = childToParent
        //     .into_iter()
        //     .map(|(p, p_tdi, c)| (p_tdi, c))
        //     .collect();

        // child -> parent
        let parent_to_child_map: Vec<(TypeDefinitionPair<'a>, Vec<TypeDefinitionPair<'a>>)> = gm
            .type_definitions
            .as_vec()
            .iter()
            .enumerate()
            .filter_map(|(tdi, td)| {
                if td.nested_type_count == 0 {
                    return None;
                }

                let nested_types: Vec<TypeDefinitionPair> = td
                    .nested_types(self.metadata)
                    .iter()
                    .map(|&nested_tdi| {
                        let nested_td = &gm.type_definitions[nested_tdi];
                        TypeDefinitionPair::new(nested_td, nested_tdi)
                    })
                    .collect();

                if nested_types.is_empty() {
                    return None;
                }
                Some((
                    TypeDefinitionPair::new(td, TypeDefinitionIndex::new(tdi as u32)),
                    nested_types,
                ))
            })
            .collect();

        let child_to_parent_map: Vec<(&TypeDefinitionPair<'a>, &TypeDefinitionPair<'a>)> =
            parent_to_child_map
                .iter()
                .flat_map(|(p, children)| {
                    let reverse = children.iter().map(|c| (c, p)).collect_vec();

                    reverse
                })
                .collect();

        self.child_to_parent_map = child_to_parent_map
            .into_iter()
            .map(|(c, p)| (c.tdi, p.clone()))
            .collect();

        self.parent_to_child_map = parent_to_child_map
            .into_iter()
            .map(|(p, c)| (p.tdi, c.into_iter().collect_vec()))
            .collect();
    }

    fn parse_method_size(&mut self, gm: &brocolib::global_metadata::GlobalMetadata) {
        // method index -> address
        // sorted by address
        let mut method_addresses_sorted: Vec<u64> = self
            .code_registration
            .code_gen_modules
            .iter()
            .flat_map(|m| &m.method_pointers)
            .copied()
            .collect();
        method_addresses_sorted.sort();
        // address -> method index in sorted list
        let method_addresses_sorted_map: HashMap<u64, usize> = method_addresses_sorted
            .iter()
            .enumerate()
            .map(|(index, m_ptr)| (*m_ptr, index))
            .collect();

        self.method_calculations = self
            .metadata
            .runtime_metadata
            .code_registration
            .code_gen_modules
            .iter()
            .flat_map(|cgm| {
                let img = gm
                    .images
                    .as_vec()
                    .iter()
                    .find(|i| cgm.name == i.name(self.metadata))
                    .unwrap();
                let mut method_calculations: HashMap<MethodIndex, MethodCalculations> =
                    HashMap::new();
                for ty in img.types(self.metadata) {
                    for (i, method) in ty.methods(self.metadata).iter().enumerate() {
                        let method_index = MethodIndex::new(ty.method_start.index() + i as u32);
                        let method_pointer_index = method.token.rid() as usize - 1;
                        let method_pointer =
                            *cgm.method_pointers.get(method_pointer_index).unwrap();

                        let sorted_address_index =
                            *method_addresses_sorted_map.get(&method_pointer).unwrap();
                        let next_method_pointer = method_addresses_sorted
                            .get(sorted_address_index + 1)
                            .cloned()
                            .unwrap_or(0);

                        method_calculations.insert(
                            method_index,
                            MethodCalculations {
                                estimated_size: method_pointer.abs_diff(next_method_pointer)
                                    as usize,
                                addrs: method_pointer,
                            },
                        );
                    }
                }
                method_calculations
            })
            .collect();
    }

    fn parse_name_tdi(&mut self, gm: &brocolib::global_metadata::GlobalMetadata) {
        self.name_to_tdi = gm
            .type_definitions
            .as_vec()
            .iter()
            .enumerate()
            .map(|(tdi, td)| {
                (
                    Il2cppFullName(td.namespace(self.metadata), td.name(self.metadata)),
                    TypeDefinitionIndex::new(tdi as u32),
                )
            })
            .collect();
    }
}
