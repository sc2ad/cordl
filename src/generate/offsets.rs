use crate::TypeDefinitionIndex;

use brocolib::global_metadata::Il2CppTypeDefinition;
use brocolib::runtime_metadata::Il2CppTypeDefinitionSizes;
use brocolib::runtime_metadata::TypeData;
use brocolib::runtime_metadata::{Il2CppType, Il2CppTypeEnum};
use itertools::Itertools;
use log::debug;

use log::warn;

use crate::generate::type_extensions::TypeExtentions;
use core::mem;

use super::metadata::PointerSize;

use super::metadata::Metadata;
use super::type_extensions::TypeDefinitionExtensions;

const IL2CPP_SIZEOF_STRUCT_WITH_NO_INSTANCE_FIELDS: u32 = 1;

#[derive(Debug, Clone)]
pub struct SizeInfo {
    pub instance_size: u32,
    pub native_size: i32,
    pub calculated_instance_size: u32,
    pub calculated_native_size: i32,
    pub minimum_alignment: u8,
    pub natural_alignment: u8,
    pub packing: Option<u8>,
    pub specified_packing: Option<u8>,
}

pub fn get_size_info<'a>(
    t: &'a Il2CppTypeDefinition,
    tdi: TypeDefinitionIndex,
    generic_inst_types: Option<&Vec<usize>>,
    metadata: &'a Metadata,
) -> SizeInfo {
    let size_metadata = get_size_of_type_table(metadata, tdi).unwrap();
    let mut instance_size = size_metadata.instance_size;
    let mut native_size = size_metadata.native_size;

    let sa = layout_fields(metadata, t, tdi, generic_inst_types, None, true);
    let mut calculated_instance_size = sa.size;

    let minimum_alignment = sa.alignment;
    let natural_alignment = sa.natural_alignment;

    if instance_size == 0 && !t.is_interface() {
        instance_size = sa.size.try_into().unwrap();
        native_size = sa.actual_size.try_into().unwrap();
    }

    if t.is_value_type() || t.is_enum_type() {
        instance_size = instance_size
            .checked_sub(metadata.object_size() as u32)
            .unwrap();
        calculated_instance_size = calculated_instance_size
            .checked_sub(metadata.object_size() as usize)
            .unwrap();
    }

    let packing = get_type_def_packing(metadata, t);
    let specified_packing = get_packing(metadata, t);

    SizeInfo {
        instance_size,
        calculated_instance_size: calculated_instance_size as u32,

        native_size,
        minimum_alignment,
        natural_alignment,
        calculated_native_size: sa.actual_size as i32,
        packing,
        specified_packing,
    }
}

pub fn get_size_and_packing<'a>(
    t: &'a Il2CppTypeDefinition,
    tdi: TypeDefinitionIndex,
    generic_inst_types: Option<&Vec<usize>>,
    metadata: &'a Metadata,
) -> (u32, Option<u8>) {
    let size_metadata = get_size_of_type_table(metadata, tdi).unwrap();
    let mut metadata_size = size_metadata.instance_size;

    if metadata_size == 0 && !t.is_interface() {
        let sa = layout_fields(metadata, t, tdi, generic_inst_types, None, true);
        metadata_size = sa.size.try_into().unwrap();
    }

    if t.is_value_type() || t.is_enum_type() {
        metadata_size = metadata_size
            .checked_sub(metadata.object_size() as u32)
            .unwrap()
    }

    let packing = get_packing(metadata, t);

    (metadata_size, packing)
}

pub fn get_il2cpptype_sa(
    metadata: &Metadata<'_>,
    ty: &Il2CppType,
    generic_inst_types: Option<&Vec<usize>>,
) -> SizeAndAlignment {
    get_type_size_and_alignment(ty, generic_inst_types, metadata)
}

