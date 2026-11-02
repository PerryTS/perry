Rooted the raw-pointer reads that pushed `raw_handle_debt` over its ceiling.
JSON.stringify of a proxy with a function replacer now hands the replacer to
its consumer through the root (`with_const_ptr`) instead of binding a raw
closure pointer, and keeps the replacer result rooted across the key write.
Two test-only reads (private storage cache, eden entry residency) moved into
scoped-pointer closures. The baseline falls from 871 to 869 and
`reflect_support.rs` drops its stale ceiling from 3 to 2.
