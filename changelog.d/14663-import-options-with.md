`import(specifier, options)` reads and validates `options.with` before loading,
as EvaluateImportCall step 10 requires: a non-object options bag, a non-object
`with` or a non-string attribute value rejects with a `TypeError`, and a
throwing `with` getter is observed (#13488).
