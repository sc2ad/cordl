#pragma once

#include "config.hpp"
#include "concepts.hpp"
#include "internal.hpp"
#include "exceptions.hpp"
#include <bit>

namespace cordl_internals {
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
  template<typename T, std::size_t offset>
  CORDL_HIDDEN constexpr void setInstanceField(void*, T&&);

  template<typename T, std::size_t offset, std::size_t sz>
  CORDL_HIDDEN constexpr void setInstanceField(std::array<std::byte, sz>&, T&&);

  template<il2cpp_reference_type T, std::size_t offset>
  CORDL_HIDDEN void setInstanceField(void* instance, T&& v) {
    ::il2cpp_functions::Init();
    ::il2cpp_functions::gc_wbarrier_set_field(reinterpret_cast<Il2CppObject*>(instance), getAtOffset<offset>(instance), v.convert());
  }

  template<il2cpp_reference_type T, std::size_t offset, std::size_t sz>
  CORDL_HIDDEN void setInstanceField(std::array<std::byte, sz>& instance, T&& v) {
    // TODO: should assigning a ref type field on a value type instance also require wbarrier?
    static_assert(offset <= sz - sizeof(void*), "offset is too large for the size of the instance to be assigned comfortably!");
    std::copy_n(std::bit_cast<std::array<std::byte, sizeof(void*)>>(v.convert()), sizeof(void*), std::next(instance.begin(), offset));
  }

  template<il2cpp_value_type T, std::size_t offset>
  CORDL_HIDDEN constexpr void setInstanceField(void* instance, T&& v) {
    copyByByte<sizeof(v.__instance)>(
      const_cast<void*>(reinterpret_cast<const void*>(v.__instance.data())),
      reinterpret_cast<void*>(getAtOffset<offset>(instance))
    );
  }

  template<il2cpp_value_type T, std::size_t offset, std::size_t sz>
  CORDL_HIDDEN constexpr void setInstanceField(std::array<std::byte, sz>& instance, T&& v) {
    static_assert(offset <= sz - T::__CORDL_VALUE_TYPE_SIZE, "offset is too large for the size of the instance to be assigned comfortably!");
    std::copy_n(v.__instance.begin(), T::__CORDL_VALUE_TYPE_SIZE, std::next(instance, offset));
  }

  template<typename T, std::size_t offset>
  CORDL_HIDDEN constexpr void setInstanceField(void* instance, T&& v) {
    *reinterpret_cast<T*>(getAtOffset<offset>(instance)) = v;
  }

  template<typename T, std::size_t offset, std::size_t sz>
  CORDL_HIDDEN constexpr void setInstanceField(std::array<std::byte, sz>& instance, T&& v) {
    static_assert(offset <= sz - sizeof(T), "offset is too large for the size of the instance to be assigned comfortably!");
    std::copy_n(std::bit_cast<std::array<std::byte, sizeof(T)>>(v).begin(), sizeof(T), std::next(instance.begin(), offset));
  }

  template<typename T, internal::NTTPString name, auto klass_resolver>
  CORDL_HIDDEN void setStaticField(T&& v);

  template<il2cpp_reference_type T, internal::NTTPString name, auto klass_resolver>
  CORDL_HIDDEN void setStaticField(T&& v) {
    static auto* field = FindField<name, klass_resolver>();
    ::il2cpp_functions::field_static_set_value(field, static_cast<void*>(v.convert()));
  }

  template<il2cpp_value_type T, internal::NTTPString name, auto klass_resolver>
  CORDL_HIDDEN void setStaticField(T&& v) {
    static auto* field = FindField<name, klass_resolver>();
    ::il2cpp_functions::field_static_set_value(field, static_cast<void*>(v.__instance.data()));
  }

  template<typename T, internal::NTTPString name, auto klass_resolver>
  CORDL_HIDDEN void setStaticField(T&& v) {
    static auto* field = FindField<name, klass_resolver>();
    ::il2cpp_functions::field_static_set_value(field, static_cast<void*>(&v));
  }

#pragma endregion // field setters

#pragma region field getters
  template<typename T, std::size_t offset>
  [[nodiscard]] CORDL_HIDDEN T getInstanceField(const void* instance);

