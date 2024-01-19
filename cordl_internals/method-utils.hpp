#pragma once

#include "config.hpp"
#include "concepts.hpp"
#include "internal.hpp"
#include "exceptions.hpp"
#include <type_traits>
#include <sstream>
#include "il2cpp-tabledefs.h"
#include "beatsaber-hook/shared/utils/il2cpp-utils-methods.hpp"

namespace UnityEngine {
class Object;
}

namespace {
namespace cordl_internals {

template <typename TOut = void, bool checkTypes = true, typename T, typename... TArgs>
CORDL_HIDDEN TOut RunMethodRethrow(T&& instance, MethodInfo const* method, TArgs&&... params) {
  CRASH_UNLESS(method);

  // do a null check for reference instance method calls
#ifndef NO_RUNTIME_INSTANCE_METHOD_NULL_CHECKS
  if constexpr (::il2cpp_utils::il2cpp_reference_type<T>) {
    // get the instance value, regardless of if it is boxed or anything
    auto inst = ::il2cpp_utils::ExtractValue(instance);

    if ((method->flags & METHOD_ATTRIBUTE_STATIC) == 0) { // method is instance method
      if (!inst) {
        // if inst evaluates false, we are dealing with a nullptr instance, and the instance method call is a bad idea
        std::stringstream str;
        // FIXME: should we use this string, or something else? log a stacktrace?
        str << "Instance was null for method call of ";
        str << method->klass->name;
        str << "::";
        str << method->name;
        throw NullException(str.str());
      }

#ifndef ALLOW_INVALID_UNITY_METHOD_CALLS
      if constexpr (std::is_convertible_v<T, UnityEngine::Object*>) {
        if (!::cordl_internals::read_cachedptr(static_cast<UnityEngine::Object*>(inst))) {
          // if cached ptr evaluates as false, we are dealing with an invalid unity instance, and the instance method call is a bad idea
          std::stringstream str;
          // FIXME: should we use this string, or something else? log a stacktrace?
          str << "Instance was null for method call of ";
          str << method->klass->name;
          str << "::";
          str << method->name;
          throw NullException(str.str());
        }
      }
#endif
    }
  }
#endif

  //   if constexpr (checkTypes && sizeof...(TArgs) > 0) { // param type check
  //     std::array<Il2CppType const*, sizeof...(TArgs)> types{ ExtractType(params)... };
  //     // TODO: check types array against types in methodinfo

  //     auto outType = ExtractType<TOut>();
  //     if (outType) {
  //       // TODO: check return type against methodinfo return type
  //     }
  //   }

  return ::il2cpp_utils::RunMethodRethrow<TOut, checkTypes, T, TArgs...>(std::forward<T>(instance), method, std::forward<TArgs>(params)...);
}
} // namespace cordl_internals
} // end anonymous namespace
