; What to look for in JavaScript. The Rust side names things and tracks scopes; the capture name
; says only which kind of syntax matched.
(import_statement) @import
(export_statement) @export
(call_expression) @call
(new_expression) @new
(class_declaration) @class
(function_declaration) @function
(generator_function_declaration) @function
(method_definition) @method
(variable_declarator value: [(arrow_function) (function_expression)]) @variable_function
(field_definition value: [(arrow_function) (function_expression)]) @field_function