pub fn get_sizeof_type<'a>(
    t: &'a Il2CppTypeDefinition,
    tdi: TypeDefinitionIndex,
    generic_inst_types: Option<&Vec<usize>>,
    metadata: &'a Metadata,
) -> u32 {
    let size_metadata = get_size_of_type_table(metadata, tdi).unwrap();
    let mut metadata_size = size_metadata.instance_size;

    if metadata_size == 0 && !t.is_interface() {
        debug!(
            "Computing instance size by laying out type for tdi: {tdi:?} {}",
            t.full_name(metadata.metadata, true)
        );
        metadata_size = layout_fields(metadata, t, tdi, generic_inst_types, None, true)
            .size
            .try_into()
            .unwrap();
        // Remove implicit size of object from total size of instance
    }

    if t.is_value_type() || t.is_enum_type() {
        // For value types we need to ALWAYS subtract our object size
        metadata_size = metadata_size
            .checked_sub(metadata.object_size() as u32)
            .unwrap();
        debug!(
            "Resulting computed instance size (post subtracting) for type {:?} is: {}",
            t.full_name(metadata.metadata, true),
            metadata_size
        );

        // If we are still 0, todo!
        if metadata_size == 0 {
            todo!("We do not yet support cases where the instance type would be a 0 AFTER we have done computation! type: {}", t.full_name(metadata.metadata, true));
        }
    }

    metadata_size
}

const PACKING_SIZE_ZERO: u32 = 0;
const PACKING_SIZE_ONE: u32 = 1;
const PACKING_SIZE_TWO: u32 = 2;
const PACKING_SIZE_FOUR: u32 = 3;
const PACKING_SIZE_EIGHT: u32 = 4;
const PACKING_SIZE_SIXTEEN: u32 = 5;
const PACKING_SIZE_THIRTYTWO: u32 = 6;
const PACKING_SIZE_SIXTYFOUR: u32 = 7;
const PACKING_SIZE_ONEHUNDREDTWENTYEIGHT: u32 = 8;

// GlobalMetadata::StructLayoutPack
fn packing_value(bitfield: u32, packing_field_offset: u8) -> u8 {
    match (bitfield >> (packing_field_offset - 1)) & 0xF {
        PACKING_SIZE_ZERO => 0,
        PACKING_SIZE_ONE => 1,
        PACKING_SIZE_TWO => 2,
        PACKING_SIZE_FOUR => 4,
        PACKING_SIZE_EIGHT => 8,
        PACKING_SIZE_SIXTEEN => 16,
        PACKING_SIZE_THIRTYTWO => 32,
        PACKING_SIZE_SIXTYFOUR => 64,
        PACKING_SIZE_ONEHUNDREDTWENTYEIGHT => 128,
        _ => {
            warn!("Invalid packing value read");
            0
        }
    }
}

// MetadataCache::StructLayoutPackIsDefault
fn packing_is_default(bitfield: u32, packing_is_default_offset: u8) -> bool {
    ((bitfield >> (packing_is_default_offset - 1)) & 0x1) != 0
}

/// RuntimeType::GetPacking
fn get_packing(metadata: &Metadata<'_>, ty_def: &Il2CppTypeDefinition) -> Option<u8> {
    // according to this, packing is by default n = 8
    // https://learn.microsoft.com/en-us/cpp/preprocessor/pack?view=msvc-170
    if packing_is_default(ty_def.bitfield, metadata.packing_is_default_offset) {
        return None;
    }
    let packing = packing_value(ty_def.bitfield, metadata.specified_packing_field_offset);

    Some(packing)
}

/// GlobalMetadata::FromTypeDefinition
fn get_type_def_packing(metadata: &Metadata, ty_def: &Il2CppTypeDefinition) -> Option<u8> {
    let packing = packing_value(ty_def.bitfield, metadata.packing_field_offset);

    // packing 8 is default
    // 0 is likely None
    if packing == 0 {
        return None;
    }

    Some(packing)
}

// MetadataCache::StructLayoutPackIsDefault
fn size_is_default(bitfield: u32, size_is_default_offset: u8) -> bool {
    ((bitfield >> (size_is_default_offset - 1)) & 0x1) != 0
}

fn get_size(metadata: &Metadata<'_>, tdi: TypeDefinitionIndex, ty_def: &Il2CppTypeDefinition) -> Option<u32> {
    if size_is_default(ty_def.bitfield, metadata.size_is_default_offset) {
        return None;
    }

    get_size_of_type_table(metadata, tdi).map(|sz| sz.native_size as u32)
}

