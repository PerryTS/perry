import "reflect-metadata";

const MODULE_METADATA = {
  PROVIDERS: "providers",
  CONTROLLERS: "controllers",
};
const PATH_METADATA = "path";
const METHOD_METADATA = "method";
const PARAMTYPES_METADATA = "design:paramtypes";
const INJECTABLE_WATERMARK = "__injectable__";
const CONTROLLER_WATERMARK = "__controller__";
const RequestMethod = { GET: 0 };

function Injectable(): ClassDecorator {
  return target => {
    Reflect.defineMetadata(INJECTABLE_WATERMARK, true, target);
  };
}

function Controller(prefix: string): ClassDecorator {
  return target => {
    Reflect.defineMetadata(CONTROLLER_WATERMARK, true, target);
    Reflect.defineMetadata(PATH_METADATA, prefix, target);
  };
}

function Get(path: string): MethodDecorator {
  return (_target: object, _key: string | symbol, descriptor: PropertyDescriptor) => {
    Reflect.defineMetadata(PATH_METADATA, path, descriptor.value);
    Reflect.defineMetadata(METHOD_METADATA, RequestMethod.GET, descriptor.value);
  };
}

function Module(metadata: any): ClassDecorator {
  return target => {
    for (const property in metadata) {
      if (Object.hasOwnProperty.call(metadata, property)) {
        Reflect.defineMetadata(property, metadata[property], target);
      }
    }
  };
}

@Injectable()
class Repo {
  getLabel() {
    return "repo";
  }
}

@Injectable()
class UsersService {
  repo: Repo;

  constructor(repo: Repo) {
    this.repo = repo;
  }

  findOne() {
    return "user:" + this.repo.getLabel();
  }
}

@Controller("/users")
class UsersController {
  service: UsersService;

  constructor(service: UsersService) {
    this.service = service;
  }

  @Get("/:id")
  find() {
    return this.service.findOne();
  }
}

@Module({
  providers: [Repo, UsersService],
  controllers: [UsersController],
})
class AppModule {}

function resolveProvider(token: any): any {
  if (token === Repo) {
    return new Repo();
  }

  if (token === UsersService) {
    const deps = Reflect.getMetadata(PARAMTYPES_METADATA, UsersService) || [];
    return new UsersService(resolveProvider(deps[0]));
  }

  return undefined;
}

const providers = Reflect.getMetadata(MODULE_METADATA.PROVIDERS, AppModule);
const controllers = Reflect.getMetadata(MODULE_METADATA.CONTROLLERS, AppModule);
const controllerType = controllers[0];
const controllerDeps = Reflect.getMetadata(PARAMTYPES_METADATA, controllerType) || [];
const controller = new UsersController(resolveProvider(controllerDeps[0]));
const names = Object.getOwnPropertyNames(controllerType.prototype);

let routePath = "";
let routeMethod = -1;
let routeResult = "";
for (const name of names) {
  const descriptor = Object.getOwnPropertyDescriptor(controllerType.prototype, name);
  if (!descriptor) {
    continue;
  }

  const path = Reflect.getMetadata(PATH_METADATA, descriptor.value);
  if (path === "/:id") {
    routePath = path;
    routeMethod = Reflect.getMetadata(METHOD_METADATA, descriptor.value);
    routeResult = controller.find();
  }
}

console.log("module providers", providers[0] === Repo && providers[1] === UsersService);
console.log("module provider length", providers.length);
console.log("module controllers", controllerType === UsersController);
console.log("module controller length", controllers.length);
console.log("provider injectable", Reflect.getMetadata(INJECTABLE_WATERMARK, UsersService));
console.log("controller metadata", Reflect.getMetadata(CONTROLLER_WATERMARK, UsersController), Reflect.getMetadata(PATH_METADATA, UsersController));
console.log("service deps", Reflect.getMetadata(PARAMTYPES_METADATA, UsersService)[0] === Repo);
console.log("controller deps", controllerDeps[0] === UsersService);
console.log("controller dep length", controllerDeps.length);
console.log("route metadata", routePath, routeMethod);
console.log("injected handler", routeResult);
