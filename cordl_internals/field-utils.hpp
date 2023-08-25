#pragma once

#include "config.hpp"
#include "concepts.hpp"
#include "internal.hpp"
#include "exceptions.hpp"

namespace cordl_internals {
#pragma region field setters
  template<typename T, std::size_t offset>
  CORDL_HIDDEN constexpr void setInstanceField(void*, T&&);

  template<il2cpp_reference_type T, std::size_t offset>
  CORDL_HIDDEN void setInstanceField(void* instance, T&& v) {
    ::il2cpp_functions::Init();
    ::il2cpp_functions::gc_wbarrier_set_field(reinterpret_cast<Il2CppObject*>(instance), getAtOffset<offset>(instance), v.convert());
  }

  template<il2cpp_value_type T, std::size_t offset>
  CORDL_HIDDEN constexpr void setInstanceField(void* instance, T&& v) {
    copyByByte<sizeof(v.__instance)>(
      const_cast<void*>(reinterpret_cast<const void*>(v.__instance.data())),
      reinterpret_cast<void*>(getAtOffset<offset>(instance))
    );
  }

  template<typename T, std::size_t offset>
  CORDL_HIDDEN constexpr void setInstanceField(void* instance, T&& v) {
    *reinterpret_cast<T*>(getAtOffset<offset>(instance)) = v;
  }

  template<typename T, internal::NTTPString name, auto klass_resolver>
  CORDL_HIDDEN void setStaticField(T&& v);

  template<il2cpp_reference_type T, internal::NTTPString name, auto klass_resolver>
  CORDL_HIDDEN void setStaticField(T&& v) {
    auto klass = klass_resolver();
    if (!klass)
      throw NullException(std::string("Class for static field with name: ") +
                          name.data.data() + " is null!");
    auto val = ::il2cpp_utils::SetFieldValue(klass, name.data.data(), v.convert());
    if (!val)
      throw FieldException(std::string("Could not set static field with name: ") +
                          name.data.data());
  }

  template<il2cpp_value_type T, internal::NTTPString name, auto klass_resolver>
  CORDL_HIDDEN void setStaticField(T&& v) {
    auto klass = klass_resolver();
    if (!klass)
      throw NullException(std::string("Class for static field with name: ") +
                          name.data.data() + " is null!");
    auto val = ::il2cpp_utils::SetFieldValue(klass, name.data.data(), v.__instance);
    if (!val)
      throw FieldException(std::string("Could not set static field with name: ") +
                          name.data.data());
  }

  template<typename T, internal::NTTPString name, auto klass_resolver>
  CORDL_HIDDEN void setStaticField(T&& v) {
    auto klass = klass_resolver();
    if (!klass)
      throw NullException(std::string("Class for static field with name: ") +
                          name.data.data() + " is null!");
    auto val = ::il2cpp_utils::SetFieldValue(klass, name.data.data(), v);
    if (!val)
      throw FieldException(std::string("Could not set static field with name: ") +
                          name.data.data());
  }

#pragma endregion // field setters

#pragma region field getters
  template<typename T, std::size_t offset>
  [[nodiscard]] CORDL_HIDDEN constexpr T getInstanceField(void* instance);

  /// @brief gets a reference type field value @ offset
  template<il2cpp_reference_type T, std::size_t offset>
  [[nodiscard]] CORDL_HIDDEN constexpr T getInstanceField(void* instance) {
    return T(*reinterpret_cast<void**>(getAtOffset<offset>(instance)));
  }

  /// @brief gets a value type field value @ offset
  template<il2cpp_value_type T, std::size_t offset>
  [[nodiscard]] CORDL_HIDDEN constexpr T getInstanceField(void* instance) {
    T v{};
    copyByByte<sizeof(v.__instance)>(
      reinterpret_cast<void*>(getAtOffset<offset>(instance)),
      v.__instance.data()
    );
    return std::move(v);
  }

  /// @brief gets an arbitrary field value @ offset
  template<typename T, std::size_t offset>
  [[nodiscard]] CORDL_HIDDEN constexpr T getInstanceField(void* instance) {
    T v{};
    copyByByte<sizeof(v)>(
      reinterpret_cast<void*>(getAtOffset<offset>(instance)),
      &v
    );
    return std::move(v);
  }

  template <typename T, internal::NTTPString name, auto klass_resolver>
  [[nodiscard]] CORDL_HIDDEN T getStaticField();

  /// @brief gets a reference type static field with name from klass
  template <il2cpp_reference_type T, internal::NTTPString name, auto klass_resolver>
  [[nodiscard]] CORDL_HIDDEN T getStaticField() {
    auto klass = klass_resolver();
    if (!klass)
      throw NullException(std::string("Class for static field with name: ") +
                          name.data.data() + " is null!");
    auto val = ::il2cpp_utils::GetFieldValue<Il2CppObject*>(klass, name.data.data());
    if (!val)
      throw FieldException(std::string("Could not get static field with name: ") +
                          name.data.data());
    return T(reinterpret_cast<void*>(*val));
  }

  /// @brief gets a reference type static field with name from klass
  template <il2cpp_value_type T, internal::NTTPString name, auto klass_resolver>
  [[nodiscard]] CORDL_HIDDEN T getStaticField() {
    auto klass = klass_resolver();
    if (!klass)
      throw NullException(std::string("Class for static field with name: ") +
                          name.data.data() + " is null!");
    auto val = ::il2cpp_utils::GetFieldValue<T>(klass, name.data.data());
    if (!val)
      throw FieldException(std::string("Could not get static field with name: ") +
                          name.data.data());
    return *val;
  }

  /// @brief gets a reference type static field with name from klass
  template <typename T, internal::NTTPString name, auto klass_resolver>
  [[nodiscard]] CORDL_HIDDEN T getStaticField() {
    auto klass = klass_resolver();
    if (!klass)
      throw NullException(std::string("Class for static field with name: ") +
                          name.data.data() + " is null!");
    auto val = ::il2cpp_utils::GetFieldValue<T>(klass, name.data.data());
    if (!val)
      throw FieldException(std::string("Could not get static field with name: ") +
                          name.data.data());
    return *val;
  }
#pragma endregion // field getters
}
