use brocolib::{
    global_metadata::{Il2CppMethodDefinition, Il2CppTypeDefinition},
    runtime_metadata::{Il2CppType, Il2CppTypeEnum},
    Metadata,
};

use super::config::GenerationConfig;

pub const OBJECT_WRAPPER_TYPE: &str = "::bs_hook::Il2CppWrapperType";

pub const PARAM_ATTRIBUTE_IN: u16 = 0x0001;
pub const PARAM_ATTRIBUTE_OUT: u16 = 0x0002;
pub const PARAM_ATTRIBUTE_OPTIONAL: u16 = 0x0010;

pub const TYPE_ATTRIBUTE_INTERFACE: u32 = 0x00000020;
pub const TYPE_ATTRIBUTE_NESTED_PUBLIC: u32 = 0x00000002;

pub const FIELD_ATTRIBUTE_PUBLIC: u16 = 0x0006;
pub const FIELD_ATTRIBUTE_PRIVATE: u16 = 0x0001;
pub const FIELD_ATTRIBUTE_STATIC: u16 = 0x0010;
pub const FIELD_ATTRIBUTE_LITERAL: u16 = 0x0040;

pub const METHOD_ATTRIBUTE_PUBLIC: u16 = 0x0006;
pub const METHOD_ATTRIBUTE_STATIC: u16 = 0x0010;
pub const METHOD_ATTRIBUTE_FINAL: u16 = 0x0020;
pub const METHOD_ATTRIBUTE_VIRTUAL: u16 = 0x0040;
pub const METHOD_ATTRIBUTE_HIDE_BY_SIG: u16 = 0x0080;
pub const METHOD_ATTRIBUTE_ABSTRACT: u16 = 0x0400;
pub const METHOD_ATTRIBUTE_SPECIAL_NAME: u16 = 0x0800;

pub trait MethodDefintionExtensions {
    fn is_public_method(&self) -> bool;
    fn is_abstract_method(&self) -> bool;
    fn is_static_method(&self) -> bool;
    fn is_virtual_method(&self) -> bool;
    fn is_hidden_sig(&self) -> bool;
    fn is_special_name(&self) -> bool;
    fn is_final_method(&self) -> bool;
}

impl MethodDefintionExtensions for Il2CppMethodDefinition {
    fn is_public_method(&self) -> bool {
        (self.flags & METHOD_ATTRIBUTE_PUBLIC) != 0
    }

    fn is_virtual_method(&self) -> bool {
        (self.flags & METHOD_ATTRIBUTE_VIRTUAL) != 0
    }

    fn is_static_method(&self) -> bool {
        (self.flags & METHOD_ATTRIBUTE_STATIC) != 0
    }

    fn is_abstract_method(&self) -> bool {
        (self.flags & METHOD_ATTRIBUTE_ABSTRACT) != 0
    }

    fn is_hidden_sig(&self) -> bool {
        (self.flags & METHOD_ATTRIBUTE_HIDE_BY_SIG) != 0
    }

    fn is_special_name(&self) -> bool {
        (self.flags & METHOD_ATTRIBUTE_SPECIAL_NAME) != 0
    }

    fn is_final_method(&self) -> bool {
        (self.flags & METHOD_ATTRIBUTE_FINAL) != 0
    }
}

pub trait ParameterDefinitionExtensions {
    fn is_param_optional(&self) -> bool;
    fn is_param_in(&self) -> bool;
    fn is_param_out(&self) -> bool;
}

impl ParameterDefinitionExtensions for Il2CppType {
    fn is_param_optional(&self) -> bool {
        (self.attrs & PARAM_ATTRIBUTE_OPTIONAL) != 0
    }

    fn is_param_in(&self) -> bool {
        (self.attrs & PARAM_ATTRIBUTE_IN) != 0
    }

    fn is_param_out(&self) -> bool {
        (self.attrs & PARAM_ATTRIBUTE_OUT) != 0
    }
}

pub trait TypeExtentions {
    fn is_static(&self) -> bool;
    fn is_constant(&self) -> bool;
    fn is_byref(&self) -> bool;
}

impl TypeExtentions for Il2CppType {
    fn is_static(&self) -> bool {
        (self.attrs & FIELD_ATTRIBUTE_STATIC) != 0
    }

    // FIELD_ATTRIBUTE_LITERAL
    fn is_constant(&self) -> bool {
        (self.attrs & FIELD_ATTRIBUTE_LITERAL) != 0
    }

    fn is_byref(&self) -> bool {
        self.byref
    }
}

