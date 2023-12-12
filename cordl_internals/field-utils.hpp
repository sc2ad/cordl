#pragma once

#include "config.hpp"
#include "concepts.hpp"
#include "internal.hpp"
#include "exceptions.hpp"
#include "beatsaber-hook/shared/utils/il2cpp-utils-fields.hpp"
#include "beatsaber-hook/shared/utils/value-wrapper-type.hpp"
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
#pragma region field setters
#pragma region ref instance field setters
  /// @brief template for field setter method on ref types
  /// @tparam T field type
  /// @tparam offset field offset
  /// @tparam InstT instance type
  template<typename T, std::size_t offset, ::il2cpp_utils::il2cpp_reference_type_pointer InstT>
  CORDL_HIDDEN void setInstanceField(InstT, T&&);

  /// @brief set reference type value @ offset on instance
  template<::il2cpp_utils::il2cpp_reference_type T, std::size_t offset, ::il2cpp_utils::il2cpp_reference_type_pointer InstT>
  CORDL_HIDDEN void setInstanceField(InstT instance, T&& v) {
    OFFSET_CHECK(il2cpp_instance_sizeof(InstT), offset, sizeof(void*), "offset is too large for the size of the instance to be assigned correctly!");
    FIELD_NULL_CHECK(instance);

    auto value = il2cpp_utils::il2cpp_reference_type_value<T>(std::forward<T>(v));
    ::il2cpp_functions::Init();
    ::il2cpp_functions::gc_wbarrier_set_field(instance, getAtOffset<offset>(instance), value);
  }

  /// @brief set value type value @ offset on instance
  template<::il2cpp_utils::il2cpp_value_type T, std::size_t offset, ::il2cpp_utils::il2cpp_reference_type_pointer InstT>
  CORDL_HIDDEN void setInstanceField(InstT instance, T&& v) {
    OFFSET_CHECK(il2cpp_instance_sizeof(InstT), offset, il2cpp_instance_sizeof(T), "offset is too large for the size of the instance to be assigned correctly!");
    FIELD_NULL_CHECK(instance);

    std::memcpy(getAtOffset<offset>(instance), v.convert(), il2cpp_instance_sizeof(T));
  }

  /// @brief set trivial value @ offset on instance
  template<typename T, std::size_t offset, ::il2cpp_utils::il2cpp_reference_type_pointer InstT>
  CORDL_HIDDEN void setInstanceField(InstT instance, T&& v) {
    OFFSET_CHECK(il2cpp_instance_sizeof(InstT), offset, sizeof(T), "offset is too large for the size of the instance to be assigned correctly!");
    FIELD_NULL_CHECK(instance);

    std::memcpy(getAtOffset<offset>(instance), &v, sizeof(T));
  }

#pragma endregion // ref instance field setters
#pragma region val instance field setters

  /// @brief template for field setter method on value types
  /// @tparam T field type
  /// @tparam offset field offset
  template<typename T, std::size_t offset, std::size_t sz>
  CORDL_HIDDEN constexpr void setInstanceField(std::array<std::byte, sz>&, T&&);

  /// @brief set reference type value @ offset on instance of size sz
  template<::il2cpp_utils::il2cpp_reference_type T, std::size_t offset, std::size_t sz>
  CORDL_HIDDEN void setInstanceField(std::array<std::byte, sz>& instance, T&& v) {
    // TODO: should assigning a ref type field on a value type instance also require wbarrier?
    OFFSET_CHECK(sz, offset, sizeof(void*), "offset is too large for the size of the instance to be assigned correctly!");
    auto value = il2cpp_utils::il2cpp_reference_type_value<T>(std::forward<T>(v));

    std::copy_n(
      std::bit_cast<std::array<std::byte, sizeof(void*)>>(value).begin(),
      sizeof(void*),
      std::next(instance.begin(), offset)
    );
  }

  /// @brief set value type value @ offset on instance of size sz
  template<::il2cpp_utils::il2cpp_value_type T, std::size_t offset, std::size_t sz>
  CORDL_HIDDEN constexpr void setInstanceField(std::array<std::byte, sz>& instance, T&& v) {
    OFFSET_CHECK(sz, offset, il2cpp_instance_sizeof(T), "offset is too large for the size of the instance to be assigned correctly!");
    SIZE_CHECK(T, "wrapper size was different from the type it wraps!");

    std::copy_n(
      v.::bs_hook::ValueTypeWrapper<il2cpp_instance_sizeof(T)>::instance.begin(),
      il2cpp_instance_sizeof(T),
      std::next(instance.begin(), offset)
    );
  }

  /// @brief set trivial value @ offset on instance of size sz
  template<typename T, std::size_t offset, std::size_t sz>
  CORDL_HIDDEN constexpr void setInstanceField(std::array<std::byte, sz>& instance, T&& v) {
    OFFSET_CHECK(sz, offset, sizeof(T), "offset is too large for the size of the instance to be assigned correctly!");

    std::copy_n(
      std::bit_cast<std::array<std::byte, sizeof(T)>>(v).begin(),
      sizeof(T),
      std::next(instance.begin(), offset)
    );
  }

