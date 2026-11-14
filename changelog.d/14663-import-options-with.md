`import(specifier, options)` reads and validates `options.with` before loading,
as EvaluateImportCall step 10 requires: a non-object options bag, a non-object
`with` or a non-string attribute value rejects with a `TypeError`, and a
throwing `with` getter is observed (#13488). `with` is read once and enumerated
through `[[OwnPropertyKeys]]`, so Proxy traps run and the loader takes `type`
from the validated attributes instead of reading the getter again.
