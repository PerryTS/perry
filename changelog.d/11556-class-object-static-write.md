**Fixed a property write on a class object resolving against the instance accessor chain**, which
killed every `claude-code` subcommand except `--version` at startup.

`set_field_by_name_object_tail` consulted the instance accessor vtable whenever the receiver
carried a non-zero `class_id`. A class constructor object carries its own class's `class_id`, so a
static write such as `Sub.errorCode = "invalid_request"` was resolved against accessors that live
on `Base.prototype` — which is not on the constructor's prototype chain. An instance
`get errorCode()` therefore intercepted the static write and refused it with
"Cannot set property errorCode of #<Sub> which has only a getter".

A class receiver now resolves the static chain instead: a real `static set`, own or inherited
through `extends`, fires; a static accessor with no setter refuses the write as the instance side
does; and a name with no static accessor falls through to the ordinary own-property store.
Instance receivers are unchanged.
