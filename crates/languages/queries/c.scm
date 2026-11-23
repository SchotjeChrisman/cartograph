; What to look for in C. The Rust side names things and tracks scopes; the capture name says
; only which kind of syntax matched.
(preproc_include) @include
(struct_specifier body: (_)) @class
(union_specifier body: (_)) @class
(enum_specifier body: (_)) @enum
(type_definition) @typedef
(function_definition) @function
(call_expression) @call
