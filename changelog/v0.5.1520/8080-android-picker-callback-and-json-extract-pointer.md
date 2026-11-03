Fixed two Android defects that made `perry-ui-android` unbuildable and stopped
`Picker` from reporting selection changes.

`crates/perry-ui-android/src/json.rs` still called the `is_raw_pointer` helper
that #7448 deleted, so the crate failed to compile for any Android target
(`error[E0425]: cannot find function is_raw_pointer`). #7448 converted the
object path to `extract_pointer` but not the array-element path. Routing it
through the same helper also extends the #7447 fix to array elements: the old
bit test is the IEEE-754 positive-subnormal predicate, so every positive
denormal element in a stringified array was classified as a pointer and
dereferenced.

`PerryBridge.setSpinnerCallback` did not exist. `widgets/picker.rs` has always
called it over JNI and discarded the resulting error with `let _ =`, so
`Picker`'s `onChange` could never fire on Android — the widget rendered and
selected, and the app was simply never told.

Neither showed up in CI because the Android jobs in `feature-matrix.yml` are
`continue-on-error: true` and push-only.
