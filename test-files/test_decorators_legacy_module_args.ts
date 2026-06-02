import "reflect-metadata";

class Repo {}
class Service {
  constructor(repo: Repo) {}
}
class UsersController {}

function Module(metadata: any): ClassDecorator {
  return target => {
    for (const key of Object.keys(metadata)) {
      Reflect.defineMetadata(key, metadata[key], target);
    }
  };
}

@Module({
  providers: [Service],
  controllers: [UsersController],
})
class AppModule {}

console.log("provider", Reflect.getMetadata("providers", AppModule)[0] === Service);
console.log("controller", Reflect.getMetadata("controllers", AppModule)[0] === UsersController);
console.log(
  "module lengths",
  Reflect.getMetadata("providers", AppModule).length,
  Reflect.getMetadata("controllers", AppModule).length,
);
