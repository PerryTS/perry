A promise resolved with a thenable whose `then` throws after a collection
(for example a native promise whose `constructor` getter allocates and returns
a non-object, so the intrinsic `then`'s SpeciesConstructor throws) now rejects
the promise. PromiseResolveThenableJob held its promise and its once-only guard
array as raw pointers across the `then` call, so a moving collection there left
the rejection writing to retired from-space (#14683).
