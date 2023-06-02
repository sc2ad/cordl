use std::path::PathBuf;

use color_eyre::Result;

use crate::generate::{
    context::CppContextCollection,
    cpp_type::CppType,
    members::CppInclude,
    metadata::{Il2cppFullName, Metadata},
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

    let (_tag, _unity_cpp_context) = cpp_context_collection
        .get()
        .iter()
        .find(|(_, c)| {
            c.get_types()
                .iter()
                .any(|(_, t)| t.name == "Object" && t.namespace == "UnityEngine")
        })
        .unwrap_or_else(|| panic!("No UnityEngine.Object type found!"));

    let unity_object_tdi = metadata
        .name_to_tdi
        .get(&Il2cppFullName("UnityEngine", "Object"))
        .expect("No UnityEngine.Object TDI found");

    metadata
        .custom_type_handler
        .insert(*unity_object_tdi, Box::new(unity_object_handler));

    Ok(())
}

fn unity_object_handler(cpp_type: &mut CppType) {
    println!("Found UnityEngine.Object type, adding UnityW!");
    cpp_type.inherit = vec!["::UnityW".to_owned()];

    let path = PathBuf::from(r"beatsaber-hook/shared/utils/unityw.hpp");

    cpp_type
        .requirements
        .required_includes
        .insert(CppInclude::new(path));
}
