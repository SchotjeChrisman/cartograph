#pragma once

namespace util {

int trim_count(int n);
void print(int value);
void print(const char* text, int width);

class Buffer {
 public:
  void push(int value);
  int size() const;
};

}  // namespace util
