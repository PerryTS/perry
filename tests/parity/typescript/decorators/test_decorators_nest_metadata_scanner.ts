import "reflect-metadata";

const PATH_METADATA = "path";
const METHOD_METADATA = "method";

function Get(path: string): MethodDecorator {
  return (_target: object, _key: string | symbol, descriptor: PropertyDescriptor) => {
    Reflect.defineMetadata(PATH_METADATA, path, descriptor.value);
    Reflect.defineMetadata(METHOD_METADATA, 0, descriptor.value);
  };
}

class UsersController {
  @Get("/:id")
  find() {}
  create() {}
}

const names = Object.getOwnPropertyNames(UsersController.prototype);
const findDescriptor = Object.getOwnPropertyDescriptor(UsersController.prototype, "find");
const findHandler = findDescriptor && findDescriptor.value;
let sawFind = false;
let sawCreate = false;
for (const name of names) {
  if (name === "find") {
    sawFind = true;
  }
  if (name === "create") {
    sawCreate = true;
  }
}

console.log("prototype methods", sawFind && sawCreate);
console.log("find descriptor", !!findDescriptor);
console.log("scanner ready", !!findDescriptor && typeof (UsersController.prototype as any).find === "function");
console.log("route path", Reflect.getMetadata(PATH_METADATA, findHandler));
console.log("route method", Reflect.getMetadata(METHOD_METADATA, findHandler));
