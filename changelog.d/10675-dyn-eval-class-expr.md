Fixed `new Function`-string interpreter (#6559) to support **class expressions** in a
deliberately narrow subset: a constructor plus regular instance/`static` methods
(identifier/string/numeric keys), no `extends`/decorators/getters-setters/fields/private
members/computed keys/static blocks. This was the sole runtime blocker for `mysql2`, whose
`generate-function`-built row parsers (`text_parser.js`/`binary_parser.js`) return a class
expression from a `Function.apply(...).apply(...)` call; `generate-function` is used well beyond
mysql2, so this likely unblocks other packages too. Desugars onto machinery the interpreter
already had (closure expando writes + the generic `new <closure>` path that already reads a
`"prototype"` dynamic prop), so no new runtime mechanism was added. mysql2 now runs an
end-to-end `CREATE`/`INSERT`/`SELECT`/`DROP` round trip against a real server; the hand-written
native mysql2 binding now looks deletable as a follow-up. See #10661.
