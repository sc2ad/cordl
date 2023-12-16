use color_eyre::Result;
use log::{info, trace, warn};
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
};

use crate::generate::{
    cpp_type::CppType,
    cs_type::VALUE_TYPE_WRAPPER_SIZE,
    members::{CppConstructorDecl, CppLine, CppMember, CppMethodDecl, CppParam},
    metadata::{Il2cppFullName, Metadata},
};

static EQUIVALENTS: LazyLock<HashMap<&str, &str>> = LazyLock::new(|| {
    HashMap::from([
        ("System.RuntimeType", "Il2CppReflectionRuntimeType"),
        ("System.MonoType", "Il2CppReflectionMonoType"),
        ("System.Reflection.EventInfo", "Il2CppReflectionEvent"),
        ("System.Reflection.MonoEvent", "Il2CppReflectionMonoEvent"),
        (
            "System.Reflection.MonoEventInfo",
            "Il2CppReflectionMonoEventInfo",
        ),
        ("System.Reflection.MonoField", "Il2CppReflectionField"),
        ("System.Reflection.MonoProperty", "Il2CppReflectionProperty"),
        ("System.Reflection.MonoMethod", "Il2CppReflectionMethod"),
        (
            "System.Reflection.MonoGenericMethod",
            "Il2CppReflectionGenericMethod",
        ),
        ("System.Reflection.MonoMethodInfo", "Il2CppMethodInfo"),
        ("System.Reflection.MonoPropertyInfo", "Il2CppPropertyInfo"),
        (
            "System.Reflection.ParameterInfo",
            "Il2CppReflectionParameter",
        ),
        ("System.Reflection.Module", "Il2CppReflectionModule"),
        (
            "System.Reflection.AssemblyName",
            "Il2CppReflectionAssemblyName",
        ),
        ("System.Reflection.Assembly", "Il2CppReflectionAssembly"),
        (
            "System.Reflection.Emit.UnmanagedMarshal",
            "Il2CppReflectionMarshal",
        ),
        ("System.Reflection.Pointer", "Il2CppReflectionPointer"),
        ("System.Threading.InternalThread", "Il2CppInternalThread"),
        ("System.Threading.Thread", "Il2CppThread"),
        ("System.Exception", "Il2CppException"),
        ("System.SystemException", "Il2CppSystemException"),
        ("System.ArgumentException", "Il2CppArgumentException"),
        ("System.TypedReference", "Il2CppTypedRef"),
        ("System.Delegate", "Il2CppDelegate"),
        ("System.MarshalByRefObject", "Il2CppMarshalByRefObject"),
        ("System.__Il2CppComObject", "Il2CppComObject"),
        ("System.AppDomain", "Il2CppAppDomain"),
        ("System.Diagnostics.StackFrame", "Il2CppStackFrame"),
        (
            "System.Globalization.DateTimeFormatInfo",
            "Il2CppDateTimeFormatInfo",
        ),
        (
            "System.Globalization.NumberFormatInfo",
            "Il2CppNumberFormatInfo",
        ),
        ("System.Globalization.CultureInfo", "Il2CppCultureInfo"),
        ("System.Globalization.RegionInfo", "Il2CppRegionInfo"),
        (
            "System.Runtime.InteropServices.SafeHandle",
            "Il2CppSafeHandle",
        ),
        ("System.Text.StringBuilder", "Il2CppStringBuilder"),
        ("System.Net.SocketAddress", "Il2CppSocketAddress"),
        ("System.Globalization.SortKey", "Il2CppSortKey"),
        (
            "System.Runtime.InteropServices.ErrorWrapper",
            "Il2CppErrorWrapper",
        ),
        (
            "System.Runtime.Remoting.Messaging.AsyncResult",
            "Il2CppAsyncResult",
        ),
        ("System.MonoAsyncCall", "Il2CppAsyncCall"),
        (
            "System.Reflection.ManifestResourceInfo",
            "Il2CppManifestResourceInfo",
        ),
        (
            "System.Runtime.Remoting.Contexts.Context",
            "Il2CppAppContext",
        ),
    ])
});

pub fn register_il2cpp_types(metadata: &mut Metadata) -> Result<()> {
    info!("Registering il2cpp type handler!");

    for (cordl_t, il2cpp_t) in EQUIVALENTS.iter() {
        info!("Registering il2cpp type handler {cordl_t} to {il2cpp_t}");

        let (cordl_t_ns, cordl_t_name) = cordl_t.rsplit_once('.').expect("No namespace?");
        let il2cpp_name = Il2cppFullName(cordl_t_ns, cordl_t_name);

        let cordl_tdi = metadata.name_to_tdi.get(&il2cpp_name);

        match cordl_tdi {
            Some(cordl_tdi) => {
                metadata.custom_type_handler.insert(
                    *cordl_tdi,
                    Box::new(|cpp_type| il2cpp_alias_handler(cpp_type, cordl_t, il2cpp_t)),
                );
            }
            None => {
                warn!("Could not find TDI for {cordl_t}");
            }
        }
    }

    Ok(())
}

fn il2cpp_alias_handler(cpp_type: &mut CppType, cordl_t: &str, il2cpp_t: &str) {
    trace!("Replacing {cordl_t} for il2cpp il2cpp_t for type {il2cpp_t}");

    // there is an il2cpp api il2cpp_t configured for this type,
    // we should emit some conversion operators for that

    if cpp_type.is_value_type {
        value_type_convert(cpp_type, il2cpp_t);
    } else {
        reference_type_convert(cpp_type, il2cpp_t);
    }
}

fn reference_type_convert(cpp_type: &mut CppType, il2cpp_t: &str) {
    let cpp_name = cpp_type.cpp_name();

    let operator_body = format!("return static_cast<{il2cpp_t}*>(this->convert());");
    let conversion_operator = CppMethodDecl {
        cpp_name: Default::default(),
        instance: true,
        return_type: format!("{il2cpp_t}*"),

        brief: Some(format!("Conversion into il2cpp il2cpp_t {il2cpp_t}")),
        body: Some(vec![Arc::new(CppLine::make(operator_body))]), // TODO:
        is_const: false,
        is_constexpr: true,
        is_virtual: false,
        is_operator: true,
        is_inline: true,
        is_no_except: true,
        parameters: vec![],
        prefix_modifiers: vec![],
        suffix_modifiers: vec![],
        template: None,
    };

    let const_operator_body = format!("return static_cast<{il2cpp_t} const*>(this->convert());");
    let const_conversion_operator = CppMethodDecl {
        cpp_name: Default::default(),
        instance: true,
        return_type: format!("{il2cpp_t} const*"),

        brief: Some(format!("Conversion into il2cpp il2cpp_t {il2cpp_t}")),
        body: Some(vec![Arc::new(CppLine::make(const_operator_body))]), // TODO:
        is_const: true,
        is_constexpr: true,
        is_virtual: false,
        is_operator: true,
        is_inline: true,
        is_no_except: true,
        parameters: vec![],
        prefix_modifiers: vec![],
        suffix_modifiers: vec![],
        template: None,
    };

    let il2cpp_t_constructor = CppConstructorDecl {
        cpp_name: cpp_name.clone(),
        parameters: vec![CppParam {
            name: "il2cpp_ptr".to_string(),
            modifiers: "".to_string(),
            ty: format!("{il2cpp_t}*").to_string(),
            def_value: None,
        }],
        template: None,
        is_constexpr: true,
        is_explicit: false,
        is_default: false,
        is_no_except: true,
        is_delete: false,
        is_protected: false,

        // use the void* ctor overload
        base_ctor: Some((
            cpp_name.clone(),
            "static_cast<void*>(il2cpp_ptr)".to_string(),
        )),
        initialized_values: HashMap::new(),
        brief: None,
        body: Some(vec![]),
    };

    cpp_type
        .declarations
        .push(CppMember::MethodDecl(conversion_operator).into());
    cpp_type
        .declarations
        .push(CppMember::MethodDecl(const_conversion_operator).into());

    cpp_type
        .declarations
        .push(CppMember::ConstructorDecl(il2cpp_t_constructor).into());
}

fn value_type_convert(cpp_type: &mut CppType, il2cpp_t: &str) {
    let cpp_name = cpp_type.cpp_name();

    let operator_body = format!("return *static_cast<{il2cpp_t}*>(this->convert());");
    let conversion_operator = CppMethodDecl {
        cpp_name: Default::default(),
        instance: true,
        return_type: il2cpp_t.to_string(),

        brief: Some(format!("Conversion into il2cpp il2cpp_t {il2cpp_t}")),
        body: Some(vec![Arc::new(CppLine::make(operator_body))]), // TODO:
        is_const: false,
        is_constexpr: true,
        is_virtual: false,
        is_operator: true,
        is_inline: true,
        is_no_except: true,
        parameters: vec![],
        prefix_modifiers: vec![],
        suffix_modifiers: vec![],
        template: None,
    };

    let const_operator_body = format!("return *static_cast<{il2cpp_t} const*>(this->convert());");
    let const_conversion_operator = CppMethodDecl {
        cpp_name: Default::default(),
        instance: true,
        return_type: il2cpp_t.to_string(),

        brief: Some(format!("Conversion into il2cpp il2cpp_t {il2cpp_t}")),
        body: Some(vec![Arc::new(CppLine::make(const_operator_body))]), // TODO:
        is_const: true,
        is_constexpr: true,
        is_virtual: false,
        is_operator: true,
        is_inline: true,
        is_no_except: true,
        parameters: vec![],
        prefix_modifiers: vec![],
        suffix_modifiers: vec![],
        template: None,
    };

    let il2cpp_t_constructor = CppConstructorDecl {
        cpp_name: cpp_name.clone(),
        parameters: vec![CppParam {
            name: "il2cpp_eq".to_string(),
            modifiers: "".to_string(),
            ty: format!("{il2cpp_t} const&").to_string(),
            def_value: None,
        }],
        template: None,
        is_constexpr: true,
        is_protected: false,
        is_explicit: false,
        is_default: false,
        is_no_except: true,
        is_delete: false,
        // use the array<byte, sz> ctor overload
        base_ctor: Some((
            cpp_name.clone(),
            format!("std::bit_cast<std::array<std::byte, {VALUE_TYPE_WRAPPER_SIZE}>>(il2cpp_eq)"),
        )),
        initialized_values: HashMap::new(),
        brief: None,
        body: Some(vec![]),
    };

    cpp_type
        .declarations
        .push(CppMember::MethodDecl(conversion_operator).into());
    cpp_type
        .declarations
        .push(CppMember::MethodDecl(const_conversion_operator).into());

    cpp_type
        .declarations
        .push(CppMember::ConstructorDecl(il2cpp_t_constructor).into());
}