/// Inspired by libil2cpp Class::LayoutFieldsLocked
pub fn layout_fields(
    metadata: &Metadata<'_>,
    declaring_ty_def: &Il2CppTypeDefinition,
    declaring_tdi: TypeDefinitionIndex,
    generic_inst_types: Option<&Vec<usize>>,
    offsets: Option<&mut Vec<u32>>,
    strictly_calculated: bool,
) -> SizeAndAlignment {
    let mut instance_size: usize;
    let mut actual_size: usize;

    let mut minimum_alignment: u8;
    let mut natural_alignment: u8 = 0;

    // packing calculation based on RuntimeType::GetPacking
    let packing = get_type_def_packing(metadata, declaring_ty_def);

    assert!(
        packing.unwrap_or_default() <= 128,
        "Packing must be valid! Actual: {packing:?}",
    );

    // assign base size values based on parent type (or no parent type)
    if declaring_ty_def.parent_index == u32::MAX {
        instance_size = metadata.object_size() as usize;
        actual_size = metadata.object_size() as usize;
        minimum_alignment = metadata.pointer_size as u8;
    } else {
        let parent_sa = get_parent_sa(metadata, declaring_ty_def.parent_index, generic_inst_types);

        instance_size = parent_sa.size;
        actual_size = parent_sa.actual_size;

        if declaring_ty_def.is_value_type() {
            minimum_alignment = 1;
        } else {
            minimum_alignment = parent_sa.alignment;
        }
    }

    // if we have fields, do something with their values
    if declaring_ty_def.field_count > 0 {
        let mut local_offsets: Vec<u32> = vec![];
        let sa = layout_instance_fields(
            metadata,
            declaring_ty_def,
            declaring_tdi,
            generic_inst_types,
            Some(&mut local_offsets),
            SizeAndAlignment {
                size: instance_size,
                actual_size,
                alignment: minimum_alignment,
                natural_alignment,
                packing,
            },
        );

        let mut offsets_opt = offsets;
        if let Some(offsets) = offsets_opt.as_mut() {
            offsets.append(&mut local_offsets);
        }

        if declaring_ty_def.is_value_type() && local_offsets.is_empty() {
            instance_size = (IL2CPP_SIZEOF_STRUCT_WITH_NO_INSTANCE_FIELDS
                + metadata.object_size() as u32) as usize;
            actual_size = (IL2CPP_SIZEOF_STRUCT_WITH_NO_INSTANCE_FIELDS
                + metadata.object_size() as u32) as usize;
        }

        instance_size = update_instance_size_for_generic_class(
            declaring_ty_def,
            declaring_tdi,
            instance_size,
            metadata,
        );

        instance_size = sa.size;
        actual_size = sa.actual_size;
        minimum_alignment = sa.alignment;
        natural_alignment = sa.natural_alignment;
    } else {
        instance_size = update_instance_size_for_generic_class(
            declaring_ty_def,
            declaring_tdi,
            instance_size,
            metadata,
        );
    }

    // if we have an explicit size, use that
    if !strictly_calculated && (declaring_ty_def.is_explicit_layout() || !size_is_default(declaring_ty_def.bitfield, metadata.size_is_default_offset))
        && let Some(sz) = get_size_of_type_table(metadata, declaring_tdi)
    {
        instance_size = sz.instance_size as usize;
        if sz.native_size >= 0 {
            actual_size = sz.native_size as usize;
        }
    }

    SizeAndAlignment {
        size: instance_size,
        actual_size,
        alignment: minimum_alignment,
        natural_alignment,
        packing,
    }
}

