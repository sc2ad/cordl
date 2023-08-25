#pragma once

#include "config.hpp"
#include <array>
#include <cstddef>

namespace cordl_internals {
    namespace internal {
        template <std::size_t sz> struct NTTPString {
            constexpr NTTPString(char const (&n)[sz]) : data{} {
                std::copy_n(n, sz, data.begin());
            }
            std::array<char, sz> data;
        };
    }

    /// @brief gets an offset from a given pointer
    template <std::size_t offset>
    CORDL_HIDDEN constexpr inline void** getAtOffset(void* instance) {
        return reinterpret_cast<void**>(static_cast<uint8_t*>(instance) + offset);
    }

    template <std::size_t sz>
    CORDL_HIDDEN constexpr void copyByByte(std::array<std::byte, sz> const& src, std::array<std::byte, sz>& dst) {
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
    CORDL_HIDDEN constexpr void moveByByte(std::array<std::byte, sz>&& src, std::array<std::byte, sz>& dst) {
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
}
