#pragma once

#include <concepts>
#include <type_traits>
#include "beatsaber-hook/shared/utils/type-concepts.hpp"
#include "beatsaber-hook/shared/utils/size-concepts.hpp"

namespace {
namespace cordl_internals {
    template <class T, class U>
    concept convertible_to = std::is_convertible_v<T, U>;

    template <class T, class U>
    concept is_or_is_backed_by =
        std::is_same_v<T, U> || (requires {
          typename T::__CORDL_BACKING_ENUM_TYPE;
        } && std::is_same_v<typename T::__CORDL_BACKING_ENUM_TYPE, U>);

    template <typename T>
    concept il2cpp_convertible = requires(T const& t) {
        {t.convert()} -> convertible_to<void*>;
    };

#pragma region offset check
    /// @brief struct to check validity of an offset, since the requires clause makes it so only valid structs for this exist, we get nice errors
    /// @tparam instance_sz the size of the instance
    /// @tparam offset the offset of the field
    /// @tparam value_sz the size of the field
    template<std::size_t instance_sz, std::size_t offset, std::size_t value_sz>
    requires(offset <= instance_sz && (offset + value_sz) <= instance_sz)
    struct offset_check {
        static constexpr bool value = true;
    };

    /// @brief shorthand to offset_check<...>::value
    /// @tparam instance_sz the size of the instance
    /// @tparam offset the offset of the field
    /// @tparam value_sz the size of the field
    template<std::size_t instance_sz, std::size_t offset, std::size_t value_sz>
    constexpr bool offset_check_v = offset_check<instance_sz, offset, value_sz>::value;

    // if you compile with the define COMPILE_TIME_OFFSET_CHECKS cordl will evaluate each field access to check whether you are going out of bounds for the field access, and if so it will not compile
    // this shouldn't happen ever, but it helps as a sanity check. can be disabled to save on compile time
    #ifdef COMPILE_TIME_OFFSET_CHECKS
        #define OFFSET_CHECK(instance_size, offset, value_size, message) static_assert(::cordl_internals::offset_check_v<instance_size, offset, value_size>, message)
    #else
        #define OFFSET_CHECK(instance_size, offset, value_size, message)
    #endif

    // if you compile with the define COMPILE_TIME_SIZE_CHECKS cordl will evaluate all sizes of objects to see whether they match what il2cpp says they should be
    // this should always be fine, but it helps as a sanity check. can be disabled to save on compile time
    #ifdef COMPILE_TIME_SIZE_CHECKS
        #define SIZE_CHECK(t, message) static_assert(il2cpp_safe(t), message)
    #else
        #define SIZE_CHECK(t, message)
    #endif
#pragma endregion // offset check
}
} // end anonymous namespace