/// equivalent to libil2cpp FieldLayout::LayoutFields with the instance field filter
fn layout_instance_fields(
    metadata: &Metadata<'_>,
    declaring_ty_def: &Il2CppTypeDefinition,
    declaring_tdi: TypeDefinitionIndex,
    generic_inst_types: Option<&Vec<usize>>,
    offsets: Option<&mut Vec<u32>>,
    parent_sa: SizeAndAlignment,
) -> SizeAndAlignment {
    let parent_size = parent_sa.size;
    let actual_parent_size = parent_sa.actual_size;
    let parent_alignment = parent_sa.alignment;
    let packing = parent_sa.packing;

    let mut instance_size = parent_size;
    let mut actual_size = actual_parent_size;
    let mut minimum_alignment = parent_alignment;
    let mut natural_alignment: u8 = 0;

    let mut offsets_opt = offsets;
    for (i, f) in declaring_ty_def
        .fields(metadata.metadata)
        .iter()
        .enumerate()
    {
        let field_ty = &metadata
            .metadata
            .runtime_metadata
            .metadata_registration
            .types[f.type_index as usize];

        if field_ty.is_static() || field_ty.is_constant() {
            // filter for instance fields
            continue;
        }

        let sa = get_type_size_and_alignment(field_ty, generic_inst_types, metadata);
        let mut alignment = sa.alignment;
        if alignment < 4 && sa.natural_alignment != 0 {
            alignment = sa.natural_alignment;
        }

        if let Some(packing) = packing
            && packing != 0
        {
            alignment = std::cmp::min(sa.alignment, packing);
        }

        let mut offset = actual_size;

        offset += (alignment - 1) as usize;
        offset &= !(alignment as usize - 1);

        // explicit layout & we have a value in the offset table
        if declaring_ty_def.is_explicit_layout()
            && let Some(special_offset) = get_offset_of_type_table(metadata, declaring_tdi, i)
        {
            offset = special_offset;
        }

        if let Some(offsets) = offsets_opt.as_mut() {
            offsets.push(offset as u32);
        }

        actual_size = usize::max(actual_size, offset + std::cmp::max(sa.size, 1));
        minimum_alignment = std::cmp::max(minimum_alignment, alignment);
        natural_alignment = std::cmp::max(
            natural_alignment,
            std::cmp::max(sa.alignment, sa.natural_alignment),
        );
    }

    instance_size = align_to(actual_size, minimum_alignment as usize);

    SizeAndAlignment {
        size: instance_size,
        actual_size,
        alignment: minimum_alignment,
        natural_alignment,
        packing,
    }
}

fn get_offset_of_type_table(
    metadata: &Metadata<'_>,
    tdi: TypeDefinitionIndex,
    field: usize,
) -> Option<usize> {
    let field_offsets = metadata
        .metadata_registration
        .field_offsets
        .as_ref()
        .unwrap();

    if let Some(offsets) = field_offsets.get(tdi.index() as usize) {
        offsets.get(field).cloned().map(|o| o as usize)
    } else {
        None
    }
}

fn get_parent_sa(
    metadata: &Metadata<'_>,
    parent_index: u32,
    generic_inst_types: Option<&Vec<usize>>,
) -> SizeAndAlignment {
    let parent_ty = &metadata.metadata_registration.types[parent_index as usize];
    let (parent_tdi, parent_generics) = match parent_ty.data {
        TypeData::TypeDefinitionIndex(parent_tdi) => (parent_tdi, None),
        TypeData::GenericClassIndex(generic_index) => {
            let generic_class = &metadata
                .metadata
                .runtime_metadata
                .metadata_registration
                .generic_classes[generic_index];

            let generic_inst = &metadata.metadata_registration.generic_insts
                [generic_class.context.class_inst_idx.unwrap()];

            let generic_ty = &metadata.metadata_registration.types[generic_class.type_index];
            let TypeData::TypeDefinitionIndex(parent_tdi) = generic_ty.data else {
                panic!(
                    "Failed to find TypeDefinitionIndex for generic class: {:?}",
                    generic_ty.data
                );
            };

            // replace all Var with the parent generic args

            let true_generics = generic_inst
                .types
                .iter()
                .map(|t_index| {
                    let ty = &metadata
                        .metadata
                        .runtime_metadata
                        .metadata_registration
                        .types[*t_index];
                    if let TypeData::GenericParameterIndex(generic_param_index) = ty.data
                        && let Some(generic_args) = &generic_inst_types
                    {
                        let generic_param = &metadata.metadata.global_metadata.generic_parameters
                            [generic_param_index];

                        generic_args[generic_param.num as usize]
                    } else {
                        // we don't know what it is, so just default later
                        *t_index
                    }
                })
                .collect_vec();

            (parent_tdi, Some(true_generics))
        }
        _ => todo!("Not yet implemented: {:?}", parent_ty.data),
    };

    layout_fields(
        metadata,
        &metadata.metadata.global_metadata.type_definitions[parent_tdi],
        parent_tdi,
        parent_generics.as_ref(),
        None,
        false,
    )
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
    let generic_type_size = get_size_of_type_table(metadata, tdi)
        .map(|s| s.instance_size)
        .unwrap_or(0) as usize;

    // If the generic class has an instance size, it was explictly set
    if generic_type_size > 0 && generic_type_size > instance_size {
        debug!(
            "Generic instance size overwrite! Old: {}, New: {}, for tdi: {:?}",
            instance_size, generic_type_size, tdi
        );
        return generic_type_size;
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
    match ty {
        OffsetType::Pointer => pointer_size as u8,
        OffsetType::Int8 => 1,
        OffsetType::Int16 => 2,
        OffsetType::Int32 => 4,
        OffsetType::Int64 => 8,
        OffsetType::IntPtr => pointer_size as u8,
        OffsetType::Float => 4,
        OffsetType::Double => 8,
    }
}

fn get_type_size_and_alignment(
    ty: &Il2CppType,
    generic_inst_types: Option<&Vec<usize>>,
    metadata: &Metadata,
) -> SizeAndAlignment {
    let mut sa = SizeAndAlignment {
        alignment: 0,
        natural_alignment: 0,
        size: 0,
        actual_size: 0,
        packing: None,
    };

    if ty.byref && !ty.valuetype {
        sa.size = metadata.pointer_size as usize;
        sa.alignment = get_alignment_of_type(OffsetType::Pointer, metadata.pointer_size);
        return sa;
    }

    // only handle if generic inst, otherwise let the rest handle it as before
    // aka a pointer size
    if ty.ty == Il2CppTypeEnum::Var
        && let TypeData::GenericParameterIndex(generic_param_index) = ty.data
        && let Some(generic_args) = &generic_inst_types
    {
        let generic_param =
            &metadata.metadata.global_metadata.generic_parameters[generic_param_index];

        let resulting_ty_idx = generic_args[generic_param.num as usize];
        let resulting_ty = &metadata.metadata_registration.types[resulting_ty_idx];

        if resulting_ty == ty {
            warn!(
                "Var points to itself! Type: {resulting_ty:?} generic args: {generic_args:?} {}",
                ty.full_name(metadata.metadata)
            );
        }
        // If Var, this is partial instantiation
        // we just treat it as Ptr below
        if resulting_ty.ty != Il2CppTypeEnum::Var {
            return get_type_size_and_alignment(resulting_ty, None, metadata);
        }
    }

    match ty.ty {
        Il2CppTypeEnum::I1 | Il2CppTypeEnum::U1 | Il2CppTypeEnum::Boolean => {
            sa.size = mem::size_of::<i8>();
            sa.actual_size = sa.size;
            sa.alignment = get_alignment_of_type(OffsetType::Int8, metadata.pointer_size);
        }
        Il2CppTypeEnum::I2 | Il2CppTypeEnum::U2 | Il2CppTypeEnum::Char => {
            sa.size = mem::size_of::<i16>();
            sa.actual_size = sa.size;
            sa.alignment = get_alignment_of_type(OffsetType::Int16, metadata.pointer_size);
        }
        Il2CppTypeEnum::I4 | Il2CppTypeEnum::U4 => {
            sa.size = mem::size_of::<i32>();
            sa.actual_size = sa.size;
            sa.alignment = get_alignment_of_type(OffsetType::Int32, metadata.pointer_size);
        }
        Il2CppTypeEnum::I8 | Il2CppTypeEnum::U8 => {
            sa.size = mem::size_of::<i64>();
            sa.actual_size = sa.size;
            sa.alignment = get_alignment_of_type(OffsetType::Int64, metadata.pointer_size);
        }
        Il2CppTypeEnum::R4 => {
            sa.size = mem::size_of::<f32>();
            sa.actual_size = sa.size;
            sa.alignment = get_alignment_of_type(OffsetType::Float, metadata.pointer_size);
        }
        Il2CppTypeEnum::R8 => {
            sa.size = mem::size_of::<f64>();
            sa.actual_size = sa.size;
            sa.alignment = get_alignment_of_type(OffsetType::Double, metadata.pointer_size);
        }

        Il2CppTypeEnum::Ptr
        | Il2CppTypeEnum::Fnptr
        | Il2CppTypeEnum::String
        | Il2CppTypeEnum::Szarray
        | Il2CppTypeEnum::Array
        | Il2CppTypeEnum::Class
        | Il2CppTypeEnum::Object
        | Il2CppTypeEnum::Mvar
        | Il2CppTypeEnum::Var
        | Il2CppTypeEnum::I
        | Il2CppTypeEnum::U => {
            // voidptr_t
            sa.size = metadata.pointer_size as usize;
            sa.actual_size = sa.size;
            sa.alignment = get_alignment_of_type(OffsetType::Pointer, metadata.pointer_size);
        }
        Il2CppTypeEnum::Valuetype => {
            let TypeData::TypeDefinitionIndex(value_tdi) = ty.data else {
                panic!(
                    "Failed to find a valid TypeDefinitionIndex from type's data: {:?}",
                    ty.data
                )
            };
            let value_td = &metadata.metadata.global_metadata.type_definitions[value_tdi];

            if value_td.is_enum_type() {
                let enum_base_type =
                    metadata.metadata_registration.types[value_td.element_type_index as usize];
                return get_type_size_and_alignment(&enum_base_type, None, metadata);
            }

            // Size of the value type comes from the instance size - size of the wrapper object
            // The way we compute the instance size is by grabbing the TD and performing a full field walk over that type
            // Specifically, we call: layout_fields_for_type
            // TODO: We should cache this call
            let res = layout_fields(metadata, value_td, value_tdi, None, None, false);
            sa.size = res.size - metadata.object_size() as usize;
            sa.actual_size = res.actual_size;
            sa.alignment = res.alignment;
            sa.natural_alignment = res.natural_alignment;
            sa.packing = res.packing;
        }
        Il2CppTypeEnum::Genericinst => {
            let TypeData::GenericClassIndex(gtype) = ty.data else {
                panic!(
                    "Failed to find a valid GenericClassIndex from type's data: {:?}",
                    ty.data
                )
            };
            let mr = &metadata.metadata_registration;
            let generic_class = mr.generic_classes.get(gtype).unwrap();

            let new_generic_inst = &mr.generic_insts[generic_class.context.class_inst_idx.unwrap()];

            let generic_type_def = &mr.types[generic_class.type_index];

            let TypeData::TypeDefinitionIndex(tdi) = generic_type_def.data else {
                panic!(
                    "Failed to find a valid TypeDefinitionIndex from type's data: {:?}",
                    generic_type_def.data
                )
            };
            let td = &metadata.metadata.global_metadata.type_definitions[tdi];

            // reference type
            if !td.is_value_type() && !td.is_enum_type() {
                sa.size = metadata.pointer_size as usize;
                sa.alignment = get_alignment_of_type(OffsetType::Pointer, metadata.pointer_size);
                return sa;
            }

            // enum type
            if td.is_enum_type() {
                let enum_base_type =
                    metadata.metadata_registration.types[td.element_type_index as usize];
                return get_type_size_and_alignment(
                    &enum_base_type,
                    Some(&new_generic_inst.types),
                    metadata,
                );
            }

            // GenericInst fields can use generic args of their declaring type
            // so we redirect Var to the declaring type args
            let new_generic_inst_types = new_generic_inst
                .types
                .iter()
                .map(|t_idx| {
                    let t = &metadata.metadata_registration.types[*t_idx];

                    match t.data {
                        TypeData::GenericParameterIndex(generic_param_idx) => {
                            let generic_param =
                                &metadata.metadata.global_metadata.generic_parameters
                                    [generic_param_idx];

                            generic_inst_types
                                .map(|generic_inst_types| {
                                    generic_inst_types[generic_param.num as usize]
                                })
                                // fallback to Var because we may not pass generic types
                                // when sizing a type def
                                .unwrap_or(*t_idx)
                        }
                        _ => *t_idx,
                    }
                })
                .collect_vec();

            // Size of the value type comes from the instance size
            // We compute the instance size by grabbing the TD and performing a full field walk over that type
            // by calling layout_fields_for_type
            // TODO: We should cache this call
            let res = layout_fields(metadata, td, tdi, Some(&new_generic_inst_types), None, false);
            sa.size = res.size - metadata.object_size() as usize;
            sa.actual_size = res.actual_size;
            sa.alignment = res.alignment;
            sa.natural_alignment = res.natural_alignment;
            sa.packing = res.packing;
            // sa.natural_alignment = res.natural_alignment;
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

#[derive(Debug)]
pub struct SizeAndAlignment {
    pub size: usize,
    actual_size: usize,
    alignment: u8,
    natural_alignment: u8,
    packing: Option<u8>,
}
