use crate::TypeDefinitionIndex;


use brocolib::global_metadata::Il2CppTypeDefinition;
use brocolib::runtime_metadata::Il2CppTypeDefinitionSizes;
use brocolib::runtime_metadata::TypeData;
use brocolib::runtime_metadata::{Il2CppType, Il2CppTypeEnum};
use log::debug;

use crate::generate::type_extensions::TypeExtentions;
use core::mem;

use super::metadata::PointerSize;

use super::metadata::Metadata;
use super::type_extensions::TypeDefinitionExtensions;

const IL2CPP_SIZEOF_STRUCT_WITH_NO_INSTANCE_FIELDS: u32 = 1;

pub fn layout_fields_for_type<'a>(
    ty_def: &'a Il2CppTypeDefinition,
    tdi: TypeDefinitionIndex,
    metadata: &'a Metadata,
    field_offsets: Option<&mut Vec<u32>>,
) -> SizeAndAlignment {
    let mut actual_size: usize = 0;
    let mut alignment: u8 = metadata.object_size();
    let mut natural_alignment: u8 = metadata.object_size();

    let mut instance_size: usize = if ty_def.parent_index == u32::MAX {
        // If our parent type doesn't exist, we should account for that by assuming object_size
        actual_size = metadata.object_size() as usize;
        metadata.object_size().into()
    } else {
        let parent_ty = &metadata.metadata_registration.types[ty_def.parent_index as usize];
        let parent_tdi: TypeDefinitionIndex = match parent_ty.data {
            TypeData::TypeDefinitionIndex(parent_tdi) => parent_tdi,

            TypeData::GenericClassIndex(generic_class) => {
                let generic_class = &metadata
                    .metadata
                    .runtime_metadata
                    .metadata_registration
                    .generic_classes[generic_class];
                let generic_ty = &metadata.metadata_registration.types[generic_class.type_index];
                let TypeData::TypeDefinitionIndex(parent_tdi) = generic_ty.data else {
                    panic!("Failed to find TypeDefinitionIndex for generic class: {:?}", generic_ty.data);
                };

                parent_tdi
            }
            _ => todo!("Not yet implemented: {:?}", parent_ty.data),
        };
        // TODO: We use the size of the type table here instead of performing a walk. Potentially risky if it does not exist?
        let res = layout_fields_for_type(
            &metadata.metadata.global_metadata.type_definitions[parent_tdi],
            parent_tdi,
            metadata,
            None,
        );
        actual_size = res.actual_size;
        alignment = if ty_def.is_value_type() {
            // Alignment of a value type is always 1
            1
        } else {
            // Otherwise grab it from the parent
            res.alignment
        };
        natural_alignment = res.natural_alignment;

        res.size
    };

    if ty_def.field_count > 0 {
        // TODO: Consider moving packing to an Il2CppTypeDefinition extension method
        let packing = u8::pow(
            ((ty_def.bitfield >> (metadata.packing_field_offset - 1)) & 0xF) as u8,
            2,
        );
        assert!(
            packing <= 128,
            "Packing must be valid! Actual: {:?}",
            packing
        );
        let mut field_offsets_option = field_offsets;
        // TODO: Try to layout our fields here.
        // The result will give us a bunch of fields with offsets and size/alignment info
        for f in ty_def.fields(metadata.metadata) {
            // First, make sure it's an instance field
            let ty = metadata
                .metadata
                .runtime_metadata
                .metadata_registration
                .types[f.type_index as usize];
            if ty.is_static() {
                // TODO: We only support non-static instance field offset computation!
                // This needs to match up with the instance fields as we write out our results
                continue;
            }
            let sa = get_type_size_and_alignment(&ty, metadata);
            // Il2cpp alignment logic from: FieldLayout.cpp: FieldLayout::LayoutFields
            let mut local_alignment = sa.alignment;
            if local_alignment < 4 && sa.natural_alignment != 0 {
                local_alignment = sa.natural_alignment;
            }
            if packing != 0 {
                local_alignment = std::cmp::min(sa.alignment, packing);
            }
            let mut offset = actual_size;

            offset += (local_alignment - 1) as usize;
            offset &= !(local_alignment - 1) as usize;

            // Add the field offsets here
            if let Some(offsets) = field_offsets_option.as_mut() {
                offsets.push(offset.try_into().unwrap());
            }
            actual_size = offset + std::cmp::max(sa.size, 1);
            alignment = std::cmp::max(alignment, local_alignment);
            natural_alignment = std::cmp::max(natural_alignment, sa.alignment);
        }
        // After we walk all of the fields, we need to align the class size
        // TODO: This assumes we are targeting an application compiled with clang
        instance_size = align_to(actual_size, alignment.into());
        if ty_def.is_value_type() && instance_size == metadata.object_size() as usize {
            instance_size = (IL2CPP_SIZEOF_STRUCT_WITH_NO_INSTANCE_FIELDS
                + metadata.object_size() as u32) as usize;
            actual_size = (IL2CPP_SIZEOF_STRUCT_WITH_NO_INSTANCE_FIELDS
                + metadata.object_size() as u32) as usize;
        }
    }
    instance_size = update_instance_size_for_generic_class(ty_def, tdi, instance_size, metadata);

    SizeAndAlignment {
        size: instance_size,
        actual_size,
        alignment,
        natural_alignment,
    }
}

