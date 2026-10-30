Function objects keep a reset prototype in the closure side table, so
`instanceof` ignored it and fell back to the builtin check;
`has_recorded_prototype` now consults both tables. An inherited getter reached
through a function receiver also now runs with that function as `this`.
