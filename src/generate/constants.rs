use brocolib::{
    global_metadata::{Il2CppMethodDefinition, Il2CppTypeDefinition},
    runtime_metadata::Il2CppType,
};

pub const OBJECT_WRAPPER_TYPE: &&str = &"::bs_hook::Il2CppWrapperType";

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
}

pub trait TypeExtentions {
    fn is_static(&self) -> bool;
    fn is_const(&self) -> bool;
    fn is_byref(&self) -> bool;
}

impl TypeExtentions for Il2CppType {
    fn is_static(&self) -> bool {
        (self.attrs & FIELD_ATTRIBUTE_STATIC) != 0
    }

    // FIELD_ATTRIBUTE_LITERAL
    fn is_const(&self) -> bool {
        (self.attrs & FIELD_ATTRIBUTE_LITERAL) != 0
    }

    fn is_byref(&self) -> bool {
        self.byref
    }
}

pub trait TypeDefinitionExtensions {
    fn is_value_type(&self) -> bool;
    fn is_enum_type(&self) -> bool;
}

impl TypeDefinitionExtensions for Il2CppTypeDefinition {
    fn is_value_type(&self) -> bool {
        self.bitfield & 1 != 0
    }

    fn is_enum_type(&self) -> bool {
        self.bitfield & 2 != 0
    }
}
