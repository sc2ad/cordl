use std::collections::HashMap;

use il2cpp_binary::{CodeRegistration, MetadataRegistration};

pub struct Metadata<'a> {
    pub metadata: &'a il2cpp_metadata_raw::Metadata<'a>,
    pub metadata_registration: &'a MetadataRegistration,
    pub code_registration: &'a CodeRegistration<'a>,
    
    // Method index in metadata
    pub methods_size_map: HashMap<usize, usize>,
    pub methods_address_sorted: HashMap<usize, u64>
}