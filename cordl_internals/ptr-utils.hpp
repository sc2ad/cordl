#pragma once

#include <type_traits>
#include <concepts>

#include "config.hpp"
#include "concepts.hpp"

namespace {
namespace cordl_internals {
    template <typename T>
    requires(std::is_pointer_v<T>)
    using to_const_pointer = std::remove_pointer_t<T> const*;

    
    
    /// @brief type to wrap a pointer to a T, not recommended to be used with anything that's not il2cpp compatible
    /// @tparam T type that instance points to
    template<typename T>
    requires(!::il2cpp_utils::il2cpp_reference_type_wrapper<T>)
    struct Ptr {
        constexpr explicit Ptr(void* i) : instance(i) {}
        constexpr void* convert() const { return const_cast<void*>(instance); }

        constexpr Ptr(T* i) : instance(i) {}
        constexpr Ptr(T& i) : instance(&i) {}

        constexpr operator T&() const { return *static_cast<T*>(const_cast<void*>(instance)); }
        constexpr operator T*() const { return static_cast<T*>(const_cast<void*>(instance)); }
        T* operator ->() const { return static_cast<T*>(const_cast<void*>(instance)); }

        protected:
            void* instance;
    };

    // specific instantiation for void pointers
    template<>
    struct Ptr<void> {
        constexpr Ptr(void* i) : instance(i) {}
        constexpr void* convert() const { return const_cast<void*>(instance); }
        constexpr operator void*() const { return const_cast<void*>(instance); }

        protected:
            void* instance;
    };

    static_assert(sizeof(Ptr<void>) == sizeof(void*));
}
} // end anonymous namespace
// Ptr is neither Ref nor Val type
template<> struct CORDL_HIDDEN ::il2cpp_utils::GenRefTypeTrait<::cordl_internals::Ptr> { constexpr static bool value = false; };
template<> struct CORDL_HIDDEN ::il2cpp_utils::GenValueTypeTrait<::cordl_internals::Ptr> { constexpr static bool value = false; };

template<typename T>
struct CORDL_HIDDEN ::il2cpp_utils::il2cpp_type_check::il2cpp_no_arg_type<::cordl_internals::Ptr<T>> {
    static inline const Il2CppType* get() {
        static auto* typ = &::il2cpp_utils::il2cpp_type_check::il2cpp_no_arg_class<T>::get()->this_arg;
        return typ;
    }
};

template<typename T>
struct CORDL_HIDDEN ::il2cpp_utils::il2cpp_type_check::il2cpp_arg_type<::cordl_internals::Ptr<T>> {
    static inline const Il2CppType* get([[maybe_unused]] ::cordl_internals::Ptr<T> arg) {
        return ::il2cpp_utils::il2cpp_type_check::il2cpp_no_arg_type<::cordl_internals::Ptr<T>>::get();
    }
};
