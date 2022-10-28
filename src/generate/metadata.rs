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
        let mut method_addresses_sorted: Vec<u64> = self
            .code_registration
            .code_gen_modules
            .iter()
            .flat_map(|m| &m.method_pointers)
            .copied()
            .collect();
        method_addresses_sorted.sort();

        self.method_calculations = self
            .code_registration
            .code_gen_modules
            .iter()
            .flat_map(|cgm| {
                let img = self.metadata
                        .images
                        .iter()
                        .find(|i| cgm.name == self.metadata.get_str(i.name_index).unwrap())
                        .unwrap();
                let mut method_calculations: HashMap<MethodIndex, MethodCalculations> =
                    HashMap::new();
                for i in 0..img.exported_type_count {
                    let ty = self
                        .metadata
                        .type_definitions
                        .get((img.exported_type_start + i) as usize)
                        .unwrap();

                    for m in 0..ty.method_count {
                        let method_index = ty.method_start + m as u32;
                        let method = self.metadata.methods.get(method_index as usize).unwrap();

                        let method_pointer_index = (method.token & 0xFFFFFF) as usize;
                        let method_pointer =
                            *cgm.method_pointers.get(method_pointer_index).unwrap();

                        let sorted_address_num = method_addresses_sorted
                            .iter()
                            .position(|m| *m == method_pointer)
                            .unwrap();
                        let next_method_pointer = *cgm
                            .method_pointers
                            .get(sorted_address_num + 1)
                            .unwrap_or(&0);

                        // let next_method_pointer = self
                        //     .metadata
                        //     .methods
                        //     .get(method_index + 1)
                        //     .map(|n| {
                        //         cgm.method_pointers
                        //             .get((n.token & 0xFFFFFF) as usize)
                        //             .unwrap()
                        //     })
                        //     .map(|&n| *cgm.method_pointers.get(n as usize).unwrap())
                        //     .unwrap_or(0);
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
