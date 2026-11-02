The branch hint on an `if` that guards a loop nest inside a loop is not
emitted when the condition is a compile-time constant. Test:
`stmt::if_loop_hint_tests`.
