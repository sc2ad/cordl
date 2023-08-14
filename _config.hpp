#pragma once
#include <stdint.h>
#include <array>

// always inline attribute
#define CORDL_ALWAYS_INLINE __attribute__((alwaysinline))
// hidden attribute
#define CORDL_HIDDEN __attribute__((hidden))

#define CORDL_METHOD CORDL_HIDDEN CORDL_ALWAYS_INLINE
#define CORDL_TYPE CORDL_HIDDEN
#define CORDL_FIELD CORDL_HIDDEN
#define CORDL_PROP CORDL_HIDDEN

namespace cordl_internals {
namespace internal {
template <std::size_t sz> struct NTTPString {
  constexpr NTTPString(char const (&n)[sz]) : data{} {
    std::copy_n(n, sz, data.begin());
  }
  std::array<char, sz> data;
};
} // namespace internal

/// @brief gets an offset from a given pointer
template <std::size_t offset>
constexpr inline uint8_t* getAtOffset(void* instance) {
  return reinterpret_cast<uint8_t*>(instance) + offset;
}

template <typename T, std::size_t offset>
CORDL_HIDDEN inline T getReferenceTypeInstance(void* instance) {
  return T(*reinterpret_cast<void**>(getAtOffset<offset>(instance)));
}

template <typename T, std::size_t offset>
CORDL_HIDDEN void setReferenceTypeInstance(void* instance, T t) {
  ::il2cpp_functions::Init();
  ::il2cpp_functions::gc_wbarrier_set_field(instance, getAtOffset(),
                                            t.convert());
}

template <typename T, std::size_t offset>
CORDL_HIDDEN inline T& getValueTypeInstance(void* instance) {
  // TODO: construct into union data
  return *reinterpret_cast<T*>(getAtOffset<offset>(instance));
}

template <typename T, std::size_t offset>
CORDL_HIDDEN inline void setValueTypeInstance(void* instance, T&& t) {
  // TODO: assign using union data
  *reinterpret_cast<T*>(getAtOffset<offset>(instance)) = t;
}

template <typename T, internal::NTTPString name, auto klass_resolver>
CORDL_HIDDEN T getReferenceTypeStatic() {
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

template <typename T, internal::NTTPString name, auto klass_resolver>
CORDL_HIDDEN void setReferenceTypeStatic(T t) {
  auto klass = klass_resolver();
  if (!klass)
    throw NullException(std::string("Class for static field with name: ") +
                        name.data.data() + " is null!");
  auto val = ::il2cpp_utils::SetFieldValue(klass, name.data.data(), t);
  if (!val)
    throw FieldException(std::string("Could not set static field with name: ") +
                         name.data.data());
  return *val;
}

template <typename T, internal::NTTPString name, auto klass_resolver>
CORDL_HIDDEN T getValueTypeStatic() {
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

template <typename T, internal::NTTPString name, auto klass_resolver>
CORDL_HIDDEN void setValueTypeStatic(T&& t) {
  auto klass = klass_resolver();
  if (!klass)
    throw NullException(std::string("Class for static field with name: ") +
                        name.data.data() + " is null!");
  auto val = ::il2cpp_utils::SetFieldValue(klass, name.data.data(), t);
  if (!val)
    throw FieldException(std::string("Could not set static field with name: ") +
                         name.data.data());
  return *val;
}

template <typename T>
concept il2cpp_value_type = requires(T const& t) {
  { std::is_array_v<decltype(t.__instance)> };
  T::__CORDL_IS_VALUE_TYPE == true;
  //   { T::__CORDL_IS_VALUE_TYPE } -> std::equal_to_v<true>;
};

template <typename T>
concept il2cpp_reference_type = requires(T const& t) {
  { std::is_array_v<decltype(t.__instance)> };
  T::__CORDL_IS_VALUE_TYPE == false;
  //   { T::__CORDL_IS_VALUE_TYPE } -> std::equal_to_v<true>;
} && std::is_assignable_v<T, ::bs_hook::Il2CppWrapperObject>;

template <typename IT> struct InterfaceW {
  void* instance;

  // reference type ctor
  template <il2cpp_reference_type U>
    requires(std::is_assignable_v<U, IT>)
  constexpr InterfaceW(U o) : instance(o.instance) {}

  // value type convert
  template <il2cpp_value_type U>
    requires(std::is_assignable<U, IT>)
  InterfaceW(U&& o)
      : instance(il2cpp_functions::value_box(classof(U), &std::forward<U>(o))) {
  }
};

} // namespace cordl_internals