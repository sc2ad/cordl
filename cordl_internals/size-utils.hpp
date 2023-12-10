#pragma once

#include <cstdint>

namespace {

namespace cordl_internals {
template <typename T, std::size_t sz>
requires(sizeof(T) == sz)
struct size_check {
  static constexpr auto value = true;
};

template <typename T, std::size_t sz>
static constexpr bool size_check_v = size_check<T, sz>::value;
} // namespace cordl_internals
} // end anonymous namespace