fn update_instance_size_for_generic_class(
    ty_def: &Il2CppTypeDefinition,
    tdi: TypeDefinitionIndex,
    instance_size: usize,
    metadata: &Metadata<'_>,
) -> usize {
    // need to set this in case there are no fields in a generic instance type
    if !ty_def.generic_container_index.is_valid() {
        return instance_size;
    }
    let size = get_size_of_type_table(metadata, tdi)
        .map(|s| s.instance_size)
        .unwrap_or(0);

    // If the generic class has an instance size, it was explictly set
    if size > 0 && size as usize > instance_size {
        debug!(
            "Generic instance size overwrite! Old: {}, New: {}, for tdi: {:?}",
            instance_size, size, tdi
        );
        return size as usize;
    }

    instance_size
}

pub fn get_size_of_type_table<'a>(
    metadata: &'a Metadata<'a>,
    tdi: TypeDefinitionIndex,
) -> Option<&'a Il2CppTypeDefinitionSizes> {
    if let Some(size_table) = &metadata
        .metadata
        .runtime_metadata
        .metadata_registration
        .type_definition_sizes
    {
        size_table.get(tdi.index() as usize)
    } else {
        None
    }
}

enum OffsetType {
    Pointer,
    Int8,
    Int16,
    Int32,
    Int64,
    IntPtr,
    Float,
    Double,
}

/// Returns the alignment of a specified type, as expected in il2cpp.
/// This is done through inspecting alignments through il2cpp directly in clang.
/// Done via: offsetof({uint8_t pad, T t}, t);
fn get_alignment_of_type(ty: OffsetType, pointer_size: PointerSize) -> u8 {
    return match ty {
        OffsetType::Pointer => pointer_size as u8,
        OffsetType::Int8 => 1,
        OffsetType::Int16 => 2,
        OffsetType::Int32 => 4,
        OffsetType::Int64 => 8,
        OffsetType::IntPtr => pointer_size as u8,
        OffsetType::Float => 4,
        OffsetType::Double => 8,
    };
}

