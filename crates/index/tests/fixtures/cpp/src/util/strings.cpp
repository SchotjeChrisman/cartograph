#include "strings.h"

namespace util {

int trim_count(int n) { return n - 1; }

void print(int value) { (void)value; }

void print(const char* text, int width) {
  print(width);
  (void)text;
}

void Buffer::push(int value) { (void)value; }

int Buffer::size() const { return 0; }

}  // namespace util
