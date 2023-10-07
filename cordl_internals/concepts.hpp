#pragma once

#include <concepts>
#include <type_traits>
#include "beatsaber-hook/shared/utils/base-wrapper-type.hpp"
#include "beatsaber-hook/shared/utils/value-wrapper-type.hpp"
#include "beatsaber-hook/shared/utils/enum-wrapper-type.hpp"
// #include "beatsaber-hook/shared/utils/typedefs-string.hpp"
// #include "beatsaber-hook/shared/utils/typedefs-array.hpp"
// #include "beatsaber-hook/shared/utils/typedefs-list.hpp"

template <typename T, typename U> struct ArrayW;

struct StringW;



namespace {
namespace cordl_internals {
    template <typename T, typename U>
    concept convertible_to = std::is_convertible_v<T, U>;

    template<typename T>
    concept has_value_marker = requires {
        { T::__CORDL_IS_VALUE_TYPE } -> convertible_to<bool>;
    };

    template<typename T, bool check>
    constexpr bool value_marker_check_v = false;

    template<has_value_marker T, bool check>
    constexpr bool value_marker_check_v<T, check> = T::__CORDL_IS_VALUE_TYPE == check;

#pragma region val type
    template <typename T>
    concept il2cpp_value_type_requirements = requires(T const& t) {
        { std::is_array_v<decltype(t.instance)> };
        requires(value_marker_check_v<T, true>);
    };

    // value type type trait, allows us to explicitly mark something a value type if required
    template<class T> struct ValueTypeTrait;

    template<template<class...> class T> struct GenValueTypeTrait;

    // automatically make anything that matches the VT requirements actually a VT
    template <il2cpp_value_type_requirements T> struct ValueTypeTrait<T> {
        constexpr static bool value = true;
    };

    template<class T> struct ValueDecompose {
        constexpr static bool value = false;
    };

    template<template<class...> class T, class... TArgs> struct ValueDecompose<T<TArgs...>> {
        constexpr static bool value = GenValueTypeTrait<T>::value;
    };

    template<class T>
    concept il2cpp_value_type = ValueDecompose<T>::value || ValueTypeTrait<T>::value;


    template<std::size_t sz>
    struct ValueTypeTrait<::bs_hook::ValueTypeWrapper<sz>> {
        constexpr static bool value = true;
    };

    template<std::size_t sz>
    struct ValueTypeTrait<::bs_hook::EnumTypeWrapper<sz>> {
        constexpr static bool value = true;
    };
#pragma endregion // val type

#pragma region ref type
    // https://godbolt.org/z/4vveWa46Y
    // Standard ref type concept
    // Note that this requires that type T is full instantiated
    // We want to ALSO support a case where that's not the case
    template <typename T>
    concept il2cpp_reference_type_requirements = requires(T const& t) {
      { t.convert() } -> convertible_to<void*>;

      // ensure these constructors exist
      requires std::is_constructible_v<T, void*>;
      requires std::is_constructible_v<T, std::nullptr_t>;
      // is the value type marker set, and set to false, or is it an
      // il2cppwrappertype
      requires(value_marker_check_v<T, false>);
    };

    // This type trait helps facilitate that.
    // We can partially specialize this with our type and we are good to go
    template <class T> struct RefTypeTrait;

    template<template<class...> class T> struct GenRefTypeTrait;

    template <il2cpp_reference_type_requirements T> struct RefTypeTrait<T> {
      constexpr static bool value = true;
    };

    template<class T> struct RefDecompose {
        constexpr static bool value = false;
    };

    template<template<class...> class T, class... TArgs> struct RefDecompose<T<TArgs...>> {
        constexpr static bool value = GenRefTypeTrait<T>::value;
    };

    // Now, as for our FULL, EXPOSED ref_type concept:
    // We FIRST check if we match RefTypeTrait<T>::value
    // Failing that, we check for standard ref_type-ness (but we do that
    // already) So:
    template <class T>
    concept il2cpp_reference_type = RefDecompose<T>::value || RefTypeTrait<T>::value;
#pragma endregion // ref type

    /// Macro to mark an incomplete type as being a ref type, also marks explicitly not a value type
    #define CORDL_REF_TYPE(...) \
        template<> struct ::cordl_internals::RefTypeTrait<__VA_ARGS__> { constexpr static bool value = true; }; \
        template<> struct ::cordl_internals::ValueTypeTrait<__VA_ARGS__> { constexpr static bool value = false; }

    /// Macro to mark an incomplete type as being a value type, also marks explicitly not a ref type
    #define CORDL_VAL_TYPE(...) \
        template<> struct ::cordl_internals::RefTypeTrait<__VA_ARGS__> { constexpr static bool value = false; }; \
        template<> struct ::cordl_internals::ValueTypeTrait<__VA_ARGS__> { constexpr static bool value = true; }

    /// Macro to mark an incomplete generic type as being a ref type, also marks explicitly not a value type
    #define CORDL_GEN_REF_TYPE(...) \
        template<> struct ::cordl_internals::GenRefTypeTrait<__VA_ARGS__> { constexpr static bool value = true; }; \
        template<> struct ::cordl_internals::GenValueTypeTrait<__VA_ARGS__> { constexpr static bool value = false; }

    /// Macro to mark an incomplete generic type as being a value type, also marks explicitly not a ref type
    #define CORDL_GEN_VAL_TYPE(...) \
        template<> struct ::cordl_internals::GenRefTypeTrait<__VA_ARGS__> { constexpr static bool value = false; }; \
        template<> struct ::cordl_internals::GenValueTypeTrait<__VA_ARGS__> { constexpr static bool value = true; }

    CORDL_REF_TYPE(::StringW);
    CORDL_REF_TYPE(::bs_hook::Il2CppWrapperType);

    // explicitly mark ListW as reftype
    CORDL_GEN_REF_TYPE(ArrayW);
    // explicitly mark ListW as reftype
    // TODO:
    // template<typename T, typename U>
    // struct ::cordl_internals::RefTypeTrait<ListW<T, U>> { constexpr static bool value = true; };
    // template<typename T, typename U>
    // struct ::cordl_internals::ValueTypeTrait<ListW<T, U>> { constexpr static bool value = false; };

    // Il2CppWrapperType type should match as ref type
    static_assert(il2cpp_reference_type<::bs_hook::Il2CppWrapperType>,
                  "Il2CppWrapperType did not match the il2cpp_reference_type "
                  "concept!");
    // StringW type should match as ref type
    static_assert(
        il2cpp_reference_type<::StringW>,
        "StringW did not match the il2cpp_reference_type concept!");

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

    #ifdef COMPILE_TIME_OFFSET_CHECKS
        #define OFFSET_CHECK(instance_size, offset, value_size, message) static_assert(offset_check_v<instance_size, offset, value_size>, message)
    #else
        #define OFFSET_CHECK(instance_size, offset, value_size, message)
    #endif
#pragma endregion // offset check
}
} // end anonymous namespace
