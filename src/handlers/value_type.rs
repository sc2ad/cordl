use std::rc::Rc;

use color_eyre::Result;

use crate::generate::{
    cpp_type::CppType,
    members::CppMember,
    metadata::{Il2cppFullName, Metadata},
};

pub fn register_value_type(metadata: &mut Metadata) -> Result<()> {
    println!("Registering unity handler!");
    register_value_type_object_handler(metadata)?;

    Ok(())
}

fn register_value_type_object_handler(metadata: &mut Metadata) -> Result<()> {
    println!("Registering System.ValueType handler!");

    let value_type_tdi = metadata
        .name_to_tdi
        .get(&Il2cppFullName("System", "ValueType"))
        .expect("No System.ValueType TDI found");

    metadata
        .custom_type_handler
        .insert(*value_type_tdi, Box::new(value_type_handler));

    Ok(())
}

fn value_type_handler(cpp_type: &mut CppType) {
    println!("Found System.ValueType type, removing inheritance!");

    // Should not inherit wrapper types!
    cpp_type.inherit.clear();
}
