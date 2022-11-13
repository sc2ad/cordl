use il2cpp_binary::Type;
use il2cpp_metadata_raw::{Il2CppMethodDefinition, Il2CppTypeDefinition};

pub trait MethodDefintionExtensions {
    fn is_public_method(&self) -> bool;
    fn is_abstract_method(&self) -> bool;
    fn is_static_method(&self) -> bool;
    fn is_virtual_method(&self) -> bool;
    fn is_hidden_sig(&self) -> bool;
    fn is_special_name(&self) -> bool;
}

impl MethodDefintionExtensions for Il2CppMethodDefinition {
    fn is_public_method(&self) -> bool {
        (self.flags & 7) == 6
    }

    fn is_virtual_method(&self) -> bool {
        (self.flags & 0x0040) != 0
    }

    fn is_static_method(&self) -> bool {
        (self.flags & 0x0010) != 0
    }

    fn is_abstract_method(&self) -> bool {
        (self.flags & 0x0400) != 0
    }

    fn is_hidden_sig(&self) -> bool {
        (self.flags & 0x0080) != 0
    }

    fn is_special_name(&self) -> bool {
        (self.flags & 0x0800) != 0
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

impl TypeExtentions for Type {
    fn is_static(&self) -> bool {
        (self.attrs & 0x0010) != 0
    }

    // FIELD_ATTRIBUTE_LITERAL
    fn is_const(&self) -> bool {
        (self.attrs & 0x0040) != 0
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
