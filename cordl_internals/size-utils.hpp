#pragma once

#include <cstdint>

namespace cordl_internals {
template <typename T, std::size_t sz> struct size_check {
  static constexpr auto value = sizeof(T) == sz;
};

template <typename T, std::size_t sz>
static constexpr bool size_check_v = size_check<T, sz>::value;
} // namespace cordl_internals