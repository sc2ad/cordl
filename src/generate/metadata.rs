use std::collections::HashMap;

use il2cpp_binary::{CodeRegistration, MetadataRegistration, Type, TypeData};
use il2cpp_metadata_raw::{Il2CppTypeDefinition, MethodIndex, TypeDefinitionIndex};
use itertools::Itertools;

use super::{
    constants::{TYPE_ATTRIBUTE_INTERFACE, TYPE_ATTRIBUTE_NESTED_PUBLIC},
    cpp_type::CppType,
    cs_type::CSType,
};

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

pub struct Metadata<'a> {
    pub metadata: &'a il2cpp_metadata_raw::Metadata<'a>,
    pub metadata_registration: &'a MetadataRegistration,
    pub code_registration: &'a CodeRegistration<'a>,

    // Method index in metadata
    pub method_calculations: HashMap<MethodIndex, MethodCalculations>,
    pub parent_to_child_map: HashMap<TypeDefinitionIndex, Vec<TypeDefinitionPair<'a>>>,
    pub child_to_parent_map: HashMap<TypeDefinitionIndex, TypeDefinitionPair<'a>>,
}

impl<'a> Metadata<'a> {
    pub fn parse(&mut self) {
        // child -> parent
        let parent_to_child_map: Vec<(TypeDefinitionPair<'a>, Vec<TypeDefinitionPair<'a>>)> = self
            .metadata
            .type_definitions
            .iter()
            .enumerate()
            .filter_map(|(tdi, td)| {
                if td.nested_type_count == 0 {
                    return None;
                }

                let mut nested_types: Vec<TypeDefinitionPair> =
                    Vec::with_capacity(td.nested_type_count as usize);
                for i in td.nested_types_start..td.nested_types_start + td.nested_type_count as u32
                {
                    let nested_ti = self.metadata.nested_types.get(i as usize).unwrap().clone();
                    let nested_t = self
                        .metadata_registration
                        .types
                        .get(nested_ti as usize)
                        .unwrap();

                    match nested_t.data {
                        TypeData::TypeDefinitionIndex(_) => (),
                        _ => {
                            // TODO: handle
                            continue;
                        }
                    }
                    let nested_tdi = CppType::get_tag_tdi(nested_t.data);
                    let nested_td = self
                        .metadata
                        .type_definitions
                        .get(nested_tdi as usize)
                        .unwrap();

                    nested_types.push(TypeDefinitionPair::new(nested_td, nested_tdi));
                }

                if nested_types.len() == 0 {
                    return None;
                }

                Some((
                    TypeDefinitionPair::new(td, tdi as TypeDefinitionIndex),
                    nested_types,
                ))
            })
            .collect();

        let child_to_parent_map: Vec<(&TypeDefinitionPair<'a>, &TypeDefinitionPair<'a>)> =
            parent_to_child_map
                .iter()
                .map(|(p, children)| {
                    let reverse = children.iter().map(|c| (c, p)).collect_vec();

                    reverse
                })
                .flatten()
                .collect();

        self.child_to_parent_map = child_to_parent_map
            .into_iter()
            .map(|(c, p)| (c.tdi, p.clone()))
            .collect();

        self.parent_to_child_map = parent_to_child_map
            .into_iter()
            .map(|(p, c)| (p.tdi, c.into_iter().map(|c| c).collect_vec()))
            .collect();

        // self.parentToChildMap = childToParent
        //     .into_iter()
        //     .map(|(p, p_tdi, c)| (p_tdi, c))
        //     .collect();

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
            .code_registration
            .code_gen_modules
            .iter()
            .flat_map(|cgm| {
                let img = self
                    .metadata
                    .images
                    .iter()
                    .find(|i| cgm.name == self.metadata.get_str(i.name_index).unwrap())
                    .unwrap();
                let mut method_calculations: HashMap<MethodIndex, MethodCalculations> =
                    HashMap::new();
                for i in 0..img.type_count {
                    let ty = self
                        .metadata
                        .type_definitions
                        .get((img.type_start + i) as usize)
                        .unwrap();

                    for m in 0..ty.method_count {
                        let method_index = ty.method_start + m as u32;
                        let method = self.metadata.methods.get(method_index as usize).unwrap();

                        let method_pointer_index = ((method.token & 0xFFFFFF) - 1) as usize;
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
}
