#pragma once

#include "config.hpp"
#include "concepts.hpp"
#include "internal.hpp"
#include "beatsaber-hook/shared/utils/base-wrapper-type.hpp"
#include "beatsaber-hook/shared/utils/il2cpp-utils.hpp"

namespace cordl_internals {
#pragma region boxing
    template<typename T>
    CORDL_HIDDEN ::bs_hook::Il2CppWrapperType Box(T);

    template<typename T>
    CORDL_HIDDEN ::bs_hook::Il2CppWrapperType Box(T*);

    template<>
    CORDL_HIDDEN constexpr ::bs_hook::Il2CppWrapperType Box<::bs_hook::Il2CppWrapperType>(::bs_hook::Il2CppWrapperType t) { return t; }

    template<il2cpp_value_type T>
    CORDL_HIDDEN ::bs_hook::Il2CppWrapperType Box(T t) {
        return ::bs_hook::Il2CppWrapperType(il2cpp_functions::value_box(classof(T), const_cast<void*>(static_cast<const void*>(t.__instance.data()))));
    }

    template<il2cpp_value_type T>
    CORDL_HIDDEN ::bs_hook::Il2CppWrapperType Box(T* t) {
        return ::bs_hook::Il2CppWrapperType(il2cpp_functions::value_box(classof(T), const_cast<void*>(static_cast<const void*>(t->__instance.data()))));
    }
#pragma endregion // boxing

#pragma region unboxing
    template<typename T>
    CORDL_HIDDEN T Unbox(::bs_hook::Il2CppWrapperType t) {
        return *reinterpret_cast<T*>(il2cpp_functions::object_unbox(t));
    }

    template<il2cpp_reference_type T>
    CORDL_HIDDEN T Unbox(::bs_hook::Il2CppWrapperType t) { return T(t.convert()); }

    template<il2cpp_value_type T>
    CORDL_HIDDEN T Unbox(::bs_hook::Il2CppWrapperType t) {
        std::array<uint8_t, sizeof(T)> v; // shitty way of getting a 0 inited value type struct even if it doesn't have a default ctor
        auto val = reinterpret_cast<T*>(v.data());
        std::memcpy(val->__instance.data(), il2cpp_functions::object_unbox(t), T::__CORDL_VALUE_TYPE_SIZE);
        return *val;
    }
#pragma endregion // unboxing

}
