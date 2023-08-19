use itertools::Itertools;
use pathdiff::diff_paths;

use crate::STATIC_CONFIG;

use super::{context::CppContext, cpp_type::CppType, writer::Writable};

use std::{
    collections::HashMap,
    hash::Hash,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Debug, Eq, Hash, PartialEq, Clone, Default, PartialOrd, Ord)]
pub struct CppTemplate {
    pub names: Vec<String>,
}
#[derive(Debug, Eq, Hash, PartialEq, Clone, Default, PartialOrd, Ord)]
pub struct CppStaticAssert {
    pub condition: String,
    pub message: Option<String>,
}

#[derive(Debug, Eq, Hash, PartialEq, Clone, Default, PartialOrd, Ord)]
pub struct CppLine {
    pub line: String,
}

impl From<String> for CppLine {
    fn from(value: String) -> Self {
        CppLine { line: value }
    }
}
impl From<&str> for CppLine {
    fn from(value: &str) -> Self {
        CppLine {
            line: value.to_string(),
        }
    }
}

impl CppLine {
    pub fn make(v: String) -> Self {
        CppLine { line: v }
    }
}

#[derive(Debug, Eq, Hash, PartialEq, Clone)]
pub struct CppForwardDeclareGroup {
    // TODO: Make this group lots into a single namespace
    pub namespace: Option<String>,
    pub items: Vec<CppForwardDeclare>,
    pub group_items: Vec<CppForwardDeclareGroup>,
}

#[derive(Debug, Eq, Hash, PartialEq, Clone)]
pub struct CppForwardDeclare {
    // TODO: Make this group lots into a single namespace
    pub is_struct: bool,
    pub cpp_namespace: Option<String>,
    pub cpp_name: String,
    pub templates: Option<CppTemplate>, // names of template arguments, T, TArgs etc.
    pub literals: Option<Vec<String>>,
}

#[derive(Debug, Eq, Hash, PartialEq, Clone)]
pub struct CppCommentedString {
    pub data: String,
    pub comment: Option<String>,
}

#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub struct CppInclude {
    pub include: PathBuf,
    pub system: bool,
}

#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub struct CppUsingAlias {
    pub result: String,
    pub alias: String,
    pub template: Option<CppTemplate>,
}

#[derive(Clone, Debug)]
pub enum CppMember {
    FieldDecl(CppFieldDecl),
    FieldImpl(CppFieldImpl),
    MethodDecl(CppMethodDecl),
    MethodImpl(CppMethodImpl),
    Property(CppPropertyDecl),
    Comment(CppCommentedString),
    ConstructorDecl(CppConstructorDecl),
    ConstructorImpl(CppConstructorImpl),
    CppUsingAlias(CppUsingAlias),
    CppLine(CppLine),
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct CppMethodData {
    pub estimated_size: usize,
    pub addrs: u64,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct CppMethodSizeStruct {
    pub cpp_method_name: String,
    pub complete_type_name: String,
    pub ret_ty: String,
    pub instance: bool,
    pub params: Vec<CppParam>,
    pub method_data: CppMethodData,

    pub template: Option<CppTemplate>,

    pub interface_clazz_of: String,
    pub is_final: bool,
    pub slot: Option<u16>,
}
#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct CppFieldDecl {
    pub cpp_name: String,
    pub field_ty: String,
    pub instance: bool,
    pub readonly: bool,
    pub const_expr: bool,
    pub value: Option<String>,
    pub brief_comment: Option<String>,
}
#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct CppFieldImpl {
    pub declaring_type: String,
    pub cpp_name: String,
    pub field_ty: String,
    pub readonly: bool,
    pub const_expr: bool,
    pub value: String,
}

