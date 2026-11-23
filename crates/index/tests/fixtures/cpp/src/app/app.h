#pragma once

#include "base.h"
#include "util/strings.h"

namespace app {

int run();

class Widget : public Base, public Box<int> {
 public:
  explicit Widget(int size);
  void draw();
};

}  // namespace app
