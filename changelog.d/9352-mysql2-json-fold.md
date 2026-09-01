### Internal

- **Folds the JSON array arm in `json_value_to_jsvalue`.** #9350's decoder
  recurses, so `js_array_push(arr, json_value_to_jsvalue(item))` read the array
  pointer as an argument *before* a call that allocates and can move it. The
  fold keeps the array out of a named local, matching the row and field
  builders alongside it, and returns `perry-ext-mysql2` to its unrooted-local
  ceiling of 9.
