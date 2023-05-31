#![feature(entry_insert)]
#![feature(let_chains)]
#![feature(core_intrinsics)]
#![feature(slice_as_chunks)]

use brocolib::{global_metadata::TypeDefinitionIndex, runtime_metadata::TypeData};
use generate::{config::GenerationConfig, context::CppContextCollection, metadata::Metadata};

use std::{fs, path::PathBuf, time};

use clap::{Parser, Subcommand};

use crate::{generate::members::CppMember, handlers::unity};
mod generate;
mod handlers;

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
    let cli = Cli::parse();
    // let cli = Cli {
    //     metadata: PathBuf::from("global-metadata.dat"),
    //     libil2cpp: PathBuf::from("libil2cpp.so"),
    //     command: None,
    // };

    let global_metadata_data = fs::read(cli.metadata)?;
    let elf_data = fs::read(cli.libil2cpp)?;
    let il2cpp_metadata = brocolib::Metadata::parse(&global_metadata_data, &elf_data)?;

    let config = GenerationConfig {
        header_path: PathBuf::from("./codegen/include"),
        source_path: PathBuf::from("./codegen/src"),
    };

    let mut metadata = Metadata {
        metadata: &il2cpp_metadata,
        code_registration: &il2cpp_metadata.runtime_metadata.code_registration,
        metadata_registration: &il2cpp_metadata.runtime_metadata.metadata_registration,
        method_calculations: Default::default(),
        parent_to_child_map: Default::default(),
        child_to_parent_map: Default::default(),
        custom_type_handler: Default::default(),
        name_to_tdi: Default::default(),
        blacklisted_types: Default::default(),
    };
    let t = time::Instant::now();
    println!("Parsing metadata methods");
    metadata.parse();
    println!("Finished in {}ms", t.elapsed().as_millis());
    let mut cpp_context_collection = CppContextCollection::new();

    // First, make all the contexts
    println!("Making types");
    for tdi_u64 in 0..metadata
        .metadata
        .global_metadata
        .type_definitions
        .as_vec()
        .len()
    {
        let tdi = TypeDefinitionIndex::new(tdi_u64 as u32);
        // Skip children, creating the parents creates them too
        if metadata.child_to_parent_map.contains_key(&tdi) {
            continue;
        }
        cpp_context_collection.make_from(&metadata, &config, TypeData::TypeDefinitionIndex(tdi));
    }

    println!("Registering handlers!");
    unity::register_unity(&cpp_context_collection, &mut metadata)?;
    println!("Handlers registered!");

    // Fill them now
    println!("Filling root types");
    for tdi_u64 in 0..metadata
        .metadata
        .global_metadata
        .type_definitions
        .as_vec()
        .len()
    {
        let tdi = TypeDefinitionIndex::new(tdi_u64 as u32);

        if metadata.child_to_parent_map.contains_key(&tdi) {
            continue;
        }
        cpp_context_collection.fill(&metadata, &config, TypeData::TypeDefinitionIndex(tdi));
    }
    // Fill children
    println!("Nested types pass");
    for parent in metadata.parent_to_child_map.keys() {
        let owner = cpp_context_collection
            .get_cpp_type(TypeData::TypeDefinitionIndex(*parent))
            .unwrap();

        // **Ignore this, we no longer recurse:**
        // skip children of children
        // only fill first grade children of types
        // if owner.nested {
        //     continue;
        // }

        let owner_ty = owner.self_tag;

        cpp_context_collection.fill_nested_types(&metadata, &config, owner_ty);
    }

    // for t in &metadata.type_definitions {
    //     // Handle the generation for a single type
    //     let dest = open_writer(&metadata, &config, &t);
    //     write_type(&metadata, &config, &t, &dest);
    // }
    fn make_td_tdi(idx: u32) -> TypeData {
        TypeData::TypeDefinitionIndex(TypeDefinitionIndex::new(idx))
    }
    // All indices require updating
    // cpp_context_collection.get()[&make_td_tdi(123)].write()?;
    // cpp_context_collection.get()[&make_td_tdi(342)].write()?;
    // cpp_context_collection.get()[&make_td_tdi(512)].write()?;
    // cpp_context_collection.get()[&make_td_tdi(1024)].write()?;
    // cpp_context_collection.get()[&make_td_tdi(600)].write()?;
    // cpp_context_collection.get()[&make_td_tdi(1000)].write()?;
    // cpp_context_collection.get()[&make_td_tdi(420)].write()?;
    // cpp_context_collection.get()[&make_td_tdi(69)].write()?;
    // cpp_context_collection.get()[&make_td_tdi(531)].write()?;
    // cpp_context_collection.get()[&make_td_tdi(532)].write()?;
    // cpp_context_collection.get()[&make_td_tdi(533)].write()?;
    // cpp_context_collection.get()[&make_td_tdi(534)].write()?;
    // cpp_context_collection.get()[&make_td_tdi(535)].write()?;
    // cpp_context_collection.get()[&make_td_tdi(1455)].write()?;
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
    println!("Nested type");
    cpp_context_collection
        .get()
        .iter()
        .find(|(_, c)| {
            c.get_types()
                .iter()
                .any(|(_, t)| t.nested_types.iter().any(|n| !n.declarations.is_empty()))
        })
        .unwrap()
        .1
        .write()?;
    // Doesn't exist anymore?
    // println!("AlignmentUnion type");
    // cpp_context_collection
    //     .get()
    //     .iter()
    //     .find(|(_, c)| {
    //         c.get_types()
    //             .iter()
    //             .any(|(_, t)| t.is_value_type && &t.name == "AlignmentUnion")
    //     })
    //     .unwrap()
    //     .1
    //     .write()?;
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
    println!("UnityEngine.Object");
    cpp_context_collection
        .get()
        .iter()
        .find(|(_, c)| {
            c.get_types()
                .iter()
                .any(|(_, t)| t.name == "Object" && t.namespace == "UnityEngine")
        })
        .unwrap()
        .1
        .write()?;
    // for (_, context) in cpp_context_collection.get() {
    //     context.write().unwrap();
    // }

    Ok(())
}
