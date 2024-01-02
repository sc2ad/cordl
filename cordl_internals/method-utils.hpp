#pragma once

#include "config.hpp"
#include "concepts.hpp"
#include "exceptions.hpp"
#include "box-utils.hpp"
#include <type_traits>
#include <sstream>
#include "il2cpp-tabledefs.h"

namespace {
namespace cordl_internals {
#pragma region extract values
    template<typename T>
    CORDL_HIDDEN void* ExtractValue(T& arg) noexcept;

    template<typename T>
    requires(il2cpp_convertible<T>)
    CORDL_HIDDEN void* ExtractValue(T& arg) noexcept { return arg.convert(); }

    template<typename T>
    requires(std::is_pointer_v<T>)
    CORDL_HIDDEN void* ExtractValue(T& arg) noexcept {
        return const_cast<void*>(static_cast<const void*>(arg));
    }

    template<>
    CORDL_HIDDEN void* ExtractValue<nullptr_t>(nullptr_t&) noexcept { return nullptr; }
    template<>
    CORDL_HIDDEN void* ExtractValue<void*>(void*& arg) noexcept { return arg; }

    template<>
    CORDL_HIDDEN constexpr void* ExtractValue<Il2CppType*>(Il2CppType*&) noexcept { return nullptr; }

    template<>
    CORDL_HIDDEN constexpr void* ExtractValue<Il2CppClass*>(Il2CppClass*&) noexcept { return nullptr; }

    template<>
    CORDL_HIDDEN constexpr void* ExtractValue<Il2CppObject*>(Il2CppObject*& arg) noexcept {
        if (arg) {
            il2cpp_functions::Init();
            auto k = il2cpp_functions::object_get_class(arg);
            if (k && il2cpp_functions::class_is_valuetype(k)) {
                // boxed value type, unbox it
                return il2cpp_functions::object_unbox(static_cast<Il2CppObject*>(arg));
            }
        }

        return arg;
    }

    template<typename T>
    CORDL_HIDDEN void* ExtractValue(T&& arg) noexcept;

    template<typename T>
    requires(il2cpp_convertible<T>)
    CORDL_HIDDEN void* ExtractValue(T&& arg) noexcept { return arg.convert(); }

    template<typename T>
    requires(std::is_pointer_v<T>)
    CORDL_HIDDEN void* ExtractValue(T&& arg) noexcept {
        return const_cast<void*>(static_cast<const void*>(arg));
    }

    template<>
    CORDL_HIDDEN void* ExtractValue<nullptr_t>(nullptr_t&&) noexcept { return nullptr; }
    template<>
    CORDL_HIDDEN void* ExtractValue<void*>(void*&& arg) noexcept { return arg; }

    template<>
    CORDL_HIDDEN constexpr void* ExtractValue<Il2CppType*>(Il2CppType*&&) noexcept { return nullptr; }

    template<>
    CORDL_HIDDEN constexpr void* ExtractValue<Il2CppClass*>(Il2CppClass*&&) noexcept { return nullptr; }

    template<>
    CORDL_HIDDEN constexpr void* ExtractValue<Il2CppObject*>(Il2CppObject*&& arg) noexcept {
        if (arg) {
            il2cpp_functions::Init();
            auto k = il2cpp_functions::object_get_class(arg);
            if (k && il2cpp_functions::class_is_valuetype(k)) {
                // boxed value type, unbox it
                return il2cpp_functions::object_unbox(static_cast<Il2CppObject*>(arg));
            }
        }

        return arg;
    }

    CORDL_HIDDEN inline auto ExtractValues() {
        return ::std::vector<void*>();
    }

    template<typename T, typename... TArgs>
    CORDL_HIDDEN std::vector<void*> ExtractValues(T&& arg, TArgs&& ...args) {
        auto firstVal = ExtractValue(arg);
        auto otherVals = ExtractValues(args...);
        otherVals.insert(otherVals.begin(), firstVal);
        return otherVals;
    }

#pragma endregion // extract values

#pragma region extract type values
    template<typename T>
    CORDL_HIDDEN void* ExtractTypeValue(T& arg) {
        return const_cast<void*>(static_cast<void const*>(&arg));
    }

    template<typename T>
    CORDL_HIDDEN void* ExtractTypeValue(T&& arg) {
        return const_cast<void*>(static_cast<void const*>(&arg));
    }

    template<>
    CORDL_HIDDEN constexpr void* ExtractTypeValue<std::nullptr_t>(std::nullptr_t&) { return nullptr; }

    template<>
    CORDL_HIDDEN constexpr void* ExtractTypeValue<std::nullptr_t>(std::nullptr_t&&) { return nullptr; }

    template<>
    CORDL_HIDDEN void* ExtractTypeValue<::bs_hook::Il2CppWrapperType>(::bs_hook::Il2CppWrapperType& arg) {
        if (arg) { // is it even a set value
            il2cpp_functions::Init();
            auto k = il2cpp_functions::object_get_class(static_cast<Il2CppObject*>(arg));
            if (k && il2cpp_functions::class_is_valuetype(k)) {
                // boxed value type, unbox it
                return il2cpp_functions::object_unbox(static_cast<Il2CppObject*>(arg));
            }
            return arg.convert();
        } else {
            return nullptr;
        }
    }

