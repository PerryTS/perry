Fixed calls to built-in methods shadowed by an own noncallable property so they throw a TypeError instead of running the original method.
