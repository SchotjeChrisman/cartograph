#pragma once

namespace mylib {

// Fast path used by every caller; defined inline so the header is self-contained.
inline int version() { return 3; }

}  // namespace mylib
