#pragma once

#include "config.hpp"
#include "concepts.hpp"
#include "internal.hpp"
#include "beatsaber-hook/shared/utils/base-wrapper-type.hpp"
#include "beatsaber-hook/shared/utils/il2cpp-utils.hpp"

namespace cordl_internals {
#pragma region boxing
    template<typename T>
    CORDL_HIDDEN ::bs_hook::Il2CppWrapperType Box(T&&);

    template<typename T>
    CORDL_HIDDEN ::bs_hook::Il2CppWrapperType Box(T);

    template<typename T>
    CORDL_HIDDEN ::bs_hook::Il2CppWrapperType Box(T*);

    template<>
    CORDL_HIDDEN constexpr ::bs_hook::Il2CppWrapperType Box<::bs_hook::Il2CppWrapperType>(::bs_hook::Il2CppWrapperType t) { return t; }

    template<>
    CORDL_HIDDEN constexpr ::bs_hook::Il2CppWrapperType Box<::bs_hook::Il2CppWrapperType&&>(::bs_hook::Il2CppWrapperType&& t) { return t; }

    template<il2cpp_value_type T>
    CORDL_HIDDEN ::bs_hook::Il2CppWrapperType Box(T&& t) {
        return ::bs_hook::Il2CppWrapperType(il2cpp_functions::value_box(classof(T), const_cast<void*>(static_cast<const void*>(t.__instance.data()))));
    }

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
    CORDL_HIDDEN T Unbox(::bs_hook::Il2CppWrapperType);

    template<typename T>
    CORDL_HIDDEN T Unbox(::bs_hook::Il2CppWrapperType&&);

    template<il2cpp_reference_type T>
    CORDL_HIDDEN T Unbox(::bs_hook::Il2CppWrapperType t) { return T(t.convert()); }

    template<il2cpp_value_type T>
    CORDL_HIDDEN T Unbox(::bs_hook::Il2CppWrapperType t) {
        std::remove_const_t<T> v{};
        copyByByte<sizeof(v.__instance)>(
            reinterpret_cast<void*>(il2cpp_functions::object_unbox(t)),
            reinterpret_cast<void*>(v.__instance.data())
        );
        return v;
    }

    template<il2cpp_value_type T>
    CORDL_HIDDEN T Unbox(::bs_hook::Il2CppWrapperType&& t) {
        std::remove_const_t<T> v{};
        copyByByte<sizeof(v.__instance)>(
            reinterpret_cast<void*>(il2cpp_functions::object_unbox(t)),
            reinterpret_cast<void*>(v.__instance.data())
        );
        return v;
    }

    template<typename T>
    requires(!il2cpp_reference_type<T> && !il2cpp_value_type<T>)
    CORDL_HIDDEN T Unbox(::bs_hook::Il2CppWrapperType t) {
        std::remove_const_t<T> v{};
        copyByByte<sizeof(v.__instance)>(
            reinterpret_cast<void*>(il2cpp_functions::object_unbox(t)),
            reinterpret_cast<void*>(&v)
        );
        return v;
    }

    template<typename T>
    requires(!il2cpp_reference_type<T> && !il2cpp_value_type<T>)
    CORDL_HIDDEN T Unbox(::bs_hook::Il2CppWrapperType&& t) {
        std::remove_const_t<T> v{};
        copyByByte<sizeof(v.__instance)>(
            reinterpret_cast<void*>(il2cpp_functions::object_unbox(t)),
            reinterpret_cast<void*>(&v)
        );
        return v;
    }
#pragma endregion // unboxing

}
