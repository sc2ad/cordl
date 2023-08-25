#![feature(entry_insert)]
#![feature(let_chains)]
#![feature(core_intrinsics)]
#![feature(slice_as_chunks)]
#![feature(read_buf)]
#![feature(map_try_insert)]
#![feature(return_position_impl_trait_in_trait)]
#![feature(lazy_cell)]
#![feature(exit_status_error)]
#![feature(result_option_inspect)]

use brocolib::{global_metadata::TypeDefinitionIndex, runtime_metadata::TypeData};
use color_eyre::Result;
use generate::{config::GenerationConfig, metadata::Metadata};
use itertools::Itertools;
use walkdir::DirEntry;

use std::{
    fs,
    path::PathBuf,
    process::{Child, Command},
    sync::LazyLock,
    thread, time,
};

use clap::{Parser, Subcommand};

use crate::{
    generate::{
        context_collection::{CppContextCollection, CppTypeTag},
        cs_context_collection::CsContextCollection,
        members::CppMember,
    },
    handlers::{unity, value_type},
};
mod generate;
mod handlers;
mod helpers;

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Cli {
    /// The global-metadata.dat file to use
    #[clap(short, long, value_parser, value_name = "FILE")]
    metadata: PathBuf,

    /// The libil2cpp.so file to use
    #[clap(short, long, value_parser, value_name = "FILE")]
    libil2cpp: PathBuf,
    /// The libil2cpp.so file to use
    #[clap(short, long)]
    format: bool,

    #[clap(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {}

pub static STATIC_CONFIG: LazyLock<GenerationConfig> = LazyLock::new(|| GenerationConfig {
    header_path: PathBuf::from("./codegen/include"),
    source_path: PathBuf::from("./codegen/src"),
    src_internals_path: PathBuf::from("./cordl_internals"),
    dst_internals_path: PathBuf::from("./codegen/include/cordl_internals"),
    dst_header_internals_file: PathBuf::from("./codegen/include/cordl_internals/cordl_internals.hpp"),
});

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let cli: Cli = Cli::parse();
    if !cli.format {
        println!("Add --format/-f to format with clang-format at end")
    }
    // let cli = Cli {
    //     metadata: PathBuf::from("global-metadata.dat"),
    //     libil2cpp: PathBuf::from("libil2cpp.so"),
    //     command: None,
    // };

    println!(
        "Copying config to codegen folder {:?}",
        STATIC_CONFIG.dst_internals_path
    );

    std::fs::create_dir_all(&STATIC_CONFIG.dst_internals_path)?;

    // copy contents of cordl_internals folder into destination
    let mut options = fs_extra::dir::CopyOptions::new();
    options.content_only = true;

    fs_extra::dir::copy(
        &STATIC_CONFIG.src_internals_path,
        &STATIC_CONFIG.dst_internals_path,
        &options
    )?;

    let global_metadata_data = fs::read(cli.metadata)?;
    let elf_data = fs::read(cli.libil2cpp)?;
    let il2cpp_metadata = brocolib::Metadata::parse(&global_metadata_data, &elf_data)?;

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

    {
        // First, make all the contexts
        println!("Making types");
        let type_defs = metadata.metadata.global_metadata.type_definitions.as_vec();
        let total = type_defs.len();
        for tdi_u64 in 0..total {
            let tdi = TypeDefinitionIndex::new(tdi_u64 as u32);

            let ty = &metadata.metadata.global_metadata.type_definitions[tdi];

            if ty.declaring_type_index != u32::MAX {
                continue;
            }

            println!(
                "Making types {:.4}% ({tdi_u64}/{total})",
                (tdi_u64 as f64 / total as f64 * 100.0)
            );
            cpp_context_collection.make_from(
                &metadata,
                &STATIC_CONFIG,
                TypeData::TypeDefinitionIndex(tdi),
            );
            cpp_context_collection.alias_nested_types_il2cpp(
                tdi,
                CppTypeTag::TypeDefinitionIndex(tdi),
                &metadata,
                false,
            );
        }
    }
    {
        // First, make all the contexts
        println!("Making nested types");
        let type_defs = metadata.metadata.global_metadata.type_definitions.as_vec();
        let total = type_defs.len();
        for tdi_u64 in 0..total {
            let tdi = TypeDefinitionIndex::new(tdi_u64 as u32);

            let ty = &metadata.metadata.global_metadata.type_definitions[tdi];

            if ty.declaring_type_index == u32::MAX {
                continue;
            }

            println!(
                "Making nested types {:.4}% ({tdi_u64}/{total})",
                (tdi_u64 as f64 / total as f64 * 100.0)
            );
            cpp_context_collection.make_nested_from(&metadata, &STATIC_CONFIG, tdi);
        }
    }

    {
        let total = metadata.metadata_registration.generic_method_table.len() as f64;
        println!("Making generic type instantiations");
        for (i, generic_class) in metadata
            .metadata_registration
            .generic_method_table
            .iter()
            .enumerate()
        {
            println!(
                "Making generic type instantiations {:.4}% ({i}/{total})",
                (i as f64 / total * 100.0)
            );
            let method_spec = metadata
                .metadata_registration
                .method_specs
                .get(generic_class.generic_method_index as usize)
                .unwrap();

            cpp_context_collection.make_generic_from(method_spec, &mut metadata, &STATIC_CONFIG);
        }
    }
    {
        let total = metadata.metadata_registration.generic_method_table.len() as f64;
        println!("Filling generic types!");
        for (i, generic_class) in metadata
            .metadata_registration
            .generic_method_table
            .iter()
            .enumerate()
        {
            println!(
                "Filling generic type instantiations {:.4}% ({i}/{total})",
                (i as f64 / total * 100.0)
            );
            let method_spec = metadata
                .metadata_registration
                .method_specs
                .get(generic_class.generic_method_index as usize)
                .unwrap();

            cpp_context_collection.fill_generic_class_inst(
                method_spec,
                &mut metadata,
                &STATIC_CONFIG,
            );
        }
    }
    {
        let total = metadata.metadata_registration.generic_method_table.len() as f64;
        println!("Filling generic methods!");
        for (i, generic_class) in metadata
            .metadata_registration
            .generic_method_table
            .iter()
            .enumerate()
        {
            println!(
                "Filling generic method instantiations {:.4}% ({i}/{total})",
                (i as f64 / total * 100.0)
            );
            let method_spec = metadata
                .metadata_registration
                .method_specs
                .get(generic_class.generic_method_index as usize)
                .unwrap();

            cpp_context_collection.fill_generic_method_inst(
                method_spec,
                &mut metadata,
                &STATIC_CONFIG,
            );
        }
    }

    println!("Registering handlers!");
    unity::register_unity(&mut metadata)?;
    value_type::register_value_type(&mut metadata)?;
    println!("Handlers registered!");

    {
        // Fill them now
        println!("Filling types");
        let type_defs = metadata.metadata.global_metadata.type_definitions.as_vec();
        let total = type_defs.len();
        for tdi_u64 in 0..total {
            let tdi = TypeDefinitionIndex::new(tdi_u64 as u32);

            println!(
                "Filling type {:.4} ({tdi_u64}/{total})",
                (tdi_u64 as f64 / total as f64 * 100.0)
            );

            cpp_context_collection.fill(
                &metadata,
                &STATIC_CONFIG,
                CppTypeTag::TypeDefinitionIndex(tdi),
            );
        }
    }

    let write_all = false;
    if write_all {
        cpp_context_collection.write_all(&STATIC_CONFIG)?;
        cpp_context_collection.write_namespace_headers()?;
    } else {
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
            .find(|(_, c)| c.get_types().iter().any(|(_, t)| t.cpp_template.is_some()))
            .unwrap()
            .1
            .write(&STATIC_CONFIG)?;
        println!("List Generic type");
        cpp_context_collection
            .get()
            .iter()
            .find(|(_, c)| {
                c.get_types().iter().any(|(_, t)| {
                    t.generic_instantiation_args.is_some() && t.cpp_name() == "List_1"
                })
            })
            .unwrap()
            .1
            .write(&STATIC_CONFIG)?;
        println!("Value type");
        cpp_context_collection
            .get()
            .iter()
            .find(|(_, c)| {
                c.get_types().iter().any(|(_, t)| {
                    t.is_value_type && t.name == "Color" && t.namespace == "UnityEngine"
                })
            })
            .unwrap()
            .1
            .write(&STATIC_CONFIG)?;
        // println!("Nested type");
        // cpp_context_collection
        //     .get()
        //     .iter()
        //     .find(|(_, c)| {
        //         c.get_types().iter().any(|(_, t)| {
        //             t.nested_types
        //                 .iter()
        //                 .any(|(_, n)| !n.declarations.is_empty())
        //         })
        //     })
        //     .unwrap()
        //     .1
        //     .write()?;
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
            .write(&STATIC_CONFIG)?;
        println!("Default param");
        cpp_context_collection
            .get()
            .iter()
            .filter(|(_, c)| {
                c.get_types().iter().any(|(_, t)| {
                    t.implementations.iter().any(|d| {
                        if let CppMember::MethodImpl(m) = d.as_ref() {
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
            .write(&STATIC_CONFIG)?;
        println!("Enum type");
        cpp_context_collection
            .get()
            .iter()
            .find(|(_, c)| c.get_types().iter().any(|(_, t)| t.is_enum_type))
            .unwrap()
            .1
            .write(&STATIC_CONFIG)?;
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
            .write(&STATIC_CONFIG)?;
        println!("BeatmapSaveDataHelpers");
        cpp_context_collection
            .get()
            .iter()
            .find(|(_, c)| {
                c.get_types()
                    .iter()
                    .any(|(_, t)| t.name == "BeatmapSaveDataHelpers")
            })
            .unwrap()
            .1
            .write(&STATIC_CONFIG)?;
        println!("HMUI.ViewController");
        cpp_context_collection
            .get()
            .iter()
            .find(|(_, c)| {
                c.get_types()
                    .iter()
                    .any(|(_, t)| t.namespace == "HMUI" && t.name == "ViewController")
            })
            .unwrap()
            .1
            .write(&STATIC_CONFIG)?;
        println!("UnityEngine.Component");
        cpp_context_collection
            .get()
            .iter()
            .find(|(_, c)| {
                c.get_types()
                    .iter()
                    .any(|(_, t)| t.namespace == "UnityEngine" && t.name == "Component")
            })
            .unwrap()
            .1
            .write(&STATIC_CONFIG)?;
        println!("UnityEngine.GameObject");
        cpp_context_collection
            .get()
            .iter()
            .find(|(_, c)| {
                c.get_types()
                    .iter()
                    .any(|(_, t)| t.namespace == "UnityEngine" && t.name == "GameObject")
            })
            .unwrap()
            .1
            .write(&STATIC_CONFIG)?;
        println!("MainFlowCoordinator");
        cpp_context_collection
            .get()
            .iter()
            .find(|(_, c)| {
                c.get_types()
                    .iter()
                    .any(|(_, t)| t.namespace.is_empty() && t.name == "MainFlowCoordinator")
            })
            .unwrap()
            .1
            .write(&STATIC_CONFIG)?;
        println!("OVRPlugin");
        cpp_context_collection
            .get()
            .iter()
            .find(|(_, c)| {
                c.get_types()
                    .iter()
                    .any(|(_, t)| t.namespace.is_empty() && t.name == "OVRPlugin")
            })
            .unwrap()
            .1
            .write(&STATIC_CONFIG)?;
        println!("HMUI.IValueChanger");
        cpp_context_collection
            .get()
            .iter()
            .find(|(_, c)| {
                c.get_types()
                    .iter()
                    .any(|(_, t)| t.namespace == "HMUI" && t.name == "IValueChanger`1")
            })
            .unwrap()
            .1
            .write(&STATIC_CONFIG)?;
        println!("System.ValueType");
        cpp_context_collection
            .get()
            .iter()
            .find(|(_, c)| {
                c.get_types()
                    .iter()
                    .any(|(_, t)| t.namespace == "System" && t.name == "ValueType")
            })
            .unwrap()
            .1
            .write(&STATIC_CONFIG)?;
        println!("System.ValueTuple_2");
        cpp_context_collection
            .get()
            .iter()
            .find(|(_, c)| {
                c.get_types()
                    .iter()
                    .any(|(_, t)| t.namespace == "System" && t.name == "ValueTuple`2")
            })
            .unwrap()
            .1
            .write(&STATIC_CONFIG)?;
        println!("System.Decimal");
        cpp_context_collection
            .get()
            .iter()
            .find(|(_, c)| {
                c.get_types()
                    .iter()
                    .any(|(_, t)| t.namespace == "System" && t.name == "Decimal")
            })
            .unwrap()
            .1
            .write(&STATIC_CONFIG)?;
        println!("System.Enum");
        cpp_context_collection
            .get()
            .iter()
            .find(|(_, c)| {
                c.get_types()
                    .iter()
                    .any(|(_, t)| t.namespace == "System" && t.name == "Enum")
            })
            .unwrap()
            .1
            .write(&STATIC_CONFIG)?;
        println!("System.Multicast");
        cpp_context_collection
            .get()
            .iter()
            .find(|(_, c)| {
                c.get_types()
                    .iter()
                    .any(|(_, t)| t.namespace == "System" && t.name == "MulticastDelegate")
            })
            .unwrap()
            .1
            .write(&STATIC_CONFIG)?;
        println!("System.Delegate");
        cpp_context_collection
            .get()
            .iter()
            .find(|(_, c)| {
                c.get_types()
                    .iter()
                    .any(|(_, t)| t.namespace == "System" && t.name == "Delegate")
            })
            .unwrap()
            .1
            .write(&STATIC_CONFIG)?;
        println!("BeatmapSaveDataVersion3.BeatmapSaveData.EventBoxGroup`1");
        cpp_context_collection
            .get()
            .iter()
            .find(|(_, c)| {
                c.get_types()
                    .iter()
                    .any(|(_, t)| t.name.contains("EventBoxGroup`1"))
            })
            .unwrap()
            .1
            .write(&STATIC_CONFIG)?;
        // for (_, context) in cpp_context_collection.get() {
        //     context.write().unwrap();
        // }
    }

    if cli.format {
        format_files()?;
    }

    Ok(())
}

fn format_files() -> Result<()> {
    println!("Formatting!");

    use walkdir::WalkDir;

    let files: Vec<DirEntry> = WalkDir::new(&STATIC_CONFIG.header_path)
        .into_iter()
        .filter(|f| f.as_ref().is_ok_and(|f| f.path().is_file()))
        .try_collect()?;
    let file_count = files.len();

    let thread_count = thread::available_parallelism()?;
    let chunks = file_count / thread_count;

    println!("{chunks} per thread for {thread_count} threads");

    let file_chunks = files
        .into_iter()
        .sorted_by(|a, b| a.path().cmp(b.path()))
        // .unique_by(|f| f.path().to_str().unwrap().to_string())
        .chunks(chunks);

    let commands: Vec<Child> = file_chunks
        .into_iter()
        .map(|files| -> Result<Child> {
            let mut command = Command::new("clang-format");
            command.arg("--verbose").arg("-i");
            command.args(
                files
                    .into_iter()
                    .map(|f| f.into_path().into_os_string().into_string().unwrap()),
            );

            Ok(command.spawn()?)
        })
        .try_collect()?;

    commands.into_iter().try_for_each(|mut c| -> Result<()> {
        c.wait()?.exit_ok()?;
        Ok(())
    })?;

    println!("Done formatting!");
    Ok(())
}