#pragma endregion // ref instance field setters
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
    ::il2cpp_functions::field_static_set_value(field, v.convert());
  }

  /// @brief method to set a field that's a trivial type
  template<typename T, internal::NTTPString name, auto klass_resolver>
  CORDL_HIDDEN void setStaticField(T&& v) {
    static auto* field = FindField<name, klass_resolver>();
    ::il2cpp_functions::field_static_set_value(
        field, const_cast<void*>(static_cast<void const*>(&v)));
  }

#pragma endregion // static field setters
#pragma endregion // field setters

#pragma region field getters

#pragma region ref instance field getters
  /// @brief template for field getter method on ref types
  /// @tparam T field type
  /// @tparam offset field offset
  template <typename T, std::size_t offset, ::il2cpp_utils::il2cpp_reference_type_pointer InstT>
  requires(std::is_const_v<std::remove_pointer_t<InstT>>)
  [[nodiscard]] CORDL_HIDDEN T const& getInstanceField(InstT instance) {
    OFFSET_CHECK(sizeof(std::remove_pointer_t<InstT>), offset, sizeof(T), "offset is too large for the size of the instance to be retreived correctly!");
    FIELD_NULL_CHECK(instance);

    return *static_cast<T const*>(static_cast<void const*>(getAtOffset<offset>(instance)));
  }

  /// @brief template for field getter method on ref types
  /// @tparam T field type
  /// @tparam offset field offset
  template <typename T, std::size_t offset, ::il2cpp_utils::il2cpp_reference_type_pointer InstT>
  requires(!std::is_const_v<std::remove_pointer_t<InstT>>)
  [[nodiscard]] CORDL_HIDDEN T& getInstanceField(InstT instance) {
    OFFSET_CHECK(sizeof(std::remove_pointer_t<InstT>), offset, sizeof(T), "offset is too large for the size of the instance to be retreived correctly!");
    FIELD_NULL_CHECK(instance);

    return *static_cast<T*>(static_cast<void*>(getAtOffset<offset>(instance)));
  }

