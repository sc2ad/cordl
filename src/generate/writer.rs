use std::{fs::File, io::Write};

pub struct CppWriter {
    pub stream: File,
    pub indent: u16,
    pub newline: bool,
}

impl CppWriter {
    pub fn indent(&mut self) {
        self.indent += 1;
    }
    pub fn dedent(&mut self) {
        if self.indent == 0 {
            panic!("Indent should not be dedented when already less than 4!")
        }
        self.indent -= 1;
    }
}

impl Write for CppWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // TODO: One day we will write indented
        // if self.indent > 0 && self.newline {
        //     self.stream.write_all("\t".repeat(self.indent.into()).as_bytes())?;
        // }
        self.newline = buf.ends_with(b"\n");
        self.stream.write(buf)

        // let buffer = str::repeat(" ", self.indent.into());
        // self.stream.write_all(buffer.as_bytes())?;
        // self.stream.write_all(buf)?;
        // return Ok(buf.len());
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.stream.flush()
    }
}

pub trait Writable: std::fmt::Debug {
    fn write(&self, writer: &mut CppWriter) -> color_eyre::Result<()>;
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub enum SortLevel {
    UsingAlias,
    UnwrappedEnum,
    Fields,
    Properties,
    Methods,
    Constructors,
    FieldsImpl,
    Unknown,
}

pub trait Sortable {
    fn sort_level(&self) -> SortLevel;
}

// impl PartialEq for dyn Sortable {
//     fn eq(&self, other: &Self) -> bool {
//         todo!()
//     }
// }
//  impl PartialOrd for dyn Sortable {
//     fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
//         self.sort_level().partial_cmp(&other.sort_level())
//     }
// }
