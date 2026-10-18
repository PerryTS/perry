HTTP response prototypes now install separate native bodies for `setHeader`,
`writeHead`, `write`, and `end`. Each body proves the receiver from its current
ordinary-layout word, native payload type word, and exact payload vtable before
entering the shared response operation. Ordinary own-field growth preserves the
carrier proof. Legacy handles and incompatible receivers retain complete method
dispatch. Subclass overrides and patched prototype methods still resolve through
ordinary property lookup.
