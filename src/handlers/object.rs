use color_eyre::Result;
use log::info;


use crate::generate::{
    cpp_type::CppType,
    cs_type::{OBJECT_WRAPPER_TYPE},
    members::{CppMember},
    metadata::{Il2cppFullName, Metadata},
};

pub fn register_system(metadata: &mut Metadata) -> Result<()> {
    info!("Registering system handler!");
    register_system_object_type_handler(metadata)?;

    Ok(())
}

fn register_system_object_type_handler(metadata: &mut Metadata) -> Result<()> {
    info!("Registering System.Object handler!");

    let system_object_tdi = metadata
        .name_to_tdi
        .get(&Il2cppFullName("System", "Object"))
        .expect("No System.Object TDI found");

    metadata
        .custom_type_handler
        .insert(*system_object_tdi, Box::new(system_object_handler));

    Ok(())
}

fn system_object_handler(cpp_type: &mut CppType) {
    info!("Found System.Object type, adding systemW!");
    cpp_type.inherit = vec![OBJECT_WRAPPER_TYPE.to_string()];

    cpp_type.requirements.need_wrapper();

    // Remove field because it does not size properly and is not necessary
    cpp_type
        .declarations
        .retain(|t| !matches!(t.as_ref(), CppMember::FieldDecl(_)));
}