impl From<CppFieldDecl> for CppFieldImpl {
    fn from(value: CppFieldDecl) -> Self {
        Self {
            const_expr: value.const_expr,
            readonly: value.readonly,
            cpp_name: value.cpp_name,
            field_ty: value.field_ty,
            declaring_type: "".to_string(),
            value: value.value.unwrap_or_default(),
        }
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct CppPropertyDecl {
    pub cpp_name: String,
    pub prop_ty: String,
    pub instance: bool,
    pub getter: Option<String>,
    pub setter: Option<String>,
    pub brief_comment: Option<String>,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct CppParam {
    pub name: String,
    pub ty: String,
    // TODO: Use bitflags to indicate these attributes
    // May hold:
    // const
    // May hold one of:
    // *
    // &
    // &&
    pub modifiers: String,
    pub def_value: Option<String>,
}

// TODO: Generics
#[derive(Clone, Debug)]
pub struct CppMethodDecl {
    pub cpp_name: String,
    pub return_type: String,
    pub parameters: Vec<CppParam>,
    pub instance: bool,
    pub template: Option<CppTemplate>,
    // TODO: Use bitflags to indicate these attributes
    // Holds unique of:
    // const
    // override
    // noexcept
    pub suffix_modifiers: Vec<String>,
    // Holds unique of:
    // constexpr
    // static
    // inline
    // explicit(...)
    // virtual
    pub prefix_modifiers: Vec<String>,
    pub is_virtual: bool,
    pub is_constexpr: bool,
    pub is_const: bool,
    pub is_no_except: bool,
    pub is_operator: bool,

    pub brief: Option<String>,
    pub body: Option<Vec<Arc<dyn Writable>>>,
}

impl From<CppMethodDecl> for CppMethodImpl {
    fn from(value: CppMethodDecl) -> Self {
        Self {
            body: value.body.unwrap_or_default(),
            brief: value.brief,
            cpp_method_name: value.cpp_name,
            declaring_cpp_full_name: "".into(),
            instance: value.instance,
            is_const: value.is_const,
            is_no_except: value.is_no_except,
            is_operator: value.is_operator,
            is_virtual: value.is_virtual,
            parameters: value.parameters,
            prefix_modifiers: value.prefix_modifiers,
            suffix_modifiers: value.suffix_modifiers,
            return_type: value.return_type,
            template: value.template,
        }
    }
}

// TODO: Generic
#[derive(Clone, Debug)]
pub struct CppMethodImpl {
    pub cpp_method_name: String,
    pub declaring_cpp_full_name: String,

    pub return_type: String,
    pub parameters: Vec<CppParam>,
    pub instance: bool,

    pub template: Option<CppTemplate>,
    pub is_const: bool,
    pub is_virtual: bool,
    pub is_no_except: bool,
    pub is_operator: bool,

    // TODO: Use bitflags to indicate these attributes
    // Holds unique of:
    // const
    // override
    // noexcept
    pub suffix_modifiers: Vec<String>,
    // Holds unique of:
    // constexpr
    // static
    // inline
    // explicit(...)
    // virtual
    pub prefix_modifiers: Vec<String>,

    pub brief: Option<String>,
    pub body: Vec<Arc<dyn Writable>>,
}

// TODO: Generics
#[derive(Clone, Debug)]
pub struct CppConstructorDecl {
    pub cpp_name: String,
    pub parameters: Vec<CppParam>,
    pub template: Option<CppTemplate>,

    pub is_constexpr: bool,
    pub is_explicit: bool,
    pub is_default: bool,
    pub is_no_except: bool,

    // call base ctor
    pub base_ctor: Option<(String, String)>,
    pub initialized_values: HashMap<String, String>,

    pub brief: Option<String>,
    pub body: Option<Vec<Arc<dyn Writable>>>,
}
#[derive(Clone, Debug)]
pub struct CppConstructorImpl {
    pub declaring_full_name: String,

    pub parameters: Vec<CppParam>,
    pub base_ctor: Option<(String, String)>,
    pub initialized_values: HashMap<String, String>,

    pub is_constexpr: bool,
    pub is_no_except: bool,

    pub template: Option<CppTemplate>,

    pub body: Vec<Arc<dyn Writable>>,
}

impl From<CppConstructorDecl> for CppConstructorImpl {
    fn from(value: CppConstructorDecl) -> Self {
        Self {
            body: value.body.unwrap_or_default(),
            declaring_full_name: value.cpp_name,
            is_constexpr: value.is_constexpr,
            base_ctor: value.base_ctor,
            initialized_values: value.initialized_values,
            is_no_except: value.is_no_except,
            parameters: value.parameters,
            template: value.template,
        }
    }
}

impl CppForwardDeclare {
    pub fn from_cpp_type(cpp_type: &CppType) -> Self {
        let ns = if !cpp_type.nested {
            Some(cpp_type.cpp_namespace().to_string())
        } else {
            None
        };

        Self {
            is_struct: cpp_type.is_value_type,
            cpp_namespace: ns,
            cpp_name: cpp_type.cpp_name().clone(),
            templates: cpp_type.cpp_template.clone(),
            literals: cpp_type.generic_instantiation_args.clone(),
        }
    }
}

impl CppParam {
    pub fn params_as_args(params: &[CppParam]) -> impl Iterator<Item = String> + '_ {
        params.iter().map(|p| match &p.def_value {
            Some(val) => format!("{}{} {} = {val}", p.ty, p.modifiers, p.name),
            None => format!("{}{} {}", p.ty, p.modifiers, p.name),
        })
    }
    pub fn params_as_args_no_default(params: &[CppParam]) -> impl Iterator<Item = String> + '_ {
        params
            .iter()
            .map(|p| format!("{}{} {}", p.ty, p.modifiers, p.name))
    }
    pub fn params_names(params: &[CppParam]) -> impl Iterator<Item = &String> {
        params.iter().map(|p| &p.name)
    }
    pub fn params_types(params: &[CppParam]) -> impl Iterator<Item = &String> {
        params.iter().map(|p| &p.ty)
    }

    pub fn params_il2cpp_types(params: &[CppParam]) -> impl Iterator<Item = String> + '_ {
        params
            .iter()
            .map(|p| format!("::il2cpp_utils::ExtractType({})", p.name))
    }
}

impl CppInclude {
    // smelly use of config but whatever
    pub fn new_context_typedef(context: &CppContext) -> Self {
        Self {
            include: diff_paths(&context.typedef_path, &STATIC_CONFIG.header_path).unwrap(),
            system: false,
        }
    }
    pub fn new_context_fundamental(context: &CppContext) -> Self {
        Self {
            include: diff_paths(&context.fundamental_path, &STATIC_CONFIG.header_path).unwrap(),
            system: false,
        }
    }

    pub fn new_system<P: AsRef<Path>>(str: P) -> Self {
        Self {
            include: str.as_ref().to_path_buf(),
            system: true,
        }
    }

    pub fn new_exact<P: AsRef<Path>>(str: P) -> Self {
        Self {
            include: str.as_ref().to_path_buf(),
            system: false,
        }
    }
}

impl CppUsingAlias {
    pub fn from_cpp_type(
        alias: String,
        cpp_type: &CppType,
        forwarded_generic_args_opt: Option<Vec<String>>,
        fixup_generic_args: bool,
    ) -> Self {
        let forwarded_generic_args = forwarded_generic_args_opt.unwrap_or_default();

        // splits literals and template
        let (literals_and_template, template) = match &cpp_type.cpp_template {
            Some(template) => {
                // Skip the first args as those aren't necessary
                let extra_template_args = template
                    .names
                    .iter()
                    .skip(forwarded_generic_args.len())
                    .cloned()
                    .collect_vec();

                let new_cpp_template = match !extra_template_args.is_empty() {
                    true => Some(CppTemplate { names: extra_template_args }),
                    false => None,
                };

                // Essentially, all nested types inherit their declaring type's generic params.
                // Append the rest of the template params as generic parameters
                match new_cpp_template {
                    Some(template) => (
                        forwarded_generic_args
                            .iter()
                            .chain(&template.names)
                            .cloned()
                            .collect_vec(),
                        Some(template),
                    ),
                    None => (forwarded_generic_args.clone(), None),
                }
            }
            None => (forwarded_generic_args.clone(), None),
        };

        let mut result = cpp_type.cpp_full_name.clone();

        // easy way to tell it's a generic instantiation
        if fixup_generic_args {
            result = format!("{result}<{}>", literals_and_template.join(", "))
        }

        Self {
            alias,
            result,
            template,
        }
    }
}
