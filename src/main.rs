#![feature(entry_insert)]
#![feature(let_chains)]
#![feature(core_intrinsics)]
#![feature(slice_as_chunks)]

use generate::config::GenerationConfig;
use generate::context::{CppContextCollection, TypeTag};
use generate::metadata::Metadata;

use std::path::PathBuf;
use std::{fs, time};

use clap::{Parser, Subcommand};
use il2cpp_binary::{Elf, TypeData};

use crate::generate::members::CppMember;
mod generate;

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Cli {
    /// The global-metadata.dat file to use
    #[clap(short, long, value_parser, value_name = "FILE")]
    metadata: PathBuf,

    /// The libil2cpp.so file to use
    #[clap(short, long, value_parser, value_name = "FILE")]
    libil2cpp: PathBuf,

    #[clap(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    // let cli = Cli::parse();
    let cli = Cli {
        metadata: PathBuf::from("global-metadata.dat"),
        libil2cpp: PathBuf::from("libil2cpp.so"),
        command: None,
    };

    let metadata_data = fs::read(cli.metadata)?;
    let il2cpp_metadata = il2cpp_metadata_raw::deserialize(&metadata_data)?;

    let elf_data = fs::read(cli.libil2cpp)?;
    let elf = Elf::parse(&elf_data)?;

    let (code_registration, metadata_registration) =
        il2cpp_binary::registrations(&elf, &il2cpp_metadata)?;

    let config = GenerationConfig {
        header_path: PathBuf::from("./codegen/include"),
        source_path: PathBuf::from("./codegen/src"),
    };

    let mut metadata = Metadata {
        metadata: &il2cpp_metadata,
        code_registration: &code_registration,
        metadata_registration: &metadata_registration,
        method_calculations: Default::default(),
    };
    let t = time::Instant::now();
    println!("Parsing metadata methods");
    metadata.parse();
    println!("Finished in {}ms", t.elapsed().as_millis());
    let mut cpp_context_collection = CppContextCollection::new();

    // First, make all the contexts
    for tdi in 0..metadata.metadata.type_definitions.len() {
        cpp_context_collection.fill(
            &metadata,
            &config,
            TypeData::TypeDefinitionIndex(tdi.try_into()?),
        );
    }
    // for t in &metadata.type_definitions {
    //     // Handle the generation for a single type
    //     let dest = open_writer(&metadata, &config, &t);
    //     write_type(&metadata, &config, &t, &dest);
    // }
    cpp_context_collection.get()[&TypeTag::TypeDefinition(123)].write()?;
    cpp_context_collection.get()[&TypeTag::TypeDefinition(342)].write()?;
    cpp_context_collection.get()[&TypeTag::TypeDefinition(512)].write()?;
    cpp_context_collection.get()[&TypeTag::TypeDefinition(1024)].write()?;
    cpp_context_collection.get()[&TypeTag::TypeDefinition(600)].write()?;
    cpp_context_collection.get()[&TypeTag::TypeDefinition(1000)].write()?;
    cpp_context_collection.get()[&TypeTag::TypeDefinition(420)].write()?;
    cpp_context_collection.get()[&TypeTag::TypeDefinition(69)].write()?;
    cpp_context_collection.get()[&TypeTag::TypeDefinition(531)].write()?;
    cpp_context_collection.get()[&TypeTag::TypeDefinition(532)].write()?;
    cpp_context_collection.get()[&TypeTag::TypeDefinition(533)].write()?;
    cpp_context_collection.get()[&TypeTag::TypeDefinition(534)].write()?;
    cpp_context_collection.get()[&TypeTag::TypeDefinition(535)].write()?;
    cpp_context_collection.get()[&TypeTag::TypeDefinition(1455)].write()?;
    println!("Generic type");
    cpp_context_collection
        .get()
        .iter()
        .find(|(_, c)| {
            c.get_types()
                .iter()
                .any(|(_, t)| !t.generic_args.names.is_empty())
        })
        .unwrap()
        .1
        .write()?;
    println!("Value type");
    cpp_context_collection
        .get()
        .iter()
        .find(|(_, c)| {
            c.get_types()
                .iter()
                .any(|(_, t)| t.is_value_type && t.name == "Color" && t.namespace == "UnityEngine")
        })
        .unwrap()
        .1
        .write()?;
    println!("AlignmentUnion type");
    cpp_context_collection
        .get()
        .iter()
        .find(|(_, c)| {
            c.get_types()
                .iter()
                .any(|(_, t)| t.is_value_type && t.name == "AlignmentUnion")
        })
        .unwrap()
        .1
        .write()?;
    println!("Array type");
    cpp_context_collection
        .get()
        .iter()
        .find(|(_, c)| {
            c.get_types()
                .iter()
                .any(|(_, t)| t.name == "Array" && t.namespace == "System")
        })
        .unwrap()
        .1
        .write()?;
    println!("Default param");
    cpp_context_collection
        .get()
        .iter()
        .filter(|(_, c)| {
            c.get_types().iter().any(|(_, t)| {
                t.declarations.iter().any(|d| {
                    if let CppMember::MethodDecl(m) = d {
                        m.parameters.iter().any(|p| p.def_value.is_some())
                    } else {
                        false
                    }
                })
            })
        })
        .nth(2)
        .unwrap()
        .1
        .write()?;
    // for (_, context) in cpp_context_collection.get() {
    //     context.write().unwrap();
    // }

    Ok(())
}
