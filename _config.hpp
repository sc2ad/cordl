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
  ::il2cpp_functions::gc_wbarrier_set_field(instance, getAtOffset(), t.convert());
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

struct InterfaceW {
  void* instance;
  InterfaceW(::bs_hook::Il2CppWrapperObject o) : instance(o.instance) {}

  // template <il2cpp_value_type T> InterfaceW(T o) : instance(il2cpp_utils::box(o)) {}
  template <il2cpp_value_type T> InterfaceW(T o) : instance(nullptr) {
    // TODO:
  }
};

} // namespace cordl_internals