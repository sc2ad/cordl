use super::{
    members::*,
    writer::{CppWriter, Writable},
};

use itertools::Itertools;
use std::io::Write;

impl Writable for CppTemplate {
    fn write(&self, writer: &mut CppWriter) -> color_eyre::Result<()> {
        writeln!(
            writer,
            "template<{}>",
            self.names
                .iter()
                .map(|(constraint, t)| format!("{constraint} {t}"))
                .collect_vec()
                .join(",")
        )?;

        Ok(())
    }
}

impl Writable for CppForwardDeclare {
    fn write(&self, writer: &mut CppWriter) -> color_eyre::Result<()> {
        if let Some(namespace) = &self.cpp_namespace {
            writeln!(writer, "namespace {namespace} {{")?;
        }

        if let Some(templates) = &self.templates {
            templates.write(writer)?;
        }

        // don't write template twice
        if self.literals.is_some() && self.templates.is_none() {
            // forward declare for instantiation
            writeln!(writer, "template<>")?;
        }

        let name = match &self.literals {
            Some(literals) => {
                format!("{}<{}>", self.cpp_name, literals.join(","))
            }
            None => self.cpp_name.clone(),
        };

        writeln!(
            writer,
            "{} {name};",
            match self.is_struct {
                true => "struct",
                false => "class",
            }
        )?;

        if self.cpp_namespace.is_some() {
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
        let path = self.include.to_str().unwrap().replace("\\", "/");
        if self.system {
            writeln!(writer, "#include <{}>", path)?;
        } else {
            writeln!(writer, "#include \"{}\"", path)?;
        }
        Ok(())
    }
}
impl Writable for CppUsingAlias {
    fn write(&self, writer: &mut CppWriter) -> color_eyre::Result<()> {
        if let Some(template) = &self.template {
            template.write(writer)?;
        }

        // TODO: Figure out how to forward template
        if let Some(_template) = &self.template {
            writeln!(writer, "using {} = {};", self.alias, self.result)?;
        } else {
            writeln!(writer, "using {} = {};", self.alias, self.result)?;
        }

        Ok(())
    }
}

impl Writable for CppFieldDecl {
    fn write(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        if let Some(comment) = &self.brief_comment {
            writeln!(writer, "/// @brief {comment}")?;
        }

        let ty = &self.field_ty;
        let name = &self.cpp_name;
        let mut prefix_mods: Vec<&str> = vec![];
        let mut suffix_mods: Vec<&str> = vec![];

        if !self.instance {
            prefix_mods.push("static");
        }

        if self.const_expr {
            prefix_mods.push("constexpr");
        } else if self.readonly {
            suffix_mods.push("const");
        }

        let prefixes = prefix_mods.join(" ");
        let suffixes = suffix_mods.join(" ");

        if let Some(value) = &self.value {
            writeln!(writer, "{prefixes} {ty} {suffixes} {name}{{{value}}};")?;
        } else {
            writeln!(writer, "{prefixes} {ty} {suffixes} {name};")?;
        }

        Ok(())
    }
}

impl Writable for CppFieldImpl {
    fn write(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        if let Some(template) = &self.declaring_type_template {
            template.write(writer)?;
        }

        let ty = &self.field_ty;
        let name = &self.cpp_name;
        let declaring_ty = &self.declaring_type;
        let mut prefix_mods: Vec<&str> = vec![];
        let mut suffix_mods: Vec<&str> = vec![];

        if self.const_expr {
            prefix_mods.push("constexpr");
        } else if self.readonly {
            suffix_mods.push("const");
        }

        let prefixes = prefix_mods.join(" ");
        let suffixes = suffix_mods.join(" ");

        let value = &self.value;
        writeln!(
            writer,
            "{prefixes} {ty} {suffixes} {declaring_ty}::{name}{{{value}}};"
        )?;

        Ok(())
    }
}

impl Writable for CppMethodDecl {
    // declaration
    fn write(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        if let Some(brief) = &self.brief {
            writeln!(writer, "/// @brief {brief}")?;
        }

        // Param default comments
        self.parameters
            .iter()
            .filter(|t| t.def_value.is_some())
            .try_for_each(|param| {
                writeln!(
                    writer,
                    "/// @param {}: {} (default: {})",
                    param.name,
                    param.ty,
                    param.def_value.as_ref().unwrap()
                )
            })?;

        if let Some(template) = &self.template {
            template.write(writer)?;
        }

        let body = &self.body;

        let mut prefix_modifiers = self
            .prefix_modifiers
            .iter()
            .map(|s| s.as_str())
            .collect_vec();

        let mut suffix_modifiers = self
            .suffix_modifiers
            .iter()
            .map(|s| s.as_str())
            .collect_vec();

        if !self.instance {
            prefix_modifiers.push("static");
        }

        if self.is_constexpr {
            prefix_modifiers.push("constexpr")
        } else if self.is_inline {
            //implicitly inline
            prefix_modifiers.push("inline")
        }

        if self.is_virtual {
            prefix_modifiers.push("virtual");
        }
        if self.is_operator {
            prefix_modifiers.push("operator")
        }

        if self.is_const && self.instance {
            suffix_modifiers.push("const");
        }
        if self.is_no_except {
            suffix_modifiers.push("noexcept");
        }

        let suffixes = suffix_modifiers.join(" ");
        let prefixes = prefix_modifiers.join(" ");

        let ret = &self.return_type;
        let name = &self.cpp_name;
        let params = CppParam::params_as_args(&self.parameters).join(", ");

        match body {
            Some(body) => {
                writeln!(writer, "{prefixes} {ret} {name}({params}) {suffixes} {{")?;
                // Body
                body.iter().try_for_each(|w| w.write(writer))?;

                writeln!(writer, "}}")?;
            }
            None => {
                writeln!(writer, "{prefixes} {ret} {name}({params}) {suffixes};",)?;
            }
        }

        Ok(())
    }
}

impl Writable for CppMethodImpl {
    // declaration
    fn write(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        if let Some(brief) = &self.brief {
            writeln!(writer, "/// @brief {brief}")?;
        }

        // Param default comments
        self.parameters
            .iter()
            .filter(|t| t.def_value.is_some())
            .try_for_each(|param| {
                writeln!(
                    writer,
                    "/// @param {}: {} (default: {})",
                    param.name,
                    param.ty,
                    param.def_value.as_ref().unwrap()
                )
            })?;

        if let Some(declaring_type_template) = &self.declaring_type_template {
            declaring_type_template.write(writer)?;
        }
        if let Some(template) = &self.template {
            template.write(writer)?;
        }

        let mut prefix_modifiers = self
            .prefix_modifiers
            .iter()
            .map(|s| s.as_str())
            .collect_vec();

        let mut suffix_modifiers = self
            .suffix_modifiers
            .iter()
            .map(|s| s.as_str())
            .collect_vec();

        if self.is_constexpr {
            prefix_modifiers.push("constexpr");
        } else if self.is_inline {
            prefix_modifiers.push("inline");
        }

        if self.is_virtual {
            prefix_modifiers.push("virtual");
        }

        if self.is_const && self.instance {
            suffix_modifiers.push("const");
        }
        if self.is_no_except {
            suffix_modifiers.push("noexcept");
        }

        let suffixes = suffix_modifiers.join(" ");
        let prefixes = prefix_modifiers.join(" ");
        let ret = &self.return_type;
        let declaring_type = &self.declaring_cpp_full_name;
        let name = &self.cpp_method_name;
        let params = CppParam::params_as_args_no_default(&self.parameters).join(", ");

        writeln!(
            writer,
            "{prefixes} {ret} {declaring_type}::{}{name}({params}) {suffixes} {{",
            if self.is_operator { "operator " } else { "" }
        )?;

        // Body
        self.body.iter().try_for_each(|w| w.write(writer))?;

        // End
        writeln!(writer, "}}")?;
        Ok(())
    }
}

impl Writable for CppConstructorDecl {
    // declaration
    fn write(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        writeln!(writer, "// Ctor Parameters {:?}", self.parameters)?;

        if let Some(template) = &self.template {
            template.write(writer)?;
        }

        // Add empty body if initialize values or base ctor are defined
        let body = &self.body;

        let name = &self.cpp_name;
        let params = CppParam::params_as_args(&self.parameters).join(", ");

        let mut prefix_modifiers = vec![];
        let mut suffix_modifiers = vec![];

        if self.is_constexpr {
            prefix_modifiers.push("constexpr")
        } else if self.body.is_some() {
            //implicitly inline
            prefix_modifiers.push("inline")
        }

        if self.is_explicit {
            prefix_modifiers.push("explicit");
        }
        if self.is_no_except {
            suffix_modifiers.push("noexcept");
        }

        let prefixes = prefix_modifiers.join(" ");
        let suffixes = suffix_modifiers.join(" ");

        if let Some(body) = &body && !self.is_default {
            let initializers = match self.initialized_values.is_empty() && self.base_ctor.is_none()
            {
                true => "".to_string(),
                false => {
                    let mut initializers_list = self
                        .initialized_values
                        .iter()
                        .map(|(name, value)| format!("{}({})", name, value))
                        .collect_vec();

                    if let Some((base_ctor, args)) = &self.base_ctor {
                        initializers_list.insert(0, format!("{base_ctor}({args})"))
                    }

                    format!(": {}", initializers_list.join(","))
                }
            };

            writeln!(writer, "{prefixes} {name}({params}) {suffixes} {initializers} {{",)?;

            body.iter().try_for_each(|w| w.write(writer))?;
            writeln!(writer, "}}")?;
        } else {
            match self.is_default {
                true => writeln!(writer, "{prefixes} {name}({params}) {suffixes} = default;")?,
                false => writeln!(writer, "{prefixes} {name}({params}) {suffixes};")?,
            };
        }

        Ok(())
    }
}

impl Writable for CppConstructorImpl {
    // declaration
    fn write(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        writeln!(writer, "// Ctor Parameters {:?}", self.parameters)?;

        // Constructor
        if let Some(template) = &self.template {
            template.write(writer)?;
        }

        let initializers = match self.initialized_values.is_empty() && self.base_ctor.is_none() {
            true => "".to_string(),
            false => {
                let mut initializers_list = self
                    .initialized_values
                    .iter()
                    .map(|(name, value)| format!("{}({})", name, value))
                    .collect_vec();

                if let Some((base_ctor, args)) = &self.base_ctor {
                    initializers_list.insert(0, format!("{base_ctor}({args})"))
                }

                format!(": {}", initializers_list.join(","))
            }
        };

        let mut suffix_modifiers: Vec<&str> = vec![];
        if self.is_no_except {
            suffix_modifiers.push("noexcept")
        }

        let mut prefix_modifiers: Vec<&str> = vec![];
        if self.is_constexpr {
            prefix_modifiers.push("constexpr")
        }

        let full_name = &self.declaring_full_name;
        let declaring_name = &self.declaring_name;
        let params = CppParam::params_as_args_no_default(&self.parameters).join(", ");

        let prefixes = prefix_modifiers.join(" ");
        let suffixes = suffix_modifiers.join(" ");

        write!(
            writer,
            "{prefixes} {full_name}::{declaring_name}({params}) {suffixes} {initializers} {{",
        )?;

        self.body.iter().try_for_each(|w| w.write(writer))?;
        // End
        writeln!(writer, "}}")?;

        Ok(())
    }
}

impl Writable for CppPropertyDecl {
    fn write(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        let mut prefix_modifiers: Vec<&str> = vec![];
        let suffix_modifiers: Vec<&str> = vec![];

        let mut property_vec: Vec<String> = vec![];

        if let Some(getter) = &self.getter {
            property_vec.push(format!("get={getter}"));
        }
        if let Some(setter) = &self.setter {
            property_vec.push(format!("put={setter}"));
        }

        if !self.instance {
            prefix_modifiers.push("static");
        }

        let property = property_vec.join(", ");
        let ty = &self.prop_ty;
        let name = &self.cpp_name;

        let prefixes = prefix_modifiers.join(" ");
        let suffixes = suffix_modifiers.join(" ");

        if let Some(comment) = &self.brief_comment {
            writeln!(writer, "/// @brief {comment}")?;
        }

        writeln!(
            writer,
            "{prefixes} {ty} __declspec(property({property})) {suffixes} {name};"
        )?;

        Ok(())
    }
}

impl Writable for CppMethodSizeStruct {
    fn write(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        writeln!(
            writer,
            "//  Writing Method size for method: {}.{}",
            self.declaring_type_name, self.cpp_method_name
        )?;
        let template = self.template.clone().unwrap_or_default();

        let complete_type_name = &self.declaring_type_name;
        let cpp_method_name = &self.cpp_method_name;
        let ret_type = &self.ret_ty;
        let size = &self.method_data.estimated_size;
        let addr = &self.method_data.addrs;

        let interface_klass_of = &self.interface_clazz_of;

        let params_format = CppParam::params_types(&self.params).join(", ");

        let method_info_var = &self.method_info_var;

        let method_info_lines = if let Some(slot) = self.slot && !self.is_final {
            vec![
                format!("
                            static auto* {method_info_var} = THROW_UNLESS(::il2cpp_utils::ResolveVtableSlot(
                                classof({complete_type_name}),
                                 {interface_klass_of}(),
                                  {slot}
                                ));")
            ]
        } else {
            self.method_info_lines.clone()
        }.join("\n");

        let f_ptr_prefix = if self.instance {
            format!("{}::", self.declaring_type_name)
        } else {
            "".to_string()
        };

        template.write(writer)?;

        writeln!(
            writer,
            "
struct ::il2cpp_utils::il2cpp_type_check::MetadataGetter<static_cast<{ret_type} ({f_ptr_prefix}*)({params_format})>(&{complete_type_name}::{cpp_method_name})> {{
  constexpr static std::size_t size = 0x{size:x};
  constexpr static std::size_t addrs = 0x{addr:x};

  inline static const ::MethodInfo* methodInfo() {{
    {method_info_lines}
    return {method_info_var};
  }}
}};"
        )?;
        Ok(())
    }
}

impl Writable for CppStaticAssert {
    fn write(&self, writer: &mut CppWriter) -> color_eyre::Result<()> {
        let condition = &self.condition;
        match &self.message {
            None => writeln!(writer, "static_assert({condition})"),
            Some(message) => writeln!(writer, "static_assert({condition}, \"{message}\");"),
        }?;
        Ok(())
    }
}
impl Writable for CppLine {
    fn write(&self, writer: &mut CppWriter) -> color_eyre::Result<()> {
        writer.write_all(self.line.as_bytes())?;
        writeln!(writer)?; // add line ending
        Ok(())
    }
}

impl Writable for CppMember {
    fn write(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        match self {
            CppMember::FieldDecl(f) => f.write(writer),
            CppMember::FieldImpl(f) => f.write(writer),
            CppMember::MethodDecl(m) => m.write(writer),
            CppMember::Property(p) => p.write(writer),
            CppMember::Comment(c) => c.write(writer),
            CppMember::MethodImpl(i) => i.write(writer),
            CppMember::ConstructorDecl(c) => c.write(writer),
            CppMember::ConstructorImpl(ci) => ci.write(writer),
            CppMember::CppUsingAlias(alias) => alias.write(writer),
            CppMember::CppLine(line) => line.write(writer),
        }
    }
}
