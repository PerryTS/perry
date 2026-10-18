The Windows release link resolves the `String.prototype.match` / `matchAll` /
`search` builtin entries again: they call the regex bodies through
`crate::regex` instead of naming them by symbol, which left them unresolved
(LNK2019) in the ThinLTO `prod` link of `perry.exe` even with the engine-off
definitions from #12767 present.
