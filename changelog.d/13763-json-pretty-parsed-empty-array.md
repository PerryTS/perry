`JSON.stringify(value, null, 2)` printed an empty array that came from
`JSON.parse` as `""` instead of `[]` (#13709). The parser builds `[]` with
capacity 0, and the indented printer only accepted arrays with a nonzero
capacity below 10,000. It now checks the GC header type instead. The same
check made an empty parsed array passed as the replacer (`JSON.stringify(o,
JSON.parse("[]"))`) emit every key instead of none; that is fixed too.
