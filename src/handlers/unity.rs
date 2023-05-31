use std::path::PathBuf;

use anyhow::anyhow;
use brocolib::{global_metadata::TypeDefinitionIndex, runtime_metadata::TypeData};
use color_eyre::Result;

use crate::generate::{
    context::CppContextCollection, cpp_type::CppType, members::CppInclude, metadata::Metadata,
};

pub fn register_unity(
    cpp_context_collection: &CppContextCollection,
    metadata: &mut Metadata,
) -> Result<()> {
    println!("Registering unity handler!");
    register_unity_object_type_handler(cpp_context_collection, metadata)?;

    Ok(())
}

fn register_unity_object_type_handler(
    cpp_context_collection: &CppContextCollection,
    metadata: &mut Metadata,
) -> Result<()> {
    println!("Registering UnityEngine.Object handler!");

    let (tag, _unity_cpp_context) = cpp_context_collection
        .get()
        .iter()
        .find(|(_, c)| {
            c.get_types()
                .iter()
                .any(|(_, t)| t.name == "Object" && t.namespace == "UnityEngine")
        })
        .unwrap_or_else(|| panic!("No UnityEngine.Object type found!"));

    if let TypeData::TypeDefinitionIndex(tdi) = tag {
        metadata
            .custom_type_handler
            .insert(*tdi, Box::new(unity_object_handler));
    }

    Ok(())
}

fn unity_object_handler(cpp_type: &mut CppType) {
    println!("Found UnityEngine.Object type, adding UnityW!");
    cpp_type.inherit.push("::UnityW".to_owned());

    let path = PathBuf::from(r"beatsaber-hook/shared/utils/unityw.hpp");

    cpp_type
        .requirements
        .required_includes
        .insert(CppInclude::new(path));
}
