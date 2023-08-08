use brocolib::{
    global_metadata::{Il2CppTypeDefinition, TypeDefinitionIndex},
    runtime_metadata::Il2CppType,
};

use super::{
    context_collection::{CppContextCollection, CppTypeTag},
    metadata::Metadata,
};

pub trait CsContextCollection {
    fn get_cpp_context_collection(&self) -> &CppContextCollection;
    fn get_mut_cpp_context_collection(&mut self) -> &mut CppContextCollection;

    fn alias_nested_types_il2cpp(
        &mut self,
        owner_ty: TypeDefinitionIndex,
        root_tag: CppTypeTag,
        metadata: &Metadata,
        nested: bool,
    );
}

impl CsContextCollection for CppContextCollection {
    fn get_cpp_context_collection(&self) -> &CppContextCollection {
        self
    }

    fn get_mut_cpp_context_collection(&mut self) -> &mut CppContextCollection {
        self
    }

    fn alias_nested_types_il2cpp(
        &mut self,
        owner_tdi: TypeDefinitionIndex,
        root_tag: CppTypeTag,
        metadata: &Metadata,
        nested: bool,
    ) {
        let owner_tag = CppTypeTag::TypeDefinitionIndex(owner_tdi);
        let owner_ty = &metadata.metadata.global_metadata.type_definitions[owner_tdi];

        for nested_type_tdi in owner_ty.nested_types(metadata.metadata) {
            // let nested_type = &metadata.metadata.global_metadata.type_definitions[*nested_type_tdi];

            let nested_tag = CppTypeTag::TypeDefinitionIndex(*nested_type_tdi);

            self.alias_type_to_context(nested_tag, root_tag, true);
            if nested {
                self.alias_type_to_parent(nested_tag, owner_tag, true);
            }
            self.alias_nested_types_il2cpp(*nested_type_tdi, root_tag, metadata, nested);
        }
    }
}
