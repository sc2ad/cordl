#![feature(entry_insert)]
#![feature(let_chains)]

use generate::config::GenerationConfig;
use generate::context::{CppContextCollection, TypeTag};
use generate::metadata::Metadata;
use il2cpp_metadata_raw::Il2CppMethodDefinition;
use std::fs;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use il2cpp_binary::{Elf, TypeData};
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

    let metadata_data = fs::read(cli.metadata).unwrap();
    let il2cpp_metadata = il2cpp_metadata_raw::deserialize(&metadata_data).unwrap();

    let elf_data = fs::read(cli.libil2cpp).unwrap();
    let elf = Elf::parse(&elf_data).unwrap();

    let (code_registration, metadata_registration) =
        il2cpp_binary::registrations(&elf, &il2cpp_metadata).unwrap();

    let config = GenerationConfig {
        header_path: PathBuf::from("./codegen/include"),
        source_path: PathBuf::from("./codegen/src"),
    };

    let codegen_module = code_registration
        .code_gen_modules
        .iter()
        .find(|e| e.name == il2cpp_metadata.images)
        .unwrap();

    let mut methods_address_sorted: Vec<(&Il2CppMethodDefinition, u64)> = il2cpp_metadata
        .methods
        .iter()
        .map(|e| {
            (
                e,
                *codegen_module
                    .method_pointers
                    .get((e.token & 0xFFFFFF) as usize)
                    .unwrap_or(&0),
            )
        })
        .collect();
    methods_address_sorted.sort_by(|(_, a), (_, b)| a.cmp(b));

    let metadata = Metadata {
        metadata: &il2cpp_metadata,
        code_registration: &code_registration,
        metadata_registration: &metadata_registration,
        methods_address_sorted: methods_address_sorted
            .iter()
            .enumerate()
            .map(|(i, (_, m))| (i, *m))
            .collect(),
        methods_size_map: methods_address_sorted
            .iter()
            .enumerate()
            .map_while(|(i, a)| {
                methods_address_sorted
                    .get(i + 1)
                    .map(|b| (i, (b.1 - a.1) as usize))
            })
            .collect(),
    };
    let mut cpp_context_collection = CppContextCollection::new();

    // First, make all the contexts
    for tdi in 0..metadata.metadata.type_definitions.len() {
        cpp_context_collection.make_from(
            &metadata,
            &config,
            TypeData::TypeDefinitionIndex(tdi.try_into().unwrap()),
            true,
        );
    }
    // for t in &metadata.type_definitions {
    //     // Handle the generation for a single type
    //     let dest = open_writer(&metadata, &config, &t);
    //     write_type(&metadata, &config, &t, &dest);
    // }
    cpp_context_collection.get()[&TypeTag::TypeDefinition(123)]
        .write()
        .unwrap();
    cpp_context_collection.get()[&TypeTag::TypeDefinition(342)]
        .write()
        .unwrap();
    cpp_context_collection.get()[&TypeTag::TypeDefinition(512)]
        .write()
        .unwrap();
    cpp_context_collection.get()[&TypeTag::TypeDefinition(1024)]
        .write()
        .unwrap();
    cpp_context_collection.get()[&TypeTag::TypeDefinition(600)]
        .write()
        .unwrap();
    // for (_, context) in cpp_context_collection.get() {
    //     context.write().unwrap();
    // }

    Ok(())
}
