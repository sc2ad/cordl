#![feature(entry_insert)]

use std::fs;
use std::path::PathBuf;
use generate::config::GenerationConfig;
use generate::context::{CppContextCollection, TypeTag};
use generate::metadata::Metadata;

use il2cpp_binary::{Elf, TypeData};
use clap::{Parser, Subcommand};
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
enum Commands {
    
}

fn main() {
    // let cli = Cli::parse();
    let cli = Cli {
        metadata: PathBuf::from("global-metadata.dat"),
        libil2cpp: PathBuf::from("libil2cpp.so"),
        command: None,
    };

    let metadata_data = fs::read(cli.metadata).unwrap();
    let il2cpp_metadata = il2cpp_metadata_raw::deserialize(&metadata_data).unwrap();
    
    let elf_data = fs::read(cli.libil2cpp).unwrap();
    let elf = Elf::parse(&elf_data).unwrap();

    let (code_registration, metadata_registration) = il2cpp_binary::registrations(&elf, &il2cpp_metadata).unwrap();

    let config = GenerationConfig {
        header_path: PathBuf::from("include"),
        source_path: PathBuf::from("src")
    };

    let metadata = Metadata {
        metadata: &il2cpp_metadata,
        code_registration: &code_registration,
        metadata_registration: &metadata_registration
    };
    let mut cpp_context_collection = CppContextCollection::new();

    // First, make all the contexts
    for tdi in 0..metadata.metadata.type_definitions.len() {
        cpp_context_collection.make_from(&metadata, &config, TypeData::TypeDefinitionIndex(tdi.try_into().unwrap()), true);
    }
    // for t in &metadata.type_definitions {
    //     // Handle the generation for a single type
    //     let dest = open_writer(&metadata, &config, &t);
    //     write_type(&metadata, &config, &t, &dest);
    // }
    cpp_context_collection.get()[&TypeTag::TypeDefinition(123)].write().unwrap();
}