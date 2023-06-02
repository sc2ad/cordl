use super::{
    members::*,
    writer::{CppWriter, Writable},
};
use itertools::Itertools;
use std::io::Write;

impl Writable for CppTemplate {
    fn write(&self, writer: &mut CppWriter) -> color_eyre::Result<()> {
        if !self.names.is_empty() {
            writeln!(
                writer,
                "template<{}>",
                self.names
                    .iter()
                    .map(|s| format!("typename {s}"))
                    .collect::<Vec<_>>()
                    .join(",")
            )?;
        }

        Ok(())
    }
}

impl Writable for CppForwardDeclare {
    fn write(&self, writer: &mut CppWriter) -> color_eyre::Result<()> {
        if let Some(namespace) = &self.namespace {
            writeln!(writer, "namespace {namespace} {{")?;
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

impl Writable for CppCommentedString {
    fn write(&self, writer: &mut CppWriter) -> color_eyre::Result<()> {
        if let Some(val) = &self.comment {
            writeln!(writer, "// {val}")?;
        }
        writeln!(writer, "{}", self.data)?;
        Ok(())
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
impl Writable for CppUsingAlias {
    fn write(&self, writer: &mut CppWriter) -> color_eyre::Result<()> {
        if let Some(template) = &self.template {
            template.write(writer)?;
        }

        if let Some(namespaze) = &self.namespaze {
            writeln!(writer, "namespace {namespaze} {{")?;
        }

        writeln!(writer, "using {} = {}", self.alias, self.result)?;

        if let Some(namespaze) = &self.namespaze {
            writeln!(writer, "}} // {namespaze}")?;
        }
        Ok(())
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
            // no wrapper/is C++ literal, like an int
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
                // literal il2cpp value
                if let Some(literal) = &self.literal_value {
                    writeln!(writer, "constexpr {} {} = {literal};", self.ty, self.name)?;
                }
                if self.instance {
                    writeln!(
                        writer,
                        "::bs_hook::InstanceField<{}, 0x{:x},{}> {cpp_name};",
                        self.ty, self.offset, self.readonly
                    )?;
                } else {
                    writeln!(
                        writer,
                        "static inline ::bs_hook::StaticField<{},\"{}\",&{},{}> {cpp_name};",
                        self.ty, self.name, self.classof_call, self.readonly
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
            "{} {}::{}({}){{",
            self.return_type,
            self.holder_cpp_name,
            self.cpp_method_name,
            CppParam::params_as_args_no_default(&self.parameters)
        )?;

        //   static auto ___internal__logger = ::Logger::get().WithContext("::Org::BouncyCastle::Crypto::Parameters::DHPrivateKeyParameters::Equals");
        //   auto* ___internal__method = THROW_UNLESS((::il2cpp_utils::FindMethod(this, "Equals", std::vector<Il2CppClass*>{}, ::std::vector<const Il2CppType*>{::il2cpp_utils::ExtractType(obj)})));
        //   return ::il2cpp_utils::RunMethodRethrow<bool, false>(this, ___internal__method, obj);

        // Body

        let complete_type_name = format!("{}::{}", self.holder_cpp_namespaze, self.holder_cpp_name);
        let params_format = CppParam::params_types(&self.parameters);

        writeln!(writer, "static auto ___internal_method = ::il2cpp_utils::il2cpp_type_check::MetadataGetter<static_cast<{} ({complete_type_name}::*)({params_format})>(&{complete_type_name}::{})>::methodInfo();",
            self.return_type,
            self.cpp_method_name)?;

        write!(
            writer,
            "return ::il2cpp_utils::RunMethodRethrow<{}, false>(this, ___internal__method,",
            self.return_type
        )?;

        let param_names = CppParam::params_names(&self.parameters);

        if !param_names.is_empty() {
            write!(writer, ", {param_names}")?;
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
                self.holder_cpp_ty_name,
                CppParam::params_as_args(&self.parameters)
            )?;
        } else {
            write!(
                writer,
                "{}({})",
                self.holder_cpp_ty_name,
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
            self.holder_cpp_ty_name,
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
                "::bs_hook::InstanceProperty<\"{}\",{},{},{}> {};",
                self.name,
                self.ty,
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
            self.complete_type_name, self.cpp_method_name
        )?;
        let params_format = CppParam::params_types(&self.params);

        let method_info_rhs = if let Some(slot) = self.slot && !self.is_final {
            format!("THROW_UNLESS(::il2cpp_utils::ResolveVtableSlot((*reinterpret_cast<Il2CppObject**>(this))->klass, {}(), {slot}))", 
              self.interface_clazz_of
            )
        } else {
            format!("THROW_UNLESS(::il2cpp_utils::FindMethod(this, \"{}\", std::vector<Il2CppClass*>{{}}, ::std::vector<const Il2CppType*>{{{params_format}}}))", 
                self.cpp_method_name
            )
        };

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

  inline static const ::MethodInfo* methodInfo() {{
    return {method_info_rhs};
  }}
}};",
            self.ret_ty,
            self.complete_type_name,
            self.complete_type_name,
            self.cpp_method_name,
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
            CppMember::MethodImpl(i) => i.write(writer),
            CppMember::ConstructorDecl(c) => c.write(writer),
            CppMember::ConstructorImpl(ci) => ci.write(writer),
        }
    }
}
