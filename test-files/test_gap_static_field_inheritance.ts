// A subclass inherits its parent's static DATA properties — both in-body static
// fields and runtime `Parent.x = …` assignments — via the class-object prototype
// chain (`Sub.__proto__ === Base`). Regression: perry inherited static METHODS
// but returned `undefined` for inherited static data fields, so
// `Sub.tag` / `Sub.kind` read undefined even though `Base` defined them. (Auth.js
// sets `SignInError.kind = "signIn"` and reads it off a `CredentialsSignin`
// subclass to choose the sign-in vs error redirect page.)
class Base {
  static inBody = "field";
}
(Base as any).external = "assigned";
class Sub extends Base {}
class Leaf extends Sub {}
console.log("Sub.inBody=" + (Sub as any).inBody);
console.log("Sub.external=" + (Sub as any).external);
console.log("Leaf.inBody=" + (Leaf as any).inBody);
console.log("Leaf.external=" + (Leaf as any).external);
// own field still wins over inherited
(Sub as any).external = "own";
console.log("Sub.own-wins=" + (Sub as any).external);
console.log("Base.unshadowed=" + (Base as any).external);
