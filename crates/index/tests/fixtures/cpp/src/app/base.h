#pragma once

/** Root of every drawable thing. */
class Base {
 public:
  virtual ~Base() = default;
};

template <typename T>
class Box {};
