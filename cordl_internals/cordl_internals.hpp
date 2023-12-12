#pragma once

#include "beatsaber-hook/shared/utils/byref.hpp"
#include "beatsaber-hook/shared/utils/il2cpp-utils-methods.hpp"
#include "beatsaber-hook/shared/utils/il2cpp-utils-properties.hpp"
#include "beatsaber-hook/shared/utils/il2cpp-utils-fields.hpp"
#include "beatsaber-hook/shared/utils/utils.h"
#include "beatsaber-hook/shared/utils/typedefs.h"

#include "config.hpp"
#include "concepts.hpp"
#include "field-utils.hpp"
#include "box-utils.hpp"
#include "method-utils.hpp"
#include "ptr-utils.hpp"
#include "size-utils.hpp"

#ifndef csnull
#define csnull (::cordl_internals::NullArg())
#endif

// TODO: Implement
template <typename T>
using ByRefConst = ::ByRef<T>;

struct StringW;
template <typename T, typename Ptr> struct ArrayW;
template <typename T, typename Ptr> struct ListW;
struct Il2CppObject;


namespace {
namespace cordl_internals {
    // Base type for interfaces, as interfaces will wrap instances too (autoboxed VTs as well)
    struct InterfaceW : public ::bs_hook::Il2CppWrapperType {
        explicit constexpr InterfaceW(void* o) noexcept : ::bs_hook::Il2CppWrapperType(o) {}

        constexpr static bool __IL2CPP_VALUE_TYPE = false;

        // TODO: operator to safely typecast to types it may be implemented on? maybe better as an operator on whatever inherits this...
        // something that has a requires(std::is_convertible_v<T, Self>)... conversion can always be forced by using .convert() anyway
    };

    // Type tag for passing null as a parameter without setting instance to null
    struct NullArg {
        template <::il2cpp_utils::il2cpp_reference_type_wrapper T> constexpr operator T() const noexcept {
            return T(nullptr);
        }
        constexpr operator ::bs_hook::Il2CppWrapperType() const noexcept {
            return ::bs_hook::Il2CppWrapperType(nullptr);
        }

        // convert to null anyways
        // this might cause issues when we have `Foo(::il2cpp_utils::il2cpp_reference_type_wrapper)` and
        // `Foo(void*)`, hopefully not
        constexpr operator std::nullptr_t() const noexcept {
            return nullptr;
        }
        constexpr operator ::StringW() const noexcept {
            return StringW(nullptr);
        }

        template <typename T, typename Ptr> constexpr operator ::ArrayW<T, Ptr>() const noexcept {
            return ArrayW<T, Ptr>(nullptr);
        }

        template <typename T, typename U>
        constexpr operator ::ListW<T, U>() const noexcept {
            return ListW<T, U>(nullptr);
        }
    };

    using intptr_t = void*;
    using uintptr_t = void*;
    using voidptr_t = void*; // dumb declspec stuff
}
}
