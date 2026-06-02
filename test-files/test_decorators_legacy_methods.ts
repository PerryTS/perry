import "reflect-metadata";

let routeHandler: any;

function Controller(prefix: string) {
  return function (target: any) {
    Reflect.defineMetadata("controller:prefix", prefix, target);
  };
}

function Get(path: string) {
  return function (target: any, key: string, descriptor: any) {
    routeHandler = descriptor.value;
    Reflect.defineMetadata("route:path", path, descriptor.value);
    Reflect.defineMetadata("route:key", key, target, key);
  };
}

function RouteParam() {
  return function (target: any, key: string, index: number) {
    Reflect.defineMetadata("route:param:" + index, key, target, key);
  };
}

class User {}

@Controller("/users")
class UsersController {
  @Reflect.metadata("custom:method", "ok")
  @Get("/:id")
  find(@RouteParam() user: User) {}
}

console.log("controller", Reflect.getMetadata("controller:prefix", UsersController));
console.log("route path", Reflect.getMetadata("route:path", routeHandler));
console.log("route key", Reflect.getMetadata("route:key", UsersController, "find"));
console.log("method custom", Reflect.getMetadata("custom:method", UsersController, "find"));
console.log("method param", Reflect.getMetadata("route:param:0", UsersController, "find"));
console.log(
  "method paramtypes",
  Reflect.getMetadata("design:paramtypes", UsersController, "find")[0] === User,
);
