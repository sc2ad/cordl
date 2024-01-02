#pragma once

#include "config.hpp"
#include <array>
#include <cstddef>
#include <cstring>
#include <string_view>

namespace UnityEngine {
    class Object;
}

namespace {
namespace cordl_internals {
    namespace internal {
        template <std::size_t sz> struct NTTPString {
            constexpr NTTPString(char const (&n)[sz]) : data{} {
                std::copy_n(n, sz, data.begin());
            }
            std::array<char, sz> data;
            constexpr operator std::string_view() const {
                return {data.data(), sz};
            }
        };
    }

    /// @brief gets an offset from a given pointer
    template <std::size_t offset>
    CORDL_HIDDEN constexpr inline void** getAtOffset(void* instance) {
        return static_cast<void**>(static_cast<void*>(static_cast<uint8_t*>(instance) + offset));
    }

    /// @brief gets an offset from a given pointer
    template <std::size_t offset>
    CORDL_HIDDEN constexpr inline const void* const* getAtOffset(const void* instance) {
        return static_cast<const void* const*>(static_cast<const void*>(static_cast<const uint8_t*>(instance) + offset));
    }

    /// @brief reads the cachedptr on the given unity object instance
    template<typename T>
    requires(std::is_convertible_v<T, UnityEngine::Object*>)
    CORDL_HIDDEN inline constexpr void* read_cachedptr(T instance) {
        return *static_cast<void**>(getAtOffset<0x10>(static_cast<UnityEngine::Object*>(instance)));
    }

    // if you compile with the define RUNTIME_FIELD_NULL_CHECKS at runtime every field access will be null checked for you, and a c++ exception will be thrown if the instance is null.
    // in case of a unity object, the m_CachedPtr is also checked. Since this can incur some overhead you can also just not define RUNTIME_FIELD_NULL_CHECKS to save performance
    #ifdef CORDL_RUNTIME_FIELD_NULL_CHECKS
        #define CORDL_FIELD_NULL_CHECK(inst) if (!inst) throw ::cordl_internals::NullException(std::string("Field access on nullptr instance, please make sure your instance is not null"))
    #else
        #define CORDL_FIELD_NULL_CHECK(instance)
    #endif

    template<typename T>
    requires(std::is_pointer_v<T>)
    constexpr inline void* convert(T&& inst) { return static_cast<void*>(const_cast<void*>(static_cast<const void*>(inst))); }

    template<il2cpp_utils::has_il2cpp_conversion T>
    constexpr inline void* convert(T&& inst) { return inst.convert(); }
}
}
