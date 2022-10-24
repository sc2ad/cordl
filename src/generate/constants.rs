use il2cpp_binary::Type;
use il2cpp_metadata_raw::Il2CppTypeDefinition;

pub trait TypeExtentions {
    fn is_static(&self) -> bool;
    fn is_const(&self) -> bool;
    fn is_byref(&self) -> bool;
    fn is_public_method(&self) -> bool;
    fn is_abstract_method(&self) -> bool;
    fn is_static_method(&self) -> bool;
    fn is_virtual_method(&self) -> bool;
    fn is_param_in(&self) -> bool;
    fn is_param_out(&self) -> bool;
    fn is_param_optional(&self) -> bool;
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
    fn is_public_method(&self) -> bool {
        (self.attrs & 7) == 6
    }

    fn is_virtual_method(&self) -> bool {
        (self.attrs & 0x0040) != 0
    }

    fn is_static_method(&self) -> bool {
        (self.attrs & 0x0010) != 0
    }

    fn is_abstract_method(&self) -> bool {
        (self.attrs & 0x0400) != 0
    }

    fn is_param_in(&self) -> bool {
        (self.attrs & 0x0001) != 0
    }

    fn is_param_out(&self) -> bool {
        (self.attrs & 0x0002) != 0
    }

    fn is_param_optional(&self) -> bool {
        (self.attrs & 0x0010) != 0
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