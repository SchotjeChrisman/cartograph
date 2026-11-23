#include "app/app.h"

#include <vector>

namespace app {

static int helper() { return 1; }

int run() {
  util::Buffer buffer;
  buffer.push(helper());
  util::print("hello", 5);
  util::print(4);
  return util::trim_count(2);
}

Widget::Widget(int size) { (void)size; }

void Widget::draw() { helper(); }

}  // namespace app
