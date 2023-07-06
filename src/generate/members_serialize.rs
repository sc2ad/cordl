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

        if let Some(templates) = &self.templates {
            templates.write(writer)?;
        }

        let name = match &self.literals {
            Some(literals) => {
                format!("{}<{}>", self.name, literals.join(","))
            }
            None => self.name.clone(),
        };

        if self.literals.is_some() {
            // forward declare for instantiation
            writeln!(writer, "template<>")?;
        }

        writeln!(
            writer,
            "{} {name};",
            match self.is_struct {
                true => "struct",
                false => "class",
            }
        )?;

        if self.namespace.is_some() {
            writeln!(writer, "}}")?;
        }

        Ok(())
    }
}

impl Writable for CppCommentedString {
    fn write(&self, writer: &mut CppWriter) -> color_eyre::Result<()> {
        writeln!(writer, "{}", self.data)?;
        if let Some(val) = &self.comment {
            writeln!(writer, "// {val}")?;
        }
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

        if self.result_literals.is_empty() {
            writeln!(writer, "using {} = {};", self.alias, self.result)?;
        } else {
            writeln!(
                writer,
                "using {} = {}<{}>;",
                self.alias,
                self.result,
                self.result_literals.join(",")
            )?;
        }

        if let Some(namespaze) = &self.namespaze {
            writeln!(writer, "}} // {namespaze}")?;
        }
        Ok(())
    }
}

impl Writable for CppFieldDecl {
    fn write(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        fn write_field_comment(this: &CppFieldDecl, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
            writeln!(
                writer,
                "/// @brief Field: name: {}, Type Name: {}, {}",
                this.name,
                this.ty,
                match this.instance {
                    true => format!("Offset: 0x{:x}", this.offset),
                    false => "Static".to_string(),
                }
            )?;

            Ok(())
        }

        // default value for fields
        let name = &self.name;
        let ty = &self.ty;

        // writes out a literal value as field default value accessible in C++
        if let Some(literal) = &self.literal_value {
            // default value for a ::StringW is a ConstString
            if ty == "::StringW" {
                writeln!(writer, "static ConstString<0x{:x}> {name}_default{{{literal}}};", literal.len() - 3)?;
            } else {
                writeln!(writer, "constexpr static {} {name}_default{{{literal}}};", self.ty)?;
            };
        }

        self.write_field_getter(writer)?;
        if !self.readonly {
            self.write_field_putter(writer)?;
            write_field_comment(self, writer)?;
            writeln!(writer, "{}__declspec(property(get=__get_{name}, put=__put_{name})) {ty} {name};", match self.instance { true => "", false => "static " })?;
        } else {
            write_field_comment(self, writer)?;
            writeln!(writer, "{}__declspec(property(get=__get_{name})) {ty} {name};", match self.instance { true => "", false => "static " })?;
        }

        Ok(())
    }
}

impl Writable for CppFieldImpl {
    fn write(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        self.write_field_getter(writer)?;
        if !self.field.readonly {
            self.write_field_putter(writer)?;
        }

        Ok(())
    }
}

impl Writable for CppMethodDecl {
    // declaration
    fn write(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        writeln!(
            writer,
            "/// @brief Name: {}, Addr: 0x{:x}, Size: 0x{:x}",
            self.cpp_name,
            self.method_data
                .as_ref()
                .map(|t| t.addrs)
                .unwrap_or(u64::MAX),
            self.method_data
                .as_ref()
                .map(|t| t.estimated_size)
                .unwrap_or(usize::MAX)
        )?;

        self.parameters.iter().try_for_each(|param|
            match &param.def_value {
                Some(def) =>
                    writeln!(
                        writer,
                        "/// @param {}: {} (default: {})",
                        param.name,
                        param.ty,
                        def
                    ),
                None =>
                    writeln!(
                        writer,
                        "/// @param {}: {}",
                        param.name,
                        param.ty
                    ),
            }
        )?;

        if self.return_type != "void" {
            writeln!(
                writer,
                "/// @return {}",
                self.return_type
            )?;
        }

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
            CppParam::params_as_args(&self.parameters).join(", ")
        )?;

        Ok(())
    }
}

