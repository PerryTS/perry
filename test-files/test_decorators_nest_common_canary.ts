import "reflect-metadata";

const MODULE_METADATA = {
  IMPORTS: "imports",
  PROVIDERS: "providers",
  CONTROLLERS: "controllers",
  EXPORTS: "exports",
};
const PATH_METADATA = "path";
const METHOD_METADATA = "method";
const PARAMTYPES_METADATA = "design:paramtypes";
const SELF_DECLARED_DEPS_METADATA = "self:paramtypes";
const PROPERTY_DEPS_METADATA = "self:properties_metadata";
const INJECTABLE_WATERMARK = "__injectable__";
const CONTROLLER_WATERMARK = "__controller__";
const RequestMethod = { GET: 0 };

function Injectable(): ClassDecorator {
  return target => Reflect.defineMetadata("__injectable__", true, target);
}

function Controller(prefix: string): ClassDecorator {
  return target => {
    Reflect.defineMetadata("__controller__", true, target);
    Reflect.defineMetadata("path", prefix, target);
  };
}

function Inject(token?: any): PropertyDecorator & ParameterDecorator {
  const injectCallHasArguments = arguments.length > 0;
  return (target: object, key: string | symbol | undefined, index?: number) => {
    let type = token || Reflect.getMetadata("design:type", target, key!);

    if (!type && !injectCallHasArguments) {
      type = Reflect.getMetadata("design:paramtypes", target, key!)?.[index!];
    }

    if (index !== undefined) {
      let dependencies = Reflect.getMetadata("self:paramtypes", target) || [];
      dependencies = [...dependencies, { index, param: type }];
      Reflect.defineMetadata("self:paramtypes", dependencies, target);
      return;
    }

    let properties = Reflect.getMetadata("self:properties_metadata", (target as any).constructor) || [];
    properties = [...properties, { key, type }];
    Reflect.defineMetadata("self:properties_metadata", properties, (target as any).constructor);
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

let routeHandler: any;

function Get(path: string): MethodDecorator {
  return (target: object, key: string | symbol, descriptor: PropertyDescriptor) => {
    routeHandler = descriptor.value;
    Reflect.defineMetadata("path", path, descriptor.value);
    Reflect.defineMetadata("method", 0, descriptor.value);
  };
}

class Repo {}

@Injectable()
class Service {
  @Inject()
  repo!: Repo;

  constructor(@Inject("CUSTOM_REPO") repo: Repo) {}
}

@Controller("/users")
class UsersController {
  @Get("/:id")
  find() {}
}

@Module({
  providers: [Repo, Service],
  controllers: [UsersController],
  exports: [Service],
})
class AppModule {}

const ctorDeps = Reflect.getMetadata(SELF_DECLARED_DEPS_METADATA, Service);
const propDeps = Reflect.getMetadata(PROPERTY_DEPS_METADATA, Service);

console.log("injectable", Reflect.getMetadata(INJECTABLE_WATERMARK, Service));
console.log("design param", Reflect.getMetadata(PARAMTYPES_METADATA, Service)[0] === Repo);
console.log("ctor inject", ctorDeps[0].index, ctorDeps[0].param);
console.log("property inject", propDeps[0].key, propDeps[0].type === Repo);
console.log("controller", Reflect.getMetadata(CONTROLLER_WATERMARK, UsersController));
console.log("controller path", Reflect.getMetadata(PATH_METADATA, UsersController));
console.log("route path", Reflect.getMetadata(PATH_METADATA, routeHandler));
console.log("route method", Reflect.getMetadata(METHOD_METADATA, routeHandler));
console.log("module providers", Reflect.getMetadata(MODULE_METADATA.PROVIDERS, AppModule)[1] === Service);
console.log("module controllers", Reflect.getMetadata(MODULE_METADATA.CONTROLLERS, AppModule)[0] === UsersController);
console.log("module exports", Reflect.getMetadata(MODULE_METADATA.EXPORTS, AppModule)[0] === Service);
console.log(
  "module lengths",
  Reflect.getMetadata(MODULE_METADATA.PROVIDERS, AppModule).length,
  Reflect.getMetadata(MODULE_METADATA.CONTROLLERS, AppModule).length,
  Reflect.getMetadata(MODULE_METADATA.EXPORTS, AppModule).length,
);
