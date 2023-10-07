use std::rc::Rc;

use color_eyre::Result;

use crate::generate::{
    cpp_type::CppType,
    cs_type::{ENUM_WRAPPER_TYPE, VALUE_WRAPPER_TYPE},
    members::CppMember,
    metadata::{Il2cppFullName, Metadata},
};

use log::info;

pub fn register_value_type(metadata: &mut Metadata) -> Result<()> {
    info!("Registering unity handler!");
    register_value_type_object_handler(metadata)?;

    Ok(())
}

fn register_value_type_object_handler(metadata: &mut Metadata) -> Result<()> {
    info!("Registering System.ValueType handler!");

    let value_type_tdi = metadata
        .name_to_tdi
        .get(&Il2cppFullName("System", "ValueType"))
        .expect("No System.ValueType TDI found");
    let enum_type_tdi = metadata
        .name_to_tdi
        .get(&Il2cppFullName("System", "Enum"))
        .expect("No System.ValueType TDI found");

    metadata
        .custom_type_handler
        .insert(*value_type_tdi, Box::new(value_type_handler));
    metadata
        .custom_type_handler
        .insert(*enum_type_tdi, Box::new(enum_type_handler));

    Ok(())
}

fn unified_type_handler(cpp_type: &mut CppType, base_ctor: &str) {
    // We don't replace parent anymore
    // cpp_type.inherit = vec![base_ctor.to_string()];

    // Fixup ctor call
    cpp_type
        .implementations
        .retain_mut(|d| !matches!(d.as_ref(), CppMember::ConstructorImpl(_)));
    cpp_type
        .declarations
        .iter_mut()
        .filter(|t| matches!(t.as_ref(), CppMember::ConstructorDecl(_)))
        .for_each(|d| {
            let CppMember::ConstructorDecl(constructor) = Rc::get_mut(d).unwrap() else {
                panic!()
            };

            // We don't replace base ctor anymore
            // constructor.base_ctor = Some((base_ctor.to_string(), "".to_string()));
            constructor.body = Some(vec![]);
            constructor.is_constexpr = true;
        });

    // remove all method decl/impl
    cpp_type
        .declarations
        .retain(|t| !matches!(t.as_ref(), CppMember::MethodDecl(_)));
    // remove all method decl/impl
    cpp_type
        .implementations
        .retain(|t| !matches!(t.as_ref(), CppMember::MethodImpl(_)));

    // Remove method size structs
    cpp_type.nonmember_implementations.clear();
}
fn value_type_handler(cpp_type: &mut CppType) {
    info!("Found System.ValueType, removing inheritance!");
    unified_type_handler(
        cpp_type,
        format!(
            "{VALUE_WRAPPER_TYPE}<0x{:x}>",
            cpp_type.calculated_size.unwrap()
        )
        .as_str(),
    );
}
fn enum_type_handler(cpp_type: &mut CppType) {
    info!("Found System.Enum type, removing inheritance!");
    unified_type_handler(
        cpp_type,
        format!(
            "{ENUM_WRAPPER_TYPE}<0x{:x}>",
            cpp_type.calculated_size.unwrap()
        )
        .as_str(),
    );
}