impl Writable for CppMethodImpl {
    // declaration
    fn write(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        self.template.write(writer)?;

        // Start
        writeln!(
            writer,
            "{} {}::{}({}){{",
            self.return_type,
            self.holder_cpp_name,
            self.cpp_method_name,
            CppParam::params_as_args_no_default(&self.parameters).join(", ")
        )?;

        //   static auto ___internal__logger = ::Logger::get().WithContext("::Org::BouncyCastle::Crypto::Parameters::DHPrivateKeyParameters::Equals");
        //   auto* ___internal__method = THROW_UNLESS((::il2cpp_utils::FindMethod(this, "Equals", std::vector<Il2CppClass*>{}, ::std::vector<const Il2CppType*>{::il2cpp_utils::ExtractType(obj)})));
        //   return ::il2cpp_utils::RunMethodRethrow<bool, false>(this, ___internal__method, obj);

        // Body

        let complete_type_name = format!("{}::{}", self.holder_cpp_namespaze, self.holder_cpp_name);
        let params_format = CppParam::params_types(&self.parameters).join(", ");

        let f_ptr_prefix = if self.instance {
            format!("{complete_type_name}::")
        } else {
            "".to_string()
        };

        writeln!(writer,
            "static auto ___internal_method = ::il2cpp_utils::il2cpp_type_check::MetadataGetter<static_cast<{} ({f_ptr_prefix}*)({params_format})>(&{complete_type_name}::{})>::methodInfo();",
            self.return_type,
            self.cpp_method_name)?;

        let instance_pointer = if self.instance { "this" } else { "nullptr" };

        let method_invoke_params = vec![instance_pointer, "___internal_method"];

        let param_names = CppParam::params_names(&self.parameters).map(|s| s.as_str());

        write!(
            writer,
            "return ::il2cpp_utils::RunMethodRethrow<{}, false>({}",
            self.return_type,
            method_invoke_params
                .into_iter()
                .chain(param_names)
                .join(", ")
        )?;
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
            CppParam::params_as_args(&self.parameters).join(", ")
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
                CppParam::params_as_args(&self.parameters).join(", ")
            )?;
        } else {
            write!(
                writer,
                "{}({})",
                self.holder_cpp_ty_name,
                CppParam::params_as_args_no_default(&self.parameters).join(", ")
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
            CppParam::params_names(&self.parameters).join(", ")
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
            "/// @brief Property: name: {}, Type Name: {}, getter: {}, setter: {}, abstract: {}",
            self.name,
            self.ty,
            self.getter.is_some(),
            self.setter.is_some(),
            self.abstr
        )?;

        // TODO:
        if self.abstr {
            writeln!(writer, "// TODO: ABSTRACT PROP HERE!")?;
            return Ok(());
        }

        // if this is an instance prop, don't put 'static ' in front
        if !self.instance {write!(writer, "static ")? }

        // TODO: verify self.name is correct for the get_, set_ methods
        // TODO: make the used prop name not start with _ if we can
        let name = &self.name;
        let ty = &self.ty;

        if self.getter.is_some() && self.setter.is_some() { // getter & setter
            writeln!(writer, "__declspec(property(get=get_{name}, put=set_{name})) {ty} {name};")?;
        } else if self.getter.is_some() { // only getter
            writeln!(writer, "__declspec(property(get=get_{name})) {ty} {name};")?;
        } else if self.setter.is_some() { // only setter
            writeln!(writer, "__declspec(property(put=set_{name})) {ty} {name};")?;
        } else { // property without get & set, mostly a safeguard to let us know something is wrong
            writeln!(writer, "// ERROR: INVALID PROP WITH GET & SET FALSE!")?;
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
        let params_format = CppParam::params_types(&self.params).join(", ");

        let method_info_rhs = if let Some(slot) = self.slot && !self.is_final {
            format!("THROW_UNLESS(::il2cpp_utils::ResolveVtableSlot(classof({}), {}(), {slot}))",
                self.complete_type_name,
                self.interface_clazz_of
            )
        } else {
            format!("THROW_UNLESS(::il2cpp_utils::FindMethod(classof({}), \"{}\", std::vector<Il2CppClass*>{{}}, ::std::vector<const Il2CppType*>{{{}}}))",
                self.complete_type_name,
                self.cpp_method_name,
                CppParam::params_types(&self.params).map(|t| format!("&classof({t})->byval_arg")).join(", ")
            )
        };

        let f_ptr_prefix = if self.instance {
            format!("{}::", self.complete_type_name)
        } else {
            "".to_string()
        };

        writeln!(
            writer,
            "template<>
struct ::il2cpp_utils::il2cpp_type_check::MetadataGetter<static_cast<{} ({f_ptr_prefix}*)({params_format})>(&{}::{})> {{
  constexpr static const std::size_t size() {{
    return 0x{:x};
  }}
  constexpr static const std::size_t addrs() {{
    return 0x{:x};
  }}

  inline static const ::MethodInfo* methodInfo() {{
    return {method_info_rhs};
  }}
}};",
            self.ret_ty,
            self.complete_type_name,
            self.cpp_method_name,
            self.method_data.estimated_size,
            self.method_data.addrs
        )?;
        Ok(())
    }
}

impl Writable for CppStructSpecialization {
    fn write(&self, writer: &mut CppWriter) -> color_eyre::Result<()> {
        if let Some(namespace) = &self.namespace {
            writeln!(writer, "namespace {} {{", namespace)?;
        }

        self.template.write(writer)?;
        let class_specifier = if self.is_struct { "struct" } else { "class" };
        writeln!(writer, "{class_specifier} {};", self.name)?;

        if self.namespace.is_some() {
            writeln!(writer, "}} // namespace end")?;
        }

        Ok(())
    }
}

impl Writable for CppMember {
    fn write(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        match self {
            CppMember::FieldDecl(f) => f.write(writer),
            CppMember::FieldImpl(fi) => fi.write(writer),
            CppMember::MethodDecl(m) => m.write(writer),
            CppMember::Property(p) => p.write(writer),
            CppMember::Comment(c) => c.write(writer),
            CppMember::MethodImpl(i) => i.write(writer),
            CppMember::ConstructorDecl(c) => c.write(writer),
            CppMember::ConstructorImpl(ci) => ci.write(writer),
            CppMember::CppUsingAlias(alias) => alias.write(writer),
        }
    }
}
