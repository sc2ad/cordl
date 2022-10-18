use super::{context::CppCommentedString, writer::Writable};

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub enum CppMember {
    Field(CppField),
    Method(CppMethod),
    Property(CppProperty),
    Comment(CppCommentedString),
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct CppField {
    pub name: String,
    pub ty: String,
    pub offset: u32,
    pub instance: bool,
    pub readonly: bool,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct CppMethod {
    pub name: String,
    pub return_type: String,
    pub parameters: Vec<String>, // todo: CPP Parameters
    pub(crate) instance: bool,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct CppProperty {
    pub name: String,
    pub ty: String,
    pub setter: bool,
    pub getter: bool,
}

impl CppField {
    pub fn make() -> CppField{
        CppField { name: todo!(), ty: todo!(), offset: todo!(), instance: todo!(), readonly: todo!() }
    }
}

impl CppMethod {
    pub fn make() -> CppMethod {
        CppMethod {
            name: todo!(),
            return_type: todo!(),
            parameters: todo!(),
            instance: todo!(),
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
        }
    }
}

// Writing

impl Writable for CppField {
    fn write(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        CppCommentedString {
                    data: format!(
                        "{} {} {};",
                        if !self.instance {
                            "static inline"
                        } else {
                            ""
                        },
                        self.ty, self.name,
                    ), // TODO
                    comment: Some(format!(
                        "Field: name: {}, Type Name: {}, Offset: 0x{:x}"
                    , self.name, self.ty, self.offset)),
                }.write(writer)?;
        Ok(())
    }
}
impl Writable for CppMethod {
    fn write(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        CppCommentedString {
            data: "".to_string(), // TODO
            comment: Some(format!(
                "Method: name: {}, Return Type Name: {} Parameters: {:?}",
                self.name, self.return_type, self.parameters
            )),
        }
        .write(writer)?;
        Ok(())
    }
}
impl Writable for CppProperty {
    fn write(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        Ok(())
    }
}

impl Writable for CppMember {
    fn write(&self, writer: &mut super::writer::CppWriter) -> color_eyre::Result<()> {
        match self {
            CppMember::Field(f) => f.write(writer),
            CppMember::Method(m) => m.write(writer),
            CppMember::Property(p) => p.write(writer),
            CppMember::Comment(c) => c.write(writer),
        }
    }
}
