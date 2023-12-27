#pragma once

#include <cstddef>

namespace {

namespace cordl_internals {
  template <typename T, std::size_t sz>
  requires(sizeof(T) == sz)
  struct size_check {
    static constexpr bool value = true;
  };

  template <typename T, std::size_t sz>
  requires(sizeof(T) == sz)
  static constexpr bool size_check_v = true;
} // namespace cordl_internals
} // end anonymous namespace
