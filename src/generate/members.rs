use itertools::Itertools;

use super::{
    context::CppContext,
    cpp_type::CppType,
    writer::{CppWriter, Writable},
};
use std::{io::Write, path::PathBuf};

#[derive(Debug, Eq, Hash, PartialEq, Clone, Default, PartialOrd, Ord)]
pub struct CppTemplate {
    pub names: Vec<String>,
}

impl Writable for CppTemplate {
    fn write(&self, writer: &mut CppWriter) -> color_eyre::Result<()> {
        if !self.names.is_empty() {
            writeln!(
                writer,
                "template<{}>",
                self.names
                    .iter()
                    .map(|s| format!("typename {}", s))
                    .collect::<Vec<_>>()
                    .join(",")
            )?;
        }

        Ok(())
    }
}

#[derive(Debug, Eq, Hash, PartialEq, Clone)]
pub struct CppForwardDeclare {
    // TODO: Make this group lots into a single namespace
    pub is_struct: bool,
    pub namespace: Option<String>,
    pub name: String,
    pub templates: CppTemplate, // names of template arguments, T, TArgs etc.
}

impl CppForwardDeclare {
    pub fn from_cpp_type(cpp_type: &CppType) -> Self {
        Self {
            is_struct: cpp_type.is_value_type,
            namespace: Some(cpp_type.cpp_namespace().to_string()),
            name: cpp_type.name().clone(),
            templates: cpp_type.generic_args.clone(),
        }
    }
}

impl Writable for CppForwardDeclare {
    fn write(&self, writer: &mut CppWriter) -> color_eyre::Result<()> {
        if let Some(namespace) = &self.namespace {
            writeln!(writer, "namespace {} {{", namespace)?;
        }

        self.templates.write(writer)?;

        writeln!(
            writer,
            "{} {};",
            match self.is_struct {
                true => "struct",
                false => "class",
            },
            self.name
        )?;

        if self.namespace.is_some() {
            writeln!(writer, "}}")?;
        }

        Ok(())
    }
}

#[derive(Debug, Eq, Hash, PartialEq, Clone)]
pub struct CppCommentedString {
    pub data: String,
    pub comment: Option<String>,
}

impl Writable for CppCommentedString {
    fn write(&self, writer: &mut CppWriter) -> color_eyre::Result<()> {
        if let Some(val) = &self.comment {
            writeln!(writer, "// {val}")?;
        }
        writeln!(writer, "{}", self.data)?;
        Ok(())
    }
}

#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub struct CppInclude {
    include: PathBuf,
    system: bool,
}

impl CppInclude {
    pub fn new_context(context: &CppContext) -> Self {
        Self {
            include: context.fundamental_path.clone(),
            system: false,
        }
    }

    pub fn new_system(str: PathBuf) -> Self {
        Self {
            include: str,
            system: true,
        }
    }

    pub fn new(str: PathBuf) -> Self {
        Self {
            include: str,
            system: false,
        }
    }
}

