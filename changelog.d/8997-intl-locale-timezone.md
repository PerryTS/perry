### Fixed

- `Intl.DateTimeFormat` now resolves arbitrary named `timeZone` options through
  Perry's compiled IANA database, including daylight-saving transitions,
  instead of silently formatting non-host zones as UTC. Weekday-only formats
  now use the requested locale's CLDR data rather than falling back to English.
- Auto-optimized `Intl.Collator` builds now retain the Unicode normalization
  tables used by locale-aware comparison, instead of silently degrading to
  codepoint order.
