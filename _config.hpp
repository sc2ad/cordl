#pragma once
#include <stdint.h>
#include <array>
#include <concepts>
#include <type_traits>
#include "beatsaber-hook/shared/utils/base-wrapper-type.hpp"
#include "beatsaber-hook/shared/utils/typedefs-string.hpp"
#include "beatsaber-hook/shared/utils/typedefs-array.hpp"
#include "beatsaber-hook/shared/utils/typedefs-list.hpp"
#include "beatsaber-hook/shared/utils/il2cpp-functions.hpp"
#include "beatsaber-hook/shared/utils/il2cpp-utils-fields.hpp"
#include "beatsaber-hook/shared/utils/il2cpp-utils-exceptions.hpp"

// always inline attribute
#define CORDL_ALWAYS_INLINE __attribute__((alwaysinline))
// hidden attribute
#define CORDL_HIDDEN __attribute__((visibility("hidden")))

#define CORDL_METHOD CORDL_HIDDEN CORDL_ALWAYS_INLINE
#define CORDL_TYPE CORDL_HIDDEN
#define CORDL_FIELD CORDL_HIDDEN
#define CORDL_PROP CORDL_HIDDEN

#define csnull ::cordl_internals::NullArg()

namespace bs_hook {
struct ValueTypeWrapper {
  constexpr static bool __CORDL_IS_VALUE_TYPE = true;

  constexpr ValueTypeWrapper() = default;
  ~ValueTypeWrapper() = default;

  constexpr ValueTypeWrapper(ValueTypeWrapper&&) = default;
  constexpr ValueTypeWrapper(ValueTypeWrapper const&) = default;

  constexpr ValueTypeWrapper& operator=(ValueTypeWrapper&&) = default;
  constexpr ValueTypeWrapper& operator=(ValueTypeWrapper const&) = default;
};
struct EnumTypeWrapper : public ValueTypeWrapper {
  constexpr static bool __CORDL_IS_VALUE_TYPE = true;

  constexpr EnumTypeWrapper() = default;
  ~EnumTypeWrapper() = default;

  constexpr EnumTypeWrapper(EnumTypeWrapper&&) = default;
  constexpr EnumTypeWrapper(EnumTypeWrapper const&) = default;

  constexpr EnumTypeWrapper& operator=(EnumTypeWrapper&&) = default;
  constexpr EnumTypeWrapper& operator=(EnumTypeWrapper const&) = default;
};
} // namespace bs_hook

namespace cordl_internals {
  namespace internal {
    template <std::size_t sz> struct NTTPString {
      constexpr NTTPString(char const (&n)[sz]) : data{} {
        std::copy_n(n, sz, data.begin());
      }
      std::array<char, sz> data;
    };
  } // namespace internal

  struct FieldException : public ::il2cpp_utils::exceptions::StackTraceException {
    using StackTraceException::StackTraceException;
  };
  struct NullException : public ::il2cpp_utils::exceptions::StackTraceException {
    using StackTraceException::StackTraceException;
  };

  template <typename T, typename U>
  concept convertible_to = std::is_convertible_v<T, U>;

  template<typename T>
  concept has_value_marker = requires {
    { T::__CORDL_IS_VALUE_TYPE } -> convertible_to<bool>;
  };

  template<typename T, bool check>
  struct value_marker_check {
    static constexpr bool value = false;
  };

  template<has_value_marker T, bool check>
  struct value_marker_check<T, check> {
    static constexpr bool value = T::__CORDL_IS_VALUE_TYPE == check;
  };

  template <typename T>
  concept il2cpp_value_type = requires(T const& t) {
    { std::is_array_v<decltype(t.__instance)> };
    value_marker_check<T, true>::value == true;
  };

  template <typename T>
  concept il2cpp_reference_type = requires(T const& t) {
    { t.convert() } -> convertible_to<void*>;

    // ensure these constructors exist
    requires std::is_constructible_v<T, void*>;
    requires std::is_constructible_v<T, std::nullptr_t>;
    // is the value type marker set, and set to false, or is it an il2cppwrappertype
    (value_marker_check<T, false>::value || std::is_same_v<std::remove_const_t<T>, ::bs_hook::Il2CppWrapperType>) == true;
  };

  static_assert(il2cpp_reference_type<::bs_hook::Il2CppWrapperType>, "Il2CppWrapperType did not match the il2cpp_reference_type concept!"); // wrappertype should match reference type always

