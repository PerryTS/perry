import "reflect-metadata";

function BaseController(): ClassDecorator {
  return target => {
    Reflect.defineMetadata("role", "base", target);
    Reflect.defineMetadata("shared", "base", target);
  };
}

function ChildController(): ClassDecorator {
  return target => {
    Reflect.defineMetadata("shared", "child", target);
  };
}

function Route(path: string): MethodDecorator {
  return (target: object, key: string | symbol) => {
    Reflect.defineMetadata("route:path", path, target, key);
  };
}

@BaseController()
class BaseUsersController {
  @Route("/base")
  find() {}
}

@ChildController()
class UsersController extends BaseUsersController {}

const metadataKeys = Reflect.getMetadataKeys(UsersController);
const ownKeys = Reflect.getOwnMetadataKeys(UsersController);
let sawInheritedRole = false;
let sawOwnShared = false;
let sawOwnRole = false;

for (const key of metadataKeys) {
  if (key === "role") {
    sawInheritedRole = true;
  }
  if (key === "shared") {
    sawOwnShared = true;
  }
}

for (const key of ownKeys) {
  if (key === "role") {
    sawOwnRole = true;
  }
}

console.log("class inherited", Reflect.getMetadata("role", UsersController));
console.log("class own missing", Reflect.getOwnMetadata("role", UsersController));
console.log("class override", Reflect.getMetadata("shared", UsersController));
console.log("has inherited", Reflect.hasMetadata("role", UsersController), Reflect.hasOwnMetadata("role", UsersController));
console.log("keys inherited", sawInheritedRole && sawOwnShared, sawOwnRole);
console.log("method inherited", Reflect.getMetadata("route:path", UsersController.prototype, "find"));
console.log("method own missing", Reflect.getOwnMetadata("route:path", UsersController.prototype, "find"));
