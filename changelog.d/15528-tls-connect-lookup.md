- **`tls.connect` `lookup` option.** `tls.connect` now passes `lookup`,
  `family`, `hints` and `autoSelectFamily` to the socket's connect, so a
  custom resolver runs and its answer is reported through `'lookup'` events,
  as in Node.
