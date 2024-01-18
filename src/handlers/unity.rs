use brocolib::{global_metadata::TypeDefinitionIndex, runtime_metadata::Il2CppType};
use color_eyre::Result;
use log::info;
use std::{path::PathBuf, rc::Rc};

use crate::{
    data::name_components::NameComponents,
    generate::{
        context_collection::CppContextCollection,
        cpp_type::CppType,
        members::{CppInclude, CppMember},
        metadata::{Il2cppFullName, Metadata, TypeUsage},
        type_extensions::TypeDefinitionExtensions,
    },
};

pub fn register_unity(metadata: &mut Metadata) -> Result<()> {
    info!("Registering unity handler!");
    // register_unity_object_type_handler(metadata)?;
    register_unity_object_type_resolve_handler(metadata)?;

    Ok(())
}

fn register_unity_object_type_resolve_handler(metadata: &mut Metadata) -> Result<()> {
    info!("Registering UnityEngine.Object resolve handler!");

    let unity_object_tdi = *metadata
        .name_to_tdi
        .get(&Il2cppFullName("UnityEngine", "Object"))
        .expect("No UnityEngine.Object TDI found");

    metadata
        .custom_type_resolve_handler
        .push(Box::new(move |a, b, c, d, e, f| {
            unity_object_resolve_handler(a, b, c, d, e, f, unity_object_tdi)
        }));

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

fn unity_object_resolve_handler(
    original: NameComponents,
    cpp_type: &CppType,
    _ctx_collection: &CppContextCollection,
    metadata: &Metadata,
    _typ: &Il2CppType,
    typ_usage: TypeUsage,
    unity_tdi: TypeDefinitionIndex,
) -> NameComponents {
    if !matches!(
        typ_usage,
        TypeUsage::FieldName
            | TypeUsage::PropertyName
            | TypeUsage::GenericArg
            | TypeUsage::ReturnType
    ) {
        return original;
    }

    let tdi = cpp_type.self_tag.get_tdi();
    let td = &metadata.metadata.global_metadata.type_definitions[tdi];

    let unity_td = &metadata.metadata.global_metadata.type_definitions[unity_tdi];

    if !td.is_assignable_to(unity_td, metadata.metadata) {
        return original;
    }

    NameComponents {
        namespace: Some("".to_string()),
        declaring_types: None,
        name: "UnityW".to_string(),
        generics: Some(vec![original.remove_pointer().combine_all()]),
        is_pointer: false,
    }
}

fn unity_object_handler(cpp_type: &mut CppType) {
    info!("Found UnityEngine.Object type, adding UnityW!");
    cpp_type.inherit = vec!["bs_hook::UnityW".to_owned()];

    let path = PathBuf::from(r"beatsaber-hook/shared/utils/unityw.hpp");

    cpp_type
        .requirements
        .add_def_include(None, CppInclude::new_exact(path));

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
