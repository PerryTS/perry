`crypto.Certificate` is now the legacy constructor function, as in Node
(#13322). `new crypto.Certificate()` through a namespace import threw
`TypeError: is not a constructor`, and `typeof crypto.Certificate` was
`object`. It now works with or without `new`, and keeps `verifySpkac`,
`exportPublicKey` and `exportChallenge` as static methods.
