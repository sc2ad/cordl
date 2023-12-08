use color_eyre::Result;
use log::info;
use std::{collections::HashMap, rc::Rc};

use crate::generate::{
    cpp_type::CppType,
    cs_type::{CSType, OBJECT_WRAPPER_TYPE},
    members::{CppConstructorDecl, CppMember, CppParam},
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
                base_ctor.0 = OBJECT_WRAPPER_TYPE.to_string();
            }
        });
    // Remove field because it does not size properly and is not necessary
    cpp_type
        .declarations
        .retain(|t| !matches!(t.as_ref(), CppMember::FieldDecl(_)));
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
                base_ctor.0 = OBJECT_WRAPPER_TYPE.to_string();
            }
        });

    cpp_type.declarations.push(
        CppMember::ConstructorDecl(CppConstructorDecl {
            cpp_name: cpp_type.cpp_name().clone(),
            parameters: vec![CppParam {
                name: "ptr".into(),
                ty: "void*".into(),
                modifiers: "".into(),
                def_value: None,
            }],
            template: None,
            is_constexpr: true,
            is_explicit: true,
            is_default: false,
            is_no_except: true,
            base_ctor: Some((OBJECT_WRAPPER_TYPE.into(), "ptr".into())),
            initialized_values: HashMap::new(),
            brief: None,
            body: Some(vec![]),
        })
        .into(),
    );
    cpp_type.declarations.push(
        CppMember::ConstructorDecl(CppConstructorDecl {
            cpp_name: cpp_type.cpp_name().clone(),
            parameters: vec![CppParam {
                name: "ptr".into(),
                ty: OBJECT_WRAPPER_TYPE.into(),
                modifiers: "".into(),
                def_value: None,
            }],
            template: None,
            is_constexpr: true,
            is_explicit: true,
            is_default: false,
            is_no_except: true,
            base_ctor: Some((OBJECT_WRAPPER_TYPE.into(), "ptr".into())),
            initialized_values: HashMap::new(),
            brief: None,
            body: Some(vec![]),
        })
        .into(),
    );

    // cpp_type.create_ref_default_constructor();
    cpp_type.create_ref_default_operators();
}
