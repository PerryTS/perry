Classes that extend `Array` can declare private fields, methods and accessors
(#13960): constructing one no longer throws "Cannot initialize #x on this
object", and the private elements survive `push` growth. This unblocks code
such as ethers' ABI `Result extends Array`.
