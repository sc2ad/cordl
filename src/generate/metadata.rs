use std::collections::HashMap;

use il2cpp_binary::{CodeRegistration, MetadataRegistration};
use il2cpp_metadata_raw::MethodIndex;

pub struct MethodCalculations {
    pub estimated_size: usize,
    pub addrs: u64,
}

pub struct Metadata<'a> {
    pub metadata: &'a il2cpp_metadata_raw::Metadata<'a>,
    pub metadata_registration: &'a MetadataRegistration,
    pub code_registration: &'a CodeRegistration<'a>,

    // Method index in metadata
    pub method_calculations: HashMap<MethodIndex, MethodCalculations>,
}

impl<'a> Metadata<'a> {
    pub fn parse(&mut self) {
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