  template <std::size_t sz>
  CORDL_HIDDEN constexpr void copyByByte(std::array<std::byte, sz> const& src,
                                        std::array<std::byte, sz>& dst) {
    for (auto i = 0; i < sz; i++) {
      dst[i] = src[i];
    }
  }

  template <std::size_t sz>
  CORDL_HIDDEN constexpr void copyByByte(void* src, void* dst) {
    for (auto i = 0; i < sz; i++) {
      reinterpret_cast<uint8_t*>(dst)[i] = reinterpret_cast<uint8_t*>(src)[i];
    }
  }

  template <std::size_t sz>
  CORDL_HIDDEN constexpr void moveByByte(std::array<std::byte, sz>&& src,
                                        std::array<std::byte, sz>& dst) {
    for (auto i = 0; i < sz; i++) {
      dst[i] = std::move(src[i]);
    }
  }

  template <std::size_t sz>
  CORDL_HIDDEN constexpr void moveByByte(void* src, void* dst) {
    for (auto i = 0; i < sz; i++) {
      reinterpret_cast<uint8_t*>(dst)[i] = std::move(reinterpret_cast<uint8_t*>(src)[i]);
    }
  }

  /// @brief gets an offset from a given pointer
  template <std::size_t offset>
  constexpr inline void** getAtOffset(void* instance) {
    return reinterpret_cast<void**>(static_cast<uint8_t*>(instance) + offset);
  }

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

// ensure bs-hook il2cpp wrapper type matches our expectations
// TODO: Do this when Il2CppWrapperType has __CORDL_IS_VALUE_TYPE == false
// static_assert(il2cpp_reference_type<::bs_hook::Il2CppWrapperType>);

struct InterfaceW : public ::bs_hook::Il2CppWrapperType {
  explicit constexpr InterfaceW(void* o) noexcept : ::bs_hook::Il2CppWrapperType(o) {}

  constexpr static bool __CORDL_IS_VALUE_TYPE = false;
};

template<typename T>
CORDL_HIDDEN ::bs_hook::Il2CppWrapperType Box(T&&);

template<typename T>
CORDL_HIDDEN ::bs_hook::Il2CppWrapperType Box(T);

template<typename T>
CORDL_HIDDEN ::bs_hook::Il2CppWrapperType Box(T*);

template<>
CORDL_HIDDEN constexpr ::bs_hook::Il2CppWrapperType Box<::bs_hook::Il2CppWrapperType>(::bs_hook::Il2CppWrapperType t) { return t; }

template<>
CORDL_HIDDEN constexpr ::bs_hook::Il2CppWrapperType Box<::bs_hook::Il2CppWrapperType&&>(::bs_hook::Il2CppWrapperType&& t) { return t; }

template<il2cpp_value_type T>
CORDL_HIDDEN ::bs_hook::Il2CppWrapperType Box(T&& t) {
    return ::bs_hook::Il2CppWrapperType(il2cpp_functions::value_box(classof(T), const_cast<void*>(static_cast<const void*>(t.__instance.data()))));
}

template<il2cpp_value_type T>
CORDL_HIDDEN ::bs_hook::Il2CppWrapperType Box(T t) {
    return ::bs_hook::Il2CppWrapperType(il2cpp_functions::value_box(classof(T), const_cast<void*>(static_cast<const void*>(t.__instance.data()))));
}

template<il2cpp_value_type T>
CORDL_HIDDEN ::bs_hook::Il2CppWrapperType Box(T* t) {
    return ::bs_hook::Il2CppWrapperType(il2cpp_functions::value_box(classof(T), const_cast<void*>(static_cast<const void*>(t->__instance.data()))));
}

// Type tag for passing null as a parameter without setting instance to null
struct NullArg {
  template <il2cpp_reference_type T> constexpr operator T() const noexcept {
    return T(nullptr);
  }
  constexpr operator ::bs_hook::Il2CppWrapperType() const noexcept {
    return ::bs_hook::Il2CppWrapperType(nullptr);
  }

  // convert to null anyways
  // this might cause issues when we have `Foo(il2cpp_reference_type)` and
  // `Foo(void*)`, hopefully not
  constexpr operator std::nullptr_t() const noexcept {
    return nullptr;
  }
  constexpr operator ::StringW() const noexcept {
    return StringW(nullptr);
  }

  template <typename T> constexpr operator ::ArrayW<T>() const noexcept {
    return ArrayW<T>(nullptr);
  }

  template <typename T, typename U>
  constexpr operator ::ListW<T, U>() const noexcept {
    return ListW<T, U>(nullptr);
  }
};

using intptr_t = int64_t*;
using uintptr_t = uint64_t*;

} // namespace cordl_internals
