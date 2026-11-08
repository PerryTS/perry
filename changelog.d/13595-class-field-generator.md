A generator function expression stored in a class field initializer
(`static f = function* () {}`, an instance field, or one passed to a call in the
initializer such as Effect's `fnUntraced(function* () {})`) now lowers to a
generator, so calling it returns an iterator instead of the first yielded value.
