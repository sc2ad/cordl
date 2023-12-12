#pragma once

#include "config.hpp"
#include "concepts.hpp"
#include "internal.hpp"
#include "beatsaber-hook/shared/utils/base-wrapper-type.hpp"
#include "beatsaber-hook/shared/utils/il2cpp-type-check.hpp"
#include "beatsaber-hook/shared/utils/il2cpp-functions.hpp"

namespace {
namespace cordl_internals {
#pragma region boxing
    template<typename T>
    CORDL_HIDDEN Il2CppObject* Box(T);

    template<typename T>
    CORDL_HIDDEN Il2CppObject* Box(T*);

    template<>
    CORDL_HIDDEN constexpr Il2CppObject* Box<Il2CppObject*>(Il2CppObject* t) { return t; }

    template<il2cpp_convertible T>
    requires(!std::is_base_of_v<Il2CppObject*, T>)
    CORDL_HIDDEN Il2CppObject* Box(T t) {
        return il2cpp_functions::value_box(classof(T), t.convert());
    }
    template <il2cpp_convertible T>
    requires(!std::is_base_of_v<Il2CppObject*, T>)
    CORDL_HIDDEN Il2CppObject* Box(T* t) {
        return il2cpp_functions::value_box(classof(T), t->convert());
    }
#pragma endregion // boxing

#pragma region unboxing
    template<typename T>
    CORDL_HIDDEN T Unbox(Il2CppObject* t) {
        return *reinterpret_cast<T*>(il2cpp_functions::object_unbox(t));
    }

    template<::il2cpp_utils::il2cpp_reference_type_wrapper T>
    CORDL_HIDDEN T Unbox(Il2CppObject* t) { return T(t); }

    template<::il2cpp_utils::il2cpp_reference_type_pointer T>
    CORDL_HIDDEN T Unbox(Il2CppObject* t) { return reinterpret_cast<T>(t); }

    template<::il2cpp_utils::il2cpp_value_type T>
    CORDL_HIDDEN T Unbox(Il2CppObject* t) {
        std::array<std::byte, il2cpp_instance_sizeof(T)> data;
        std::memcpy(data.data(), il2cpp_functions::object_unbox(t), il2cpp_instance_sizeof(T));
        return T(std::move(data));
    }
#pragma endregion // unboxing

}
} // end anonymous namespace