impl Writable for CppInclude {
    fn write(&self, writer: &mut CppWriter) -> color_eyre::Result<()> {
        if self.system {
            writeln!(writer, "#include <{}>", self.include.to_str().unwrap())?;
        } else {
            writeln!(writer, "#include \"{}\"", self.include.to_str().unwrap())?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum CppMember {
    Field(CppField),
    MethodDecl(CppMethodDecl),
    MethodImpl(CppMethodImpl),
    Property(CppProperty),
    Comment(CppCommentedString),
    MethodSizeStruct(CppMethodSizeStruct), // TODO: Or a nested type
    ConstructorDecl(CppConstructorDecl),
    ConstructorImpl(CppConstructorImpl),
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct CppMethodData {
    pub estimated_size: usize,
    pub addrs: u64,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct CppMethodSizeStruct {
    pub cpp_name: String,
    pub ty: String,
    pub ret_ty: String,
    pub instance: bool,
    pub params: Vec<CppParam>,
    pub method_data: CppMethodData,
}
#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct CppField {
    pub name: String,
    pub ty: String,
    pub offset: u32,
    pub instance: bool,
    pub readonly: bool,
    pub classof_call: String,
    pub literal_value: Option<String>,
    pub use_wrapper: bool,
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
#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct CppMethodDecl {
    pub cpp_name: String,
    pub return_type: String,
    pub parameters: Vec<CppParam>,
    pub instance: bool,
    pub template: CppTemplate,
    // TODO: Use bitflags to indicate these attributes
    // Holds unique of:
    // const
    // override
    // noexcept
    pub suffix_modifiers: String,
    // Holds unique of:
    // constexpr
    // static
    // inline
    // explicit(...)
    // virtual
    pub prefix_modifiers: String,
    // TODO: Add all descriptions missing for the method
    pub method_data: CppMethodData,
    pub is_virtual: bool,
}

// TODO: Generic
#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct CppMethodImpl {
    pub cpp_name: String,
    pub name: String,
    pub holder_namespaze: String,
    pub holder_cpp_name: String,
    pub return_type: String,
    pub parameters: Vec<CppParam>,
    pub instance: bool,
    pub interface_clazz_of: String,
    pub is_final: bool,
    pub slot: Option<u16>,
    pub template: CppTemplate,
    // TODO: Use bitflags to indicate these attributes
    // Holds unique of:
    // const
    // override
    // noexcept
    pub suffix_modifiers: String,
    // Holds unique of:
    // constexpr
    // static
    // inline
    // explicit(...)
    // virtual
    pub prefix_modifiers: String,
}

// TODO: Generics
#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct CppConstructorDecl {
    pub ty: String,
    pub parameters: Vec<CppParam>,
    pub template: CppTemplate,
}
#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct CppConstructorImpl {
    pub holder_cpp_ty: String,

    pub parameters: Vec<CppParam>,
    pub is_constexpr: bool,
    pub template: CppTemplate,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct CppProperty {
    pub name: String,
    pub ty: String,
    pub setter: Option<CppMethodData>,
    pub getter: Option<CppMethodData>,
    pub abstr: bool,
    pub instance: bool,
    pub classof_call: String,
}
// Writing

impl CppParam {
    fn params_as_args(params: &[CppParam]) -> String {
        params
            .iter()
            .map(|p| match &p.def_value {
                Some(val) => format!("{}{} {} = {val}", p.ty, p.modifiers, p.name),
                None => format!("{}{} {}", p.ty, p.modifiers, p.name),
            })
            .join(", ")
    }
    fn params_as_args_no_default(params: &[CppParam]) -> String {
        params
            .iter()
            .map(|p| format!("{}{} {}", p.ty, p.modifiers, p.name))
            .join(", ")
    }
    fn params_names(params: &[CppParam]) -> String {
        params.iter().map(|p| &p.name).join(", ")
    }
    fn params_types(params: &[CppParam]) -> String {
        params.iter().map(|p| &p.ty).join(", ")
    }

    fn params_il2cpp_types(params: &[CppParam]) -> String {
        params
            .iter()
            .map(|p| format!("::il2cpp_utils::ExtractType({})", p.name))
            .join(", ")
    }
}

impl Writable for CppField {
    fn write(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        writeln!(
            writer,
            "// Field: name: {}, Type Name: {}, Offset: 0x{:x}",
            self.name, self.ty, self.offset
        )?;

        let cpp_name = if self.literal_value.is_some() {
            format!("_{}", &self.name)
        } else {
            self.name.to_string()
        };

        match self.use_wrapper {
            // no wrapper
            false => writeln!(
                writer,
                "{}{} {} = {}",
                if self.instance { "" } else { "inline static " },
                self.ty,
                self.name,
                self.literal_value.as_ref().unwrap_or(&"{}".to_string())
            )?,
            // wrapper
            true => {
                if let Some(literal) = &self.literal_value {
                    writeln!(writer, "constexpr {} {} = {literal};", self.ty, self.name)?;
                }
                if self.instance {
                    writeln!(
                        writer,
                        "::bs_hook::InstanceField<{}, 0x{:x},{}> {cpp_name};",
                        self.ty, self.offset, !self.readonly
                    )?;
                } else {
                    writeln!(
                        writer,
                        "static inline ::bs_hook::StaticField<{},\"{}\",{},&{}> {cpp_name};",
                        self.ty, self.name, !self.readonly, self.classof_call
                    )?;
                }
            }
        }

        Ok(())
    }
}
impl Writable for CppMethodDecl {
    // declaration
    fn write(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        writeln!(
            writer,
            "// Method: name: {}, Return Type Name: {} Parameters: {:?} Addr {:x} Size {:x}",
            self.cpp_name,
            self.return_type,
            self.parameters,
            self.method_data.addrs,
            self.method_data.estimated_size
        )?;

        self.template.write(writer)?;

        if !self.instance {
            write!(writer, "static ")?;
        } else if self.is_virtual {
            write!(writer, "virtual ")?;
        }
        writeln!(
            writer,
            "{} {}({});",
            self.return_type,
            self.cpp_name,
            CppParam::params_as_args(&self.parameters)
        )?;

        Ok(())
    }
}

impl Writable for CppMethodImpl {
    // declaration
    fn write(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        self.template.write(writer)?;
        
        if !self.instance {
            write!(writer, "static ")?;
        }

        // Start
        writeln!(
            writer,
            "{} {}::{}::{}({}){{",
            self.return_type,
            self.holder_namespaze,
            self.holder_cpp_name,
            self.cpp_name,
            CppParam::params_as_args_no_default(&self.parameters)
        )?;

        //   static auto ___internal__logger = ::Logger::get().WithContext("::Org::BouncyCastle::Crypto::Parameters::DHPrivateKeyParameters::Equals");
        //   auto* ___internal__method = THROW_UNLESS((::il2cpp_utils::FindMethod(this, "Equals", std::vector<Il2CppClass*>{}, ::std::vector<const Il2CppType*>{::il2cpp_utils::ExtractType(obj)})));
        //   return ::il2cpp_utils::RunMethodRethrow<bool, false>(this, ___internal__method, obj);

        // Body
        if let Some(slot) = self.slot && !self.is_final {
            writeln!(writer, "auto ___internal__method = THROW_UNLESS(::il2cpp_utils::ResolveVtableSlot((*reinterpret_cast<Il2CppObject**>(this))->klass, {}(), {slot}));", 
              self.interface_clazz_of
            )?
        } else {
            writeln!(writer, "static auto ___internal__method = THROW_UNLESS(::il2cpp_utils::FindMethod(this, \"{}\", std::vector<Il2CppClass*>{{}}, ::std::vector<const Il2CppType*>{{{}}}));", 
                self.name,
                CppParam::params_il2cpp_types(&self.parameters)
            )?
        }

        write!(
            writer,
            "return ::il2cpp_utils::RunMethodRethrow<{}, false>(this, ___internal__method",
            self.return_type
        )?;

        let param_names = CppParam::params_names(&self.parameters);

        if !param_names.is_empty() {
            write!(writer, ", {}", param_names)?;
        }

        writeln!(writer, ");")?;

        // End
        writeln!(writer, "}}")?;
        Ok(())
    }
}

impl Writable for CppConstructorDecl {
    // declaration
    fn write(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        writeln!(writer, "// Ctor Parameters {:?}", self.parameters)?;

        self.template.write(writer)?;
        writeln!(
            writer,
            "{}({});",
            self.ty,
            CppParam::params_as_args(&self.parameters)
        )?;
        Ok(())
    }
}
impl Writable for CppConstructorImpl {
    // declaration
    fn write(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        writeln!(writer, "// Ctor Parameters {:?}", self.parameters)?;

        // Constructor
        self.template.write(writer)?;

        if self.is_constexpr {
            // TODO:
            write!(
                writer,
                "inline {}({})",
                self.holder_cpp_ty,
                CppParam::params_as_args(&self.parameters)
            )?;
        } else {
            write!(
                writer,
                "{}({})",
                self.holder_cpp_ty,
                CppParam::params_as_args_no_default(&self.parameters)
            )?;
        }

        if self.is_constexpr {
            // Constexpr constructor
            writeln!(
                writer,
                " : {} {{",
                self.parameters
                    .iter()
                    .map(|p| format!("{}({})", &p.name, &p.name))
                    .collect_vec()
                    .join(",")
            )?;
        } else {
            // Call base constructor
            writeln!(
            writer,
            " : ::bs_hook::Il2CppWrapperType(::il2cpp_utils::New<Il2CppObject*>(classof({}), {})) {{",
            self.holder_cpp_ty,
            CppParam::params_names(&self.parameters)
        )?;
        }

        // End
        writeln!(writer, "}}")?;

        Ok(())
    }
}

impl Writable for CppProperty {
    fn write(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        writeln!(
            writer,
            "// Property: name: {}, Type Name: {}, setter {} getter {} abstract {}",
            self.name,
            self.ty,
            self.setter.is_some(),
            self.getter.is_some(),
            self.abstr
        )?;

        // TODO:
        if self.abstr {
            return Ok(());
        }

        if self.instance {
            writeln!(
                writer,
                "::bs_hook::InstanceProperty<{},\"{}\",{},{}> {};",
                self.ty,
                self.name,
                self.getter.is_some(),
                self.setter.is_some(),
                self.name
            )?;
        } else {
            writeln!(
                writer,
                "static inline ::bs_hook::StaticProperty<{},\"{}\",{},{}, &{}> {};",
                self.ty,
                self.name,
                self.getter.is_some(),
                self.setter.is_some(),
                self.classof_call,
                self.name
            )?;
        }

        Ok(())
    }
}

impl Writable for CppMethodSizeStruct {
    fn write(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        writeln!(
            writer,
            "//  Writing Method size for method: {}.{}",
            self.ty, self.cpp_name
        )?;
        let params_format = CppParam::params_types(&self.params);
        writeln!(
            writer,
            "template<>
struct ::il2cpp_utils::il2cpp_type_check::MetadataGetter<static_cast<{} ({}::*)({params_format})>(&{}::{})> {{
  constexpr static const usize size() {{
    return 0x{:x};
  }}
  constexpr static const usize addrs() {{
    return 0x{:x};
  }}
}};",
            self.ret_ty,
            self.ty,
            self.ty,
            self.cpp_name,
            self.method_data.estimated_size,
            self.method_data.addrs
        )?;
        Ok(())
    }
}

impl Writable for CppMember {
    fn write(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        match self {
            CppMember::Field(f) => f.write(writer),
            CppMember::MethodDecl(m) => m.write(writer),
            CppMember::Property(p) => p.write(writer),
            CppMember::Comment(c) => c.write(writer),
            CppMember::MethodSizeStruct(s) => s.write(writer),
            CppMember::MethodImpl(i) => i.write(writer),
            CppMember::ConstructorDecl(c) => c.write(writer),
            CppMember::ConstructorImpl(ci) => ci.write(writer),
        }
    }
}
