### wasm: a collection inside an import callback no longer drops later imports

The wasm host held the imports object as raw NaN-boxed bits, so a collection
triggered *inside* one import callback left every later import in the same call
resolving a relocated object. The lookup failed, the import was skipped, and the
host substituted its default result — so wasm continued with no error reported.
Through undici's llhttp that silently dropped `on_message_complete`: a truncated
HTTP response reported as a clean parse. The host now holds an opaque token and
the imports object stays on the runtime side, where the collector rewrites it.

Found by the new llhttp differential (#9611), which drives both of undici's real
llhttp builds the way undici drives them and requires byte-identical parse
results against node.
