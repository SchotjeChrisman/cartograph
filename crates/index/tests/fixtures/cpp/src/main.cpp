#include <mylib/api.h>
#include <vector>

#include "app/app.h"
#include "util/strings.h"
#include "no_such_header.h"

int main() {
  std::vector<int> values;
  values.push_back(mylib::version());
  app::Widget widget(3);
  widget.draw();
  printf("%d", app::run());
  return std::move(values).size();
}