  template<typename T, std::size_t offset, std::size_t sz>
  [[nodiscard]] CORDL_HIDDEN constexpr T getInstanceField(const std::array<std::byte, sz>& instance);

  /// @brief gets a reference type field value @ offset
  template<il2cpp_reference_type T, std::size_t offset>
  [[nodiscard]] CORDL_HIDDEN T getInstanceField(const void* instance) {
    return T(*const_cast<void**>(getAtOffset<offset>(instance)));
  }

  template<il2cpp_reference_type T, std::size_t offset, std::size_t sz>
  [[nodiscard]] CORDL_HIDDEN constexpr T getInstanceField(const std::array<std::byte, sz>& instance) {
    return T(*const_cast<void**>(static_cast<const void**>(static_cast<const void*>(&std::next(instance.begin() + offset)))));
  }

  /// @brief gets a value type field value @ offset
  template<il2cpp_value_type T, std::size_t offset>
  [[nodiscard]] CORDL_HIDDEN T getInstanceField(const void* instance) {
    std::array<std::byte, T::__CORDL_VALUE_TYPE_SIZE> data;
    std::memcpy(data.data(), getAtOffset<offset>(instance), T::__CORDL_VALUE_TYPE_SIZE);
    return T(data);
  }

  template<il2cpp_value_type T, std::size_t offset, std::size_t sz>
  [[nodiscard]] CORDL_HIDDEN constexpr T getInstanceField(const std::array<std::byte, sz>& instance) {
    static_assert(offset <= sz - T::__CORDL_VALUE_TYPE_SIZE, "offset is too large for the size of the instance to be assigned comfortably!");
    std::array<std::byte, T::__CORDL_VALUE_TYPE_SIZE> data;
    std::copy_n(std::next(instance.begin(), offset), T::__CORDL_VALUE_TYPE_SIZE, data.begin());
    return T(data);
  }

  /// @brief gets an arbitrary field value @ offset
  template<typename T, std::size_t offset>
  [[nodiscard]] CORDL_HIDDEN T getInstanceField(const void* instance) {
    std::array<std::byte, sizeof(T)> data;
    std::memcpy(data.data(), getAtOffset<offset>(instance), sizeof(T));
    return std::bit_cast<T>(data);
  }

  /// @brief gets an arbitrary field value @ offset
  template<typename T, std::size_t offset, std::size_t sz>
  [[nodiscard]] CORDL_HIDDEN constexpr T getInstanceField(const std::array<std::byte, sz>& instance) {
    static_assert(offset <= sz - sizeof(T), "offset is too large for the size of the instance to be assigned comfortably!");
    std::array<std::byte, sizeof(T)> arr;
    std::copy_n(std::next(instance.begin(), offset), sizeof(T), arr.begin());
    return std::bit_cast<T>(arr);
  }

  template <typename T, internal::NTTPString name, auto klass_resolver>
  [[nodiscard]] CORDL_HIDDEN T getStaticField();

  /// @brief gets a reference type static field with name from klass
  template <il2cpp_reference_type T, internal::NTTPString name, auto klass_resolver>
  [[nodiscard]] CORDL_HIDDEN T getStaticField() {
    static auto* field = FindField<name, klass_resolver>();
    void* val{};
    ::il2cpp_functions::field_static_get_value(field, &val);
    return T(val);
  }

  /// @brief gets a reference type static field with name from klass
  template <il2cpp_value_type T, internal::NTTPString name, auto klass_resolver>
  [[nodiscard]] CORDL_HIDDEN T getStaticField() {
    static auto* field = FindField<name, klass_resolver>();
    std::array<std::byte, T::__CORDL_VALUE_TYPE_SIZE> data;
    ::il2cpp_functions::field_static_get_value(field, static_cast<void*>(data.data()));
    return T(val);
  }

  /// @brief gets a reference type static field with name from klass
  template <typename T, internal::NTTPString name, auto klass_resolver>
  [[nodiscard]] CORDL_HIDDEN T getStaticField() {
    static auto* field = FindField<name, klass_resolver>();
    T val{};
    ::il2cpp_functions::field_static_get_value(field, static_cast<void*>(&val));
    return val;
  }
#pragma endregion // field getters
}
