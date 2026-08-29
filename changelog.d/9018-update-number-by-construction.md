### Changed

- `i++`/`i--` on a local proven to always hold a Number (the #8105 number-by-construction fact) now steps inline as `±1.0` instead of calling `js_to_numeric` + `js_numeric_step` per update. Loops like `for (let j = a.length - 1; j >= 0; j--)`, whose counter the integer fact can never admit, drop two runtime calls per iteration.
