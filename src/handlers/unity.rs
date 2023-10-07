use std::{path::PathBuf, rc::Rc};
use log::info;
use color_eyre::Result;

use crate::generate::{
    cpp_type::CppType,
    members::{CppInclude, CppMember},
    metadata::{Il2cppFullName, Metadata},
};

pub fn register_unity(metadata: &mut Metadata) -> Result<()> {
    info!("Registering unity handler!");
    register_unity_object_type_handler(metadata)?;

    Ok(())
}

fn register_unity_object_type_handler(metadata: &mut Metadata) -> Result<()> {
    info!("Registering UnityEngine.Object handler!");

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
    info!("Found UnityEngine.Object type, adding UnityW!");
    cpp_type.inherit = vec!["bs_hook::UnityW".to_owned()];

    let path = PathBuf::from(r"beatsaber-hook/shared/utils/unityw.hpp");

    cpp_type
        .requirements
        .add_include(None, CppInclude::new_exact(path));

    // Fixup ctor call declarations
    cpp_type
        .declarations
        .iter_mut()
        .filter(|t| matches!(t.as_ref(), CppMember::ConstructorDecl(_)))
        .for_each(|d| {
            let CppMember::ConstructorDecl(constructor) = Rc::get_mut(d).unwrap() else {
                panic!()
            };

            if let Some(base_ctor) = &mut constructor.base_ctor {
                base_ctor.0 = "UnityW".to_string();
            }
        });
    // Fixup ctor call implementations
    cpp_type
        .implementations
        .iter_mut()
        .filter(|t| matches!(t.as_ref(), CppMember::ConstructorImpl(_)))
        .for_each(|d| {
            let CppMember::ConstructorImpl(constructor) = Rc::get_mut(d).unwrap() else {
                panic!()
            };

            if let Some(base_ctor) = &mut constructor.base_ctor {
                base_ctor.0 = "UnityW".to_string();
            }
        });
}
