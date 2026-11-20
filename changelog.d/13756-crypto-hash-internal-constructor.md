`createHash` / `createHmac` objects are born on the prototype of node's
internal `Hash` / `Hmac` constructor, which `crypto.Hash` / `crypto.Hmac`
share: `h instanceof crypto.Hash` is `true`, `h.constructor` is the internal
constructor (not the deprecated wrapper), and `crypto.Hash.prototype.update`
exists before the first `createHash`, as in node.
