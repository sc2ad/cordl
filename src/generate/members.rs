use itertools::Itertools;

use super::{
    context::CppContext,
    cpp_type::CppType,
    writer::{CppWriter, Writable},
};
use std::{io::Write, path::PathBuf};

#[derive(Debug, Eq, Hash, PartialEq, Clone, Default)]
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
            is_struct: cpp_type.is_struct,
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
    Constructor(CppConstructor),
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
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct CppMethodDecl {
    pub cpp_name: String,
    pub return_type: String,
    pub parameters: Vec<CppParam>,
    pub instance: bool,
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

#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct CppConstructor {
    pub ty: String,
    pub parameters: Vec<CppParam>,
    // TODO: Add all descriptions missing for the method
    pub method_data: CppMethodData,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct CppMethodImpl {
    pub cpp_name: String,
    pub name: String,
    pub holder_namespaze: String,
    pub holder_name: String,
    pub return_type: String,
    pub parameters: Vec<CppParam>,
    pub instance: bool,
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

impl CppField {
    pub fn make() -> CppField {
        CppField {
            name: todo!(),
            ty: todo!(),
            offset: todo!(),
            instance: todo!(),
            readonly: todo!(),
            classof_call: todo!(),
        }
    }
}

impl CppMethodDecl {
    pub fn make() -> CppMethodDecl {
        CppMethodDecl {
            cpp_name: todo!(),
            return_type: todo!(),
            parameters: todo!(),
            instance: todo!(),
            suffix_modifiers: todo!(),
            prefix_modifiers: todo!(),
            method_data: todo!(),
            is_virtual: todo!(),
        }
    }
}
impl CppMethodImpl {
    pub fn make() -> CppMethodImpl {
        CppMethodImpl {
            cpp_name: todo!(),
            return_type: todo!(),
            parameters: todo!(),
            instance: todo!(),
            suffix_modifiers: todo!(),
            prefix_modifiers: todo!(),
            name: todo!(),
            holder_namespaze: todo!(),
            holder_name: todo!(),
        }
    }
}

impl CppProperty {
    pub fn make() -> CppProperty {
        CppProperty {
            name: todo!(),
            ty: todo!(),
            setter: todo!(),
            getter: todo!(),
            abstr: todo!(),
            instance: todo!(),
            classof_call: todo!(),
        }
    }
}

// Writing

impl Writable for CppField {
    fn write(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        writeln!(
            writer,
            "// Field: name: {}, Type Name: {}, Offset: 0x{:x}",
            self.name, self.ty, self.offset
        )?;

        if self.instance {
            writeln!(
                writer,
                "::bs_hook::InstanceField<{}, 0x{:x},{}> {};",
                self.ty, self.offset, !self.readonly, self.name
            )?;
        } else {
            writeln!(
                writer,
                "static inline ::bs_hook::StaticField<{},\"{}\",{},{}> {};",
                self.ty, self.name, !self.readonly, self.classof_call, self.name
            )?;
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
            self.parameters
                .iter()
                .map(|p| format!("{}{} {}", p.ty, p.modifiers, p.name))
                .join(", ")
        )?;

        Ok(())
    }
}
impl Writable for CppConstructor {
    // declaration
    fn write(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        writeln!(
            writer,
            "// Ctor Parameters: {:?} Addr {:x} Size {:x}",
            self.parameters, self.method_data.addrs, self.method_data.estimated_size
        )?;

        writeln!(writer, "template<::il2cpp_utils::CreationType creationType = ::il2cpp_utils::CreationType::Temporary>")?;

        writeln!(
            writer,
            "inline {}({}) {{",
            self.ty,
            self.parameters
                .iter()
                .map(|p| format!("{}{} {}", p.ty, p.modifiers, p.name))
                .join(", ")
        )?;

        // TODO: Call base constructor and allocate
        writeln!(
            writer,
            "this->_ctor({});",
            self.parameters.iter().map(|p| &p.name).join(", ")
        )?;

        writeln!(writer, "}}")?;

        Ok(())
    }
}
impl Writable for CppMethodImpl {
    // declaration
    fn write(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        if !self.instance {
            write!(writer, "static ")?;
        }

        // Start
        writeln!(
            writer,
            "{} {}::{}::{}({}){{",
            self.return_type,
            self.holder_namespaze,
            self.holder_name,
            self.cpp_name,
            self.parameters
                .iter()
                .map(|p| format!("{}{} {}", p.ty, p.modifiers, p.name))
                .join(",")
        )?;

        //   static auto ___internal__logger = ::Logger::get().WithContext("::Org::BouncyCastle::Crypto::Parameters::DHPrivateKeyParameters::Equals");
        //   auto* ___internal__method = THROW_UNLESS((::il2cpp_utils::FindMethod(this, "Equals", std::vector<Il2CppClass*>{}, ::std::vector<const Il2CppType*>{::il2cpp_utils::ExtractType(obj)})));
        //   return ::il2cpp_utils::RunMethodRethrow<bool, false>(this, ___internal__method, obj);

        // Body
        let param_names = self
            .parameters
            .iter()
            .map(|p| format!("::il2cpp_utils::ExtractType({})", p.name))
            .join(", ");

        writeln!(writer, "static auto ___internal__method = THROW_UNLESS(::il2cpp_utils::FindMethod(this, \"{}\", std::vector<Il2CppClass*>{{}}, ::std::vector<const Il2CppType*>{{{}}}));", 
            self.name,
            &param_names
        )?;

        write!(
            writer,
            "return ::il2cpp_utils::RunMethodRethrow<{}, false>(this, ___internal__method",
            self.return_type
        )?;

        if !param_names.is_empty() {
            write!(writer, ", {}", param_names)?;
        }

        writeln!(writer, ");")?;

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
                self.name,
                self.ty,
                self.getter.is_some(),
                self.setter.is_some(),
                self.name
            )?;
        } else {
            writeln!(
                writer,
                "static inline ::bs_hook::StaticProperty<{},\"{}\",{},{}, {}> {};",
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
        let params_format = self
            .params
            .iter()
            .map(|p| format!("{} {}", p.ty, p.name))
            .collect::<Vec<_>>()
            .join(",");
        writeln!(
            writer,
            "template<>
struct ::il2cpp_utils::il2cpp_type_check::MetadataGetter<static_cast<void ({}::*)({})>(&{}::{})> {{
  constexpr static const usize size() {{
    return 0x{:x};
  }}
  constexpr static const usize addrs() {{
    return 0x{:x};
  }}
}};",
            self.ty,
            params_format,
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
            CppMember::Constructor(c) => c.write(writer),
        }
    }
}