#pragma endregion // ref instance field getters
#pragma region val instance field getters
  /// @brief template for field getter method on value types
  /// @tparam T field type
  /// @tparam offset field offset
  /// @tparam sz wrapper array size
  template <typename T, std::size_t offset, std::size_t sz>
  [[nodiscard]] CORDL_HIDDEN constexpr T const& getInstanceField(std::array<std::byte, sz> const& instance);

  /// @brief get reference type value @ offset on instance of size sz
  template <::il2cpp_utils::il2cpp_reference_type T, std::size_t offset, std::size_t sz>
  [[nodiscard]] CORDL_HIDDEN constexpr T const& getInstanceField(std::array<std::byte, sz> const& instance) {
    OFFSET_CHECK(sz, offset, sizeof(void*), "offset is too large for the size of the instance to be retreived correctly!");

    return *static_cast<T const*>(static_cast<void const*>(instance.data() + offset));
  }

  /// @brief get value type value @ offset on instance of size sz
  template <::il2cpp_utils::il2cpp_value_type T, std::size_t offset, std::size_t sz>
  [[nodiscard]] CORDL_HIDDEN constexpr T const& getInstanceField(std::array<std::byte, sz> const& instance) {
    OFFSET_CHECK(sz, offset, il2cpp_instance_sizeof(T), "offset is too large for the size of the instance to be retreived correctly!");
    SIZE_CHECK(T, "wrapper size was different from the type it wraps!");

    return *static_cast<T const*>(static_cast<void const*>(instance.data() + offset));
  }

  /// @brief get trivial type value @ offset on instance of size sz
  template <typename T, std::size_t offset, std::size_t sz>
  [[nodiscard]] CORDL_HIDDEN constexpr T const& getInstanceField(std::array<std::byte, sz> const& instance) {
    OFFSET_CHECK(sz, offset, sizeof(T), "offset is too large for the size of the instance to be retreived correctly!");

    return *static_cast<T const*>(static_cast<void const*>(instance.data() + offset));
  }

  /// @brief template for field getter method on value types
  /// @tparam T field type
  /// @tparam offset field offset
  /// @tparam sz wrapper array size
  template <typename T, std::size_t offset, std::size_t sz>
  [[nodiscard]] CORDL_HIDDEN constexpr T& getInstanceField(std::array<std::byte, sz>& instance);

  /// @brief get reference type value @ offset on instance of size sz
  template <::il2cpp_utils::il2cpp_reference_type T, std::size_t offset, std::size_t sz>
  [[nodiscard]] CORDL_HIDDEN constexpr T& getInstanceField(std::array<std::byte, sz>& instance) {
    OFFSET_CHECK(sz, offset, sizeof(void*), "offset is too large for the size of the instance to be retreived correctly!");

    return *static_cast<T*>(static_cast<void*>(instance.data() + offset));
  }

  /// @brief get value type value @ offset on instance of size sz
  template <::il2cpp_utils::il2cpp_value_type T, std::size_t offset, std::size_t sz>
  [[nodiscard]] CORDL_HIDDEN constexpr T& getInstanceField(std::array<std::byte, sz>& instance) {
    OFFSET_CHECK(sz, offset, il2cpp_instance_sizeof(T), "offset is too large for the size of the instance to be retreived correctly!");
    SIZE_CHECK(T, "wrapper size was different from the type it wraps!");

    return *static_cast<T*>(static_cast<void*>(instance.data() + offset));
  }

  /// @brief get trivial type value @ offset on instance of size sz
  template <typename T, std::size_t offset, std::size_t sz>
  [[nodiscard]] CORDL_HIDDEN constexpr T& getInstanceField(std::array<std::byte, sz>& instance) {
    OFFSET_CHECK(sz, offset, sizeof(T), "offset is too large for the size of the instance to be retreived correctly!");

    return *static_cast<T*>(static_cast<void*>(instance.data() + offset));
  }

#pragma endregion // val instance field getters
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
      static_assert(false, "This code branch should never be compiled, something went wrong!");
    }
  }

  /// @brief method to set a field that's a value type
  template <::il2cpp_utils::il2cpp_value_type T, internal::NTTPString name, auto klass_resolver>
  [[nodiscard]] CORDL_HIDDEN T getStaticField() {
    static auto* field = FindField<name, klass_resolver>();
    std::array<std::byte, il2cpp_instance_sizeof(T)> data;
    ::il2cpp_functions::field_static_get_value(field, static_cast<void*>(data.data()));
    return T(data);
  }

  /// @brief method to set a field that's a trivial type
  template <typename T, internal::NTTPString name, auto klass_resolver>
  [[nodiscard]] CORDL_HIDDEN T getStaticField() {
    static auto* field = FindField<name, klass_resolver>();
    T val{};
    ::il2cpp_functions::field_static_get_value(field, static_cast<void*>(&val));
    return val;
  }
#pragma endregion static field getters
#pragma endregion // field getters
}
} // end anonymous namespace
