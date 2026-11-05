### Fix readline helper callback validation

`clearLine`, `clearScreenDown`, and `moveCursor` now validate their fixed
optional callback positions before any write, including `moveCursor(0, 0)`.
Invalid callbacks throw Node-compatible `TypeError` with
`ERR_INVALID_ARG_TYPE`. Omitted, undefined, and callable callbacks retain their
ANSI output, return values, and deferred callback behavior. `cursorTo` is
unchanged.
