`buf.constructor` on a Node `Buffer` is the `Buffer` function again (or the
user subclass's constructor), not a namespace object, so
`buf.constructor === Buffer` and `buf.constructor.isBuffer(buf)` work.
