#pragma once

#include "config.hpp"
#include "size-utils.hpp"
#include "ptr-utils.hpp"
#include "method-utils.hpp"
#include "field-utils.hpp"

#include "beatsaber-hook/shared/utils/byref.hpp"
#include "beatsaber-hook/shared/utils/il2cpp-utils-methods.hpp"
#include "beatsaber-hook/shared/utils/il2cpp-utils-properties.hpp"
#include "beatsaber-hook/shared/utils/il2cpp-utils-fields.hpp"
#include "beatsaber-hook/shared/utils/utils.h"
#include "beatsaber-hook/shared/utils/typedefs.h"

#include "concepts.hpp"
#include "box-utils.hpp"

// TODO: Implement
template <typename T>
using ByRefConst = ::ByRef<T>;

template <typename T, typename Ptr> struct ArrayW;
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
}
}