    template<>
    CORDL_HIDDEN void* ExtractTypeValue<::bs_hook::Il2CppWrapperType>(::bs_hook::Il2CppWrapperType&& arg) {
        if (arg) { // is it even a set value
            il2cpp_functions::Init();
            auto k = il2cpp_functions::object_get_class(static_cast<Il2CppObject*>(arg));
            if (k && il2cpp_functions::class_is_valuetype(k)) {
                // boxed value type, unbox it
                return il2cpp_functions::object_unbox(static_cast<Il2CppObject*>(arg));
            }
            return arg.convert();
        } else {
            return nullptr;
        }
    }

    template<il2cpp_convertible T>
    requires(!std::is_same_v<T, ::bs_hook::Il2CppWrapperType>)
    CORDL_HIDDEN constexpr void* ExtractTypeValue(T& arg) { return arg.convert(); }

    template<il2cpp_convertible T>
    requires(!std::is_same_v<T, ::bs_hook::Il2CppWrapperType>)
    CORDL_HIDDEN constexpr void* ExtractTypeValue(T&& arg) { return arg.convert(); }

    template<typename T>
    requires(std::is_pointer_v<T>)
    CORDL_HIDDEN constexpr void* ExtractTypeValue(T& arg) { return arg; }

    template<typename T>
    requires(std::is_pointer_v<T>)
    CORDL_HIDDEN constexpr void* ExtractTypeValue(T&& arg) { return arg; }

#pragma endregion // extract type values

#pragma region extract type
    template <typename T> CORDL_HIDDEN Il2CppType const* ExtractType() {
        return ::il2cpp_utils::il2cpp_type_check::il2cpp_no_arg_type<T>::get()
            .value_or(nullptr);
    }

    template <typename T> CORDL_HIDDEN Il2CppType const* ExtractType(T&& arg) {
        return ::il2cpp_utils::il2cpp_type_check::il2cpp_arg_type<T>::get(arg)
            .value_or(nullptr);
    }

    CORDL_HIDDEN auto ExtractTypes() {
        return std::vector<Il2CppType const*>();
    }

    template <typename T, typename... TArgs>
    CORDL_HIDDEN std::vector<Il2CppType const*> ExtractTypes(T&& arg,
                                                             TArgs&&... args) {
        auto tFirst = ExtractType(arg);
        auto tOthers = ExtractTypes(args...);
        if (tFirst) tOthers.insert(tOthers.begin(), tFirst);
        return tOthers;
    }

#pragma endregion // extract type
    template <typename TOut = void, bool checkTypes = true, typename T,
              typename... TArgs>
    CORDL_HIDDEN TOut RunMethodRethrow(T&& instance, MethodInfo const* method,
                                       TArgs&&... params) {
        CRASH_UNLESS(method);

        // get the instance value, regardless of if it is boxed or anything
        auto inst = ExtractValue(instance);

        // do a null check for reference instance method calls
#ifndef NO_RUNTIME_INSTANCE_METHOD_NULL_CHECKS
        if constexpr (::il2cpp_utils::il2cpp_reference_type<T>) {
            if ((method->flags & METHOD_ATTRIBUTE_STATIC) == 0) { // method is instance method
                if (!inst) {
                    // if inst evaluates false, we are dealing with a nullptr instance, and the instance method call is a bad idea
                    std::stringstream str;
                    // FIXME: should we use this string, or something else? log a stacktrace?
                    str << "Instance was null for method call of ";
                    str << method->klass->name; str << "::"; str << method->name;
                    throw NullException(str.str());
                }

                #ifndef ALLOW_INVALID_UNITY_METHOD_CALLS
                if constexpr (std::is_convertible_v<T, UnityEngine::Object*>) {
                    if (!read_cachedptr(static_cast<UnityEngine::Object*>(inst))) {
                        // if cached ptr evaluates as false, we are dealing with an invalid unity instance, and the instance method call is a bad idea
                        std::stringstream str;
                        // FIXME: should we use this string, or something else? log a stacktrace?
                        str << "Instance was null for method call of ";
                        str << method->klass->name; str << "::"; str << method->name;
                        throw NullException(str.str());
                    }
                }
                #endif
            }
        }
#endif

        if constexpr (checkTypes && sizeof...(TArgs) > 0) { // param type check
            std::array<Il2CppType const*, sizeof...(TArgs)> types{ ExtractType(
                params)... };
            // TODO: check types array against types in methodinfo

            auto outType = ExtractType<TOut>();
            if (outType) {
                // TODO: check return type against methodinfo return type
            }
        }

        Il2CppException* exp = nullptr;
        std::array<void*, sizeof...(params)> invokeParams{ExtractTypeValue(params)...};
        il2cpp_functions::Init();
        auto* ret = il2cpp_functions::runtime_invoke(method, inst,
                                                     invokeParams.data(), &exp);

        // an exception was thrown, rethrow it!
        if (exp) throw il2cpp_utils::RunMethodException(exp, method);

        if constexpr (!std::is_same_v<void, TOut>) { // return type is not void, we should return something!
            // FIXME: what if the return type is a ByRef<T> ?
            if constexpr (::il2cpp_utils::il2cpp_type_check::need_box<TOut>::value) { // value type returns from runtime invoke are boxed
                // FIXME: somehow allow the gc free as an out of scope instead of having to temporarily save the retval?
                auto retval = Unbox<TOut>(ret);
                il2cpp_functions::il2cpp_GC_free(ret);
                return retval;
            } else if constexpr (il2cpp_utils::il2cpp_reference_type_wrapper<TOut>) { // ref type returns are just that, ref type returns
                return TOut(ret);
            } else { // probably ref type pointer
                return static_cast<TOut>(static_cast<void*>(ret));
            }

        }
    }
}
} // end anonymous namespace