pub trait TypeDefinitionExtensions {
    fn is_value_type(&self) -> bool;
    fn is_enum_type(&self) -> bool;
    fn is_interface(&self) -> bool;

    fn full_name_cpp(
        &self,
        metadata: &Metadata,
        config: &GenerationConfig,
        with_generics: bool,
    ) -> String;
    fn full_name_nested(
        &self,
        metadata: &Metadata,
        config: &GenerationConfig,
        with_generics: bool,
    ) -> String;
}

impl TypeDefinitionExtensions for Il2CppTypeDefinition {
    fn is_value_type(&self) -> bool {
        self.bitfield & 1 != 0
    }

    fn is_enum_type(&self) -> bool {
        self.bitfield & 2 != 0
    }

    fn is_interface(&self) -> bool {
        self.flags & TYPE_ATTRIBUTE_INTERFACE != 0
    }

    fn full_name_cpp(
        &self,
        metadata: &Metadata,
        config: &GenerationConfig,
        with_generics: bool,
    ) -> String {
        let namespace = config.namespace_cpp(self.namespace(metadata));
        let name = config.name_cpp(self.name(metadata));

        let mut full_name = String::from("::");

        if self.declaring_type_index != u32::MAX {
            let declaring_ty = metadata.runtime_metadata.metadata_registration.types
                [self.declaring_type_index as usize];

            let s = match declaring_ty.data {
                brocolib::runtime_metadata::TypeData::TypeDefinitionIndex(tdi) => {
                    let declaring_td = &metadata.global_metadata.type_definitions[tdi];
                    declaring_td.full_name_cpp(metadata, config, with_generics)
                }
                _ => declaring_ty.full_name(metadata),
            };

            full_name.push_str(&s);
            full_name.push_str("::");
        } else {
            // only write namespace if no declaring type
            full_name.push_str(&namespace);
            full_name.push_str("::");
        }

        full_name.push_str(&name);
        if self.generic_container_index.is_valid() && with_generics {
            let gc = self.generic_container(metadata);
            full_name.push_str(&gc.to_string(metadata));
        }
        full_name
    }

    fn full_name_nested(
        &self,
        metadata: &Metadata,
        config: &GenerationConfig,
        with_generics: bool,
    ) -> String {
        let namespace = config.namespace_cpp(self.namespace(metadata));
        let name = config.name_cpp(self.name(metadata));

        let mut full_name = String::new();

        if self.declaring_type_index != u32::MAX {
            let declaring_ty = metadata.runtime_metadata.metadata_registration.types
                [self.declaring_type_index as usize];

            let declaring_ty = match declaring_ty.data {
                brocolib::runtime_metadata::TypeData::TypeDefinitionIndex(tdi) => {
                    let declaring_td = &metadata.global_metadata.type_definitions[tdi];
                    declaring_td.full_name_nested(metadata, config, with_generics)
                }
                _ => declaring_ty.full_name(metadata),
            };

            full_name.push_str(&declaring_ty);
            full_name.push('/');
        } else {
            // only write namespace if no declaring type
            full_name.push_str(&namespace);
            full_name.push('.');
        }

        full_name.push_str(&name);
        if self.generic_container_index.is_valid() && with_generics {
            let gc = self.generic_container(metadata);
            full_name.push_str(&gc.to_string(metadata));
        }
        full_name
    }
}

pub trait Il2CppTypeEnumExtensions {
    fn is_primitive_builtin(&self) -> bool;
}

impl Il2CppTypeEnumExtensions for Il2CppTypeEnum {
    fn is_primitive_builtin(&self) -> bool {
        !matches!(
            self,
            Il2CppTypeEnum::Byref
                | Il2CppTypeEnum::Valuetype
                | Il2CppTypeEnum::Class
                | Il2CppTypeEnum::Var
                | Il2CppTypeEnum::Array
                | Il2CppTypeEnum::Genericinst
                | Il2CppTypeEnum::Typedbyref
                | Il2CppTypeEnum::I
                | Il2CppTypeEnum::U
                | Il2CppTypeEnum::Fnptr
                | Il2CppTypeEnum::Object
                | Il2CppTypeEnum::Szarray
                | Il2CppTypeEnum::Mvar
                | Il2CppTypeEnum::Internal
                | Il2CppTypeEnum::Modifier
                | Il2CppTypeEnum::Sentinel
                | Il2CppTypeEnum::Pinned
                | Il2CppTypeEnum::Enum
        )
    }
}
