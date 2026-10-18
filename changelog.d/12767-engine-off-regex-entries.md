A runtime built without the `regex-engine` feature (the compiler links one)
defines the boxed `String.prototype.match` / `matchAll` / `search` bodies
again. The builtin method entries name them unconditionally, so MSVC's linker
rejected `perry.exe` (LNK2019 `js_string_match_js`, `js_string_match_all_js`,
`js_string_search_js`) and both Windows release legs failed. They answer as
the engine-off method dispatch already does: no match. A program that uses a
regex builds the engine in.
