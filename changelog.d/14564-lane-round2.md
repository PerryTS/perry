The `rustc warnings` lane completes its climb: the five census-wrapped tails in
the crypto randomness entry points carry the explicit
`#[allow(unreachable_code)]` they justified (unreachable on the instrumented
linux cohort, the only live path elsewhere), a second `Walker::build` cast is
taken through `*const ()`, the dead `string_prototype_value` test helper is
gone, and `WalkScope`'s tuple is read by the non-linux `Drop` arm so macOS
stops counting it dead.
