### Fixed

- Regex literals containing raw no-break space (U+00A0) or narrow no-break
  space (U+202F) match the same way as their escaped forms. Parser and runtime
  parity coverage now guards both standalone and character-class patterns
  (#8902).
