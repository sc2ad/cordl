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

  /// @brief reads the cachedptr on the given unity object instance
  template<::cordl_internals::cordl_ref_type T>
  requires(std::is_convertible_v<T, UnityEngine::Object*>)
  CORDL_HIDDEN inline constexpr void* read_cachedptr(T instance) {
    return *static_cast<void**>(getAtOffset<0x10>(instance));
  }

  /// @brief checks for instance being null or null equivalent
  template<::cordl_internals::cordl_ref_type T>
  requires(std::is_convertible_v<T, UnityEngine::Object*>)
  inline bool check_null(T instance) {
    return instance && read_cachedptr(instance);
  }

  /// @brief checks for instance being null
  template<::cordl_internals::cordl_ref_type T>
  requires(!std::is_convertible_v<T, UnityEngine::Object*>)
  inline bool check_null(T instance) {
    return instance;
  }

  // if you compile with the define RUNTIME_FIELD_NULL_CHECKS at runtime every field access will be null checked for you, and a c++ exception will be thrown if the instance is null.
  // in case of a unity object, the m_CachedPtr is also checked. Since this can incur some overhead you can also just not define RUNTIME_FIELD_NULL_CHECKS to save performance
  #ifdef RUNTIME_FIELD_NULL_CHECKS
    #define NULL_CHECK(instance) if (!::cordl_internals::check_null(instance)) throw ::cordl_internals::NullException(std::string("Field access on nullptr instance, please make sure your instance is not null"))
  #else
    #define NULL_CHECK(instance)
  #endif

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
  /// @brief template for field setter method on ref types
  /// @tparam T field type
  /// @tparam offset field offset
  /// @tparam InstT instance type
  template<typename T, std::size_t offset, ::cordl_internals::cordl_ref_type InstT>
  CORDL_HIDDEN void setInstanceField(InstT, T&&);

  /// @brief template for field setter method on value types
  /// @tparam T field type
  /// @tparam offset field offset
  template<typename T, std::size_t offset, std::size_t sz>
  CORDL_HIDDEN constexpr void setInstanceField(std::array<std::byte, sz>&, T&&);

  /// @brief set reference type value @ offset on instance
  template<::cordl_internals::cordl_ref_type T, std::size_t offset, ::cordl_internals::cordl_ref_type InstT>
  CORDL_HIDDEN void setInstanceField(InstT instance, T&& v) {
    OFFSET_CHECK(sizeof(std::remove_pointer_t<InstT>), offset, sizeof(void*), "offset is too large for the size of the instance to be assigned correctly!");
    NULL_CHECK(instance);

    ::il2cpp_functions::Init();
    ::il2cpp_functions::gc_wbarrier_set_field(instance, getAtOffset<offset>(instance), v);
  }

  /// @brief set reference type value @ offset on instance of size sz
  template<::cordl_internals::cordl_ref_type T, std::size_t offset, std::size_t sz>
  CORDL_HIDDEN void setInstanceField(std::array<std::byte, sz>& instance, T&& v) {
    // TODO: should assigning a ref type field on a value type instance also require wbarrier?
    OFFSET_CHECK(sz, offset, sizeof(void*), "offset is too large for the size of the instance to be assigned correctly!");

    std::copy_n(std::bit_cast<std::array<std::byte, sizeof(void*)>>(v).begin(), sizeof(void*), std::next(instance.begin(), offset));
  }

  /// @brief set value type value @ offset on instance
  template<::il2cpp_utils::il2cpp_value_type T, std::size_t offset, ::cordl_internals::cordl_ref_type InstT>
  CORDL_HIDDEN void setInstanceField(InstT instance, T&& v) {
    OFFSET_CHECK(sizeof(std::remove_pointer_t<InstT>), offset, il2cpp_instance_sizeof(T), "offset is too large for the size of the instance to be assigned correctly!");
    NULL_CHECK(instance);

    std::memcpy(getAtOffset<offset>(instance), v.convert(), il2cpp_instance_sizeof(T));
  }

  /// @brief set value type value @ offset on instance of size sz
  template<::il2cpp_utils::il2cpp_value_type T, std::size_t offset, std::size_t sz>
  CORDL_HIDDEN constexpr void setInstanceField(std::array<std::byte, sz>& instance, T&& v) {
    OFFSET_CHECK(sz, offset, il2cpp_instance_sizeof(T), "offset is too large for the size of the instance to be assigned correctly!");
    SIZE_CHECK(T, "wrapper size was different from the type it wraps!");

    std::copy_n(v.::bs_hook::ValueTypeWrapper<il2cpp_instance_sizeof(T)>::instance.begin(), il2cpp_instance_sizeof(T), std::next(instance.begin(), offset));
  }

  /// @brief set trivial value @ offset on instance
  template<typename T, std::size_t offset, ::cordl_internals::cordl_ref_type InstT>
  CORDL_HIDDEN void setInstanceField(InstT instance, T&& v) {
    OFFSET_CHECK(il2cpp_instance_sizeof(InstT), offset, sizeof(T), "offset is too large for the size of the instance to be assigned correctly!");
    NULL_CHECK(instance);

    std::memcpy(getAtOffset<offset>(instance), &v, sizeof(T));
  }

  /// @brief set trivial value @ offset on instance of size sz
  template<typename T, std::size_t offset, std::size_t sz>
  CORDL_HIDDEN constexpr void setInstanceField(std::array<std::byte, sz>& instance, T&& v) {
    OFFSET_CHECK(sz, offset, sizeof(T), "offset is too large for the size of the instance to be assigned correctly!");

    std::copy_n(std::bit_cast<std::array<std::byte, sizeof(T)>>(v).begin(), sizeof(T), std::next(instance.begin(), offset));
  }

  /// @brief template for setting a static field on a class
  /// @tparam T field type
  /// @tparam name field name
  /// @tparam klass_resolver method to get the Il2CppClass* on which the field resides
  template<typename T, internal::NTTPString name, auto klass_resolver>
  CORDL_HIDDEN void setStaticField(T&& v);

  /// @brief method to set a field that's a reference type
  template<::cordl_internals::cordl_ref_type T, internal::NTTPString name, auto klass_resolver>
  CORDL_HIDDEN void setStaticField(T&& v) {
    static auto* field = FindField<name, klass_resolver>();
    ::il2cpp_functions::field_static_set_value(field, v);
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

#pragma endregion // field setters

#pragma region field getters
  /// @brief template for field getter method on ref types
  /// @tparam T field type
  /// @tparam offset field offset
  template <typename T, std::size_t offset,
            ::cordl_internals::cordl_ref_type InstT>
  [[nodiscard]] CORDL_HIDDEN T const& getInstanceField(InstT const& instance);

  /// @brief template for field getter method on ref types
  /// @tparam T field type
  /// @tparam offset field offset
  template <typename T, std::size_t offset,
            ::cordl_internals::cordl_ref_type InstT>
  [[nodiscard]] CORDL_HIDDEN T& getInstanceField(InstT& instance);

  /// @brief template for field getter method on value types
  /// @tparam T field type
  /// @tparam offset field offset
  /// @tparam sz wrapper array size
  template <typename T, std::size_t offset, std::size_t sz>
  [[nodiscard]] CORDL_HIDDEN constexpr T const&
  getInstanceField(std::array<std::byte, sz> const& instance);
  
  /// @brief template for field getter method on value types
  /// @tparam T field type
  /// @tparam offset field offset
  /// @tparam sz wrapper array size
  template <typename T, std::size_t offset, std::size_t sz>
  [[nodiscard]] CORDL_HIDDEN constexpr T& getInstanceField(std::array<std::byte, sz>& instance);

  /// @brief get reference type value @ offset on instance
  template <::cordl_internals::cordl_ref_type T, std::size_t offset, ::cordl_internals::cordl_ref_type InstT>
  [[nodiscard]] CORDL_HIDDEN T getInstanceField(InstT const& instance) {
    OFFSET_CHECK(sizeof(std::remove_pointer_t<InstT>), offset, sizeof(void*), "offset is too large for the size of the instance to be retreived correctly!");
    NULL_CHECK(instance);

    return *static_cast<T*>(const_cast<void*>(
        static_cast<void const*>(getAtOffset<offset>(instance))));
  }

  /// @brief get reference type value @ offset on instance of size sz
  template <::cordl_internals::cordl_ref_type T, std::size_t offset, std::size_t sz>
  [[nodiscard]] CORDL_HIDDEN constexpr T getInstanceField(std::array<std::byte, sz> const &instance) {
    OFFSET_CHECK(sz, offset, sizeof(void*), "offset is too large for the size of the instance to be retreived correctly!");

    return *static_cast<T**>(static_cast<void*>(const_cast<std::byte*>(instance.data() + offset)));
  }

  /// @brief get value type value @ offset on instance
  template <::il2cpp_utils::il2cpp_value_type T, std::size_t offset, ::cordl_internals::cordl_ref_type InstT>
  [[nodiscard]] CORDL_HIDDEN T& getInstanceField(InstT const& instance) {
    OFFSET_CHECK(sizeof(std::remove_pointer_t<InstT>), offset, il2cpp_instance_sizeof(T), "offset is too large for the size of the instance to be retreived correctly!");
    SIZE_CHECK(T, "wrapper size was different from the type it wraps!");
    NULL_CHECK(instance);

    return *static_cast<T*>(const_cast<void*>(
        static_cast<void const*>(getAtOffset<offset>(instance))));
  }

  /// @brief get value type value @ offset on instance of size sz
  template <::il2cpp_utils::il2cpp_value_type T, std::size_t offset, std::size_t sz>
  [[nodiscard]] CORDL_HIDDEN constexpr T& getInstanceField(std::array<std::byte, sz> const& instance) {
    OFFSET_CHECK(sz, offset, il2cpp_instance_sizeof(T), "offset is too large for the size of the instance to be retreived correctly!");
    SIZE_CHECK(T, "wrapper size was different from the type it wraps!");

    return *const_cast<T*>(static_cast<T const*>(
        static_cast<void const*>(instance.data() + offset)));
  }

  /// @brief get trivial type value @ offset on instance
  template <typename T, std::size_t offset, ::cordl_internals::cordl_ref_type InstT>
  [[nodiscard]] CORDL_HIDDEN T& getInstanceField(InstT const& instance) {
    OFFSET_CHECK(sizeof(std::remove_pointer_t<InstT>), offset, sizeof(T), "offset is too large for the size of the instance to be retreived correctly!");
    NULL_CHECK(instance);

    return *static_cast<T*>(const_cast<void*>(
        static_cast<void const*>(getAtOffset<offset>(instance))));
  }

  /// @brief get trivial type value @ offset on instance of size sz
  template <typename T, std::size_t offset, std::size_t sz>
  [[nodiscard]] CORDL_HIDDEN constexpr T& getInstanceField(std::array<std::byte, sz> const& instance) {
    OFFSET_CHECK(sz, offset, sizeof(T), "offset is too large for the size of the instance to be retreived correctly!");

    return *const_cast<T*>(static_cast<T const*>(
        static_cast<void const*>(instance.data() + offset)));
  }

  /// @brief template for getting a static field on a class
  /// @tparam T field type
  /// @tparam name field name
  /// @tparam klass_resolver method to get the Il2CppClass* on which the field resides
  template <typename T, internal::NTTPString name, auto klass_resolver>
  [[nodiscard]] CORDL_HIDDEN T getStaticField();

  /// @brief method to set a field that's a reference type
  template <::cordl_internals::cordl_ref_type T, internal::NTTPString name, auto klass_resolver>
  [[nodiscard]] CORDL_HIDDEN T getStaticField() {
    static auto* field = FindField<name, klass_resolver>();
    void* val{};
    ::il2cpp_functions::field_static_get_value(field, &val);
    return static_cast<T>(val);
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
#pragma endregion // field getters
}
} // end anonymous namespace
