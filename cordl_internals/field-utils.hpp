#pragma once

#include "config.hpp"
#include "concepts.hpp"
#include "internal.hpp"
#include "exceptions.hpp"
#include "beatsaber-hook/shared/utils/il2cpp-utils-fields.hpp"

#include <bit>
#include <cstddef>

namespace UnityEngine {
  class Object;
}

namespace {
namespace cordl_internals {

  /// @brief method to find a field info in a klass
  /// @tparam name field name
  /// @tparam klass_resolver method to get the Il2CppClass* on which to get the klass
  template<internal::NTTPString name, auto klass_resolver>
  CORDL_HIDDEN FieldInfo* FindField() {
    static auto* klass = klass_resolver();
    if (!klass)
      throw NullException(std::string("Class for static field with name: ") +
                          name.data.data() + " is null!");
    static auto* field = ::il2cpp_utils::FindField(klass, name);
    if (!field)
      throw FieldException(std::string("Could not set static field with name: ") +
                          name.data.data());
    return field;
  }

#pragma region static field setters

  /// @brief template for setting a static field on a class
  /// @tparam T field type
  /// @tparam name field name
  /// @tparam klass_resolver method to get the Il2CppClass* on which the field resides
  template<typename T, internal::NTTPString name, auto klass_resolver>
  CORDL_HIDDEN void setStaticField(T&& v);

  /// @brief method to set a field that's a reference type
  template<::il2cpp_utils::il2cpp_reference_type T, internal::NTTPString name, auto klass_resolver>
  CORDL_HIDDEN void setStaticField(T&& v) {
    static auto* field = FindField<name, klass_resolver>();
    auto value = il2cpp_utils::il2cpp_reference_type_value<T>(std::forward<T>(v));
    ::il2cpp_functions::field_static_set_value(field, value);
  }

  /// @brief method to set a field that's a value type
  template<::il2cpp_utils::il2cpp_value_type T, internal::NTTPString name, auto klass_resolver>
  CORDL_HIDDEN void setStaticField(T&& v) {
    static auto* field = FindField<name, klass_resolver>();
    ::il2cpp_functions::field_static_set_value(field, static_cast<void*>(&v));
  }

  /// @brief method to set a field that's a trivial type
  template<typename T, internal::NTTPString name, auto klass_resolver>
  CORDL_HIDDEN void setStaticField(T&& v) {
    static auto* field = FindField<name, klass_resolver>();
    ::il2cpp_functions::field_static_set_value(
        field, const_cast<void*>(static_cast<void const*>(&v)));
  }

#pragma endregion // static field setters

#pragma region static field getters

  /// @brief template for getting a static field on a class
  /// @tparam T field type
  /// @tparam name field name
  /// @tparam klass_resolver method to get the Il2CppClass* on which the field resides
  template <typename T, internal::NTTPString name, auto klass_resolver>
  [[nodiscard]] CORDL_HIDDEN T getStaticField();

  /// @brief method to set a field that's a reference type
  template <::il2cpp_utils::il2cpp_reference_type T, internal::NTTPString name, auto klass_resolver>
  [[nodiscard]] CORDL_HIDDEN T getStaticField() {
    static auto* field = FindField<name, klass_resolver>();
    void* val{};
    ::il2cpp_functions::field_static_get_value(field, &val);

    if constexpr (il2cpp_utils::il2cpp_reference_type_pointer<T>) {
      return static_cast<T>(val);
    } else if constexpr (il2cpp_utils::il2cpp_reference_type_wrapper<T>) {
      return T(val);
    } else {
      return {};
    }
  }

  /// @brief method to set a field that's a trivial type
  template <typename T, internal::NTTPString name, auto klass_resolver>
  [[nodiscard]] CORDL_HIDDEN T getStaticField() {
    static auto* field = FindField<name, klass_resolver>();
    T val{};
    ::il2cpp_functions::field_static_get_value(field, static_cast<void*>(&val));
    return val;
  }
#pragma endregion // static field getters
}
} // end anonymous namespace
