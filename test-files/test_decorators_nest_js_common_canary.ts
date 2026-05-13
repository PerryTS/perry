import "reflect-metadata";
import {
  Controller,
  CONTROLLER_WATERMARK,
  Get,
  Injectable,
  INJECTABLE_WATERMARK,
  METHOD_METADATA,
  Module,
  MODULE_METADATA,
  PARAMTYPES_METADATA,
  PATH_METADATA,
} from "./fixtures/nest_like_common.js";

@Injectable()
class Repo {}

@Injectable()
class Service {
  constructor(repo: Repo) {}
}

@Controller("/users")
class UsersController {
  constructor(service: Service) {}

  @Get("/:id")
  find() {
    return "ok";
  }
}

@Module({
  providers: [Repo, Service],
  controllers: [UsersController],
})
class AppModule {}

const providers = Reflect.getMetadata(MODULE_METADATA.PROVIDERS, AppModule);
const controllers = Reflect.getMetadata(MODULE_METADATA.CONTROLLERS, AppModule);
const routeHandler = UsersController.prototype.find;

console.log("js decorator injectable", Reflect.getMetadata(INJECTABLE_WATERMARK, Service));
console.log("js decorator controller", Reflect.getMetadata(CONTROLLER_WATERMARK, UsersController));
console.log("js decorator provider array", providers.length, providers[0] === Repo, providers[1] === Service);
console.log("js decorator controller array", controllers.length, controllers[0] === UsersController);
console.log("js decorator service deps", Reflect.getMetadata(PARAMTYPES_METADATA, Service)[0] === Repo);
console.log("js decorator controller deps", Reflect.getMetadata(PARAMTYPES_METADATA, UsersController)[0] === Service);
console.log("js decorator route", Reflect.getMetadata(PATH_METADATA, routeHandler), Reflect.getMetadata(METHOD_METADATA, routeHandler));
