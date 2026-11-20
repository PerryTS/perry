`--target web`: `Toggle(label, onChange)` passes `true`/`false` to `onChange`
instead of `1`/`0`, so `value === true` holds when the box is checked.
