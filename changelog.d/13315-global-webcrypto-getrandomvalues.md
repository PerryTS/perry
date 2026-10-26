`globalThis.crypto.getRandomValues()` now works in programs that never import
or name the crypto module (#13313). It used to throw
`crypto.webcrypto.getRandomValues is not supported`, because only naming the
module installed the crypto dispatch code. The call, including the extracted
method form, now fills the array through the same helper as
`crypto.randomFillSync()`, without linking the rest of the crypto dispatch
code into every binary.
