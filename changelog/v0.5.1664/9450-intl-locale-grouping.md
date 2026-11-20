### Fixed

- `Intl.NumberFormat` and Number/BigInt locale formatting now use CLDR primary
  and secondary grouping widths instead of fixed three-digit groups. Indian
  locales therefore render `123456789` as `12,34,56,789`, while western
  grouping and `useGrouping: false` remain unchanged. (#9450)
