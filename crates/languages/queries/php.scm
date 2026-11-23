; What to look for in PHP and the Laravel plugin layered on it. The Rust side names things and
; tracks scopes; the capture name says only which kind of syntax matched.
(namespace_definition) @namespace
(namespace_use_declaration) @use
(class_declaration) @class
(interface_declaration) @interface
(trait_declaration) @trait
(enum_declaration) @enum
(function_definition) @function
(method_declaration) @method
(use_declaration) @trait_use
(property_declaration) @property
(function_call_expression) @call
(scoped_call_expression) @scoped_call
(member_call_expression) @member_call
(object_creation_expression) @new
(return_statement) @return