fn get_type_size_and_alignment(ty: &Il2CppType, metadata: &Metadata) -> SizeAndAlignment {
    let mut sa = SizeAndAlignment {
        alignment: 0,
        natural_alignment: 0,
        size: 0,
        actual_size: 0,
    };

    if ty.byref {
        sa.size = metadata.pointer_size as usize;
        sa.actual_size = metadata.pointer_size as usize;
        sa.alignment = get_alignment_of_type(OffsetType::Pointer, metadata.pointer_size);
        return sa;
    }

    match ty.ty {
        Il2CppTypeEnum::I1 | Il2CppTypeEnum::U1 | Il2CppTypeEnum::Boolean => {
            sa.size = mem::size_of::<i8>();
            sa.actual_size = mem::size_of::<i8>();
            sa.alignment = get_alignment_of_type(OffsetType::Int8, metadata.pointer_size);
        }
        Il2CppTypeEnum::I2 | Il2CppTypeEnum::U2 | Il2CppTypeEnum::Char => {
            sa.size = mem::size_of::<i16>();
            sa.actual_size = mem::size_of::<i16>();
            sa.alignment = get_alignment_of_type(OffsetType::Int16, metadata.pointer_size);
        }
        Il2CppTypeEnum::I4 | Il2CppTypeEnum::U4 => {
            sa.size = mem::size_of::<i32>();
            sa.actual_size = mem::size_of::<i32>();
            sa.alignment = get_alignment_of_type(OffsetType::Int32, metadata.pointer_size);
        }
        Il2CppTypeEnum::I8 | Il2CppTypeEnum::U8 => {
            sa.size = mem::size_of::<i64>();
            sa.actual_size = mem::size_of::<i64>();
            sa.alignment = get_alignment_of_type(OffsetType::Int64, metadata.pointer_size);
        }
        Il2CppTypeEnum::R4 => {
            sa.size = mem::size_of::<f32>();
            sa.alignment = get_alignment_of_type(OffsetType::Float, metadata.pointer_size);
        }
        Il2CppTypeEnum::R8 => {
            sa.size = mem::size_of::<f64>();
            sa.actual_size = mem::size_of::<f64>();
            sa.alignment = get_alignment_of_type(OffsetType::Double, metadata.pointer_size);
        }

        Il2CppTypeEnum::Ptr
        | Il2CppTypeEnum::Fnptr
        | Il2CppTypeEnum::String
        | Il2CppTypeEnum::Szarray
        | Il2CppTypeEnum::Array
        | Il2CppTypeEnum::Class
        | Il2CppTypeEnum::Object
        | Il2CppTypeEnum::Var
        | Il2CppTypeEnum::Mvar => {
            // voidptr_t
            sa.size = metadata.pointer_size as usize;
            sa.actual_size = metadata.pointer_size as usize;
            sa.alignment = get_alignment_of_type(OffsetType::Pointer, metadata.pointer_size);
        }
        Il2CppTypeEnum::Valuetype => {
            let TypeData::TypeDefinitionIndex(tdi) = ty.data else {panic!("Failed to find a valid TypeDefinitionIndex from type's data: {:?}", ty.data)};
            let td = &metadata.metadata.global_metadata.type_definitions[tdi];

            if td.is_enum_type() {
                let enum_base_type =
                    metadata.metadata_registration.types[td.element_type_index as usize];
                return get_type_size_and_alignment(&enum_base_type, metadata);
            } else {
                // Size of the value type comes from the instance size - size of the wrapper object
                // The way we compute the instance size is by grabbing the TD and performing a full field walk over that type
                // Specifically, we call: layout_fields_for_type
                // TODO: We should cache this call
                let res = layout_fields_for_type(td, tdi, metadata, None);
                sa.actual_size = res.actual_size - metadata.object_size() as usize;
                sa.size = res.size - metadata.object_size() as usize;
                sa.alignment = res.alignment;
                sa.natural_alignment = res.natural_alignment;
            }
        }
        Il2CppTypeEnum::Genericinst => {
            let TypeData::GenericClassIndex(gtype) = ty.data else {panic!("Failed to find a valid GenericClassIndex from type's data: {:?}", ty.data)};
            let mr = &metadata.metadata_registration;
            let generic_class = mr.generic_classes.get(gtype).unwrap();

            let generic_type_def = &mr.types[generic_class.type_index];

            let TypeData::TypeDefinitionIndex(tdi) = generic_type_def.data else {panic!("Failed to find a valid TypeDefinitionIndex from type's data: {:?}", generic_type_def.data)};
            let td = &metadata.metadata.global_metadata.type_definitions[tdi];

            if td.is_value_type() {
                if td.is_enum_type() {
                    let enum_base_type =
                        metadata.metadata_registration.types[td.element_type_index as usize];
                    return get_type_size_and_alignment(&enum_base_type, metadata);
                } else {
                    // Size of the value type comes from the instance size
                    // We compute the instance size by grabbing the TD and performing a full field walk over that type
                    // by calling layout_fields_for_type
                    // TODO: We should cache this call
                    let res = layout_fields_for_type(td, tdi, metadata, None);
                    sa.actual_size = res.actual_size - metadata.object_size() as usize;
                    sa.size = res.size - metadata.object_size() as usize;
                    sa.alignment = res.alignment;
                    sa.natural_alignment = res.natural_alignment;
                }
            } else {
                sa.size = metadata.pointer_size as usize;
                sa.actual_size = metadata.pointer_size as usize;
                sa.alignment = get_alignment_of_type(OffsetType::Pointer, metadata.pointer_size);
                return sa;
            }
        }
        _ => {
            panic!(
                "Failed to compute type size and alignment of type: {:?}",
                ty
            );
        }
    }

    sa
}

fn align_to(size: usize, alignment: usize) -> usize {
    if size & (alignment - 1) != 0 {
        (size + alignment - 1) & !(alignment - 1)
    } else {
        size
    }
}

pub struct SizeAndAlignment {
    pub size: usize,
    actual_size: usize,
    alignment: u8,
    natural_alignment: u8,
}
