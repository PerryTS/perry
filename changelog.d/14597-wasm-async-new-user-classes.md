`--target web`: an object literal or `new UserClass()` inside an `async`
function threw `toJsValue(...) is not a constructor`, because the async
body's JS emitter wrote the class name as a string. A user class is now built
as in synchronous code (the compiled constructor runs on a `{__class__}`
instance), and method calls on it inside async bodies reach the class's
compiled methods (#14589).
