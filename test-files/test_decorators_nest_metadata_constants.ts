import "reflect-metadata";

const INJECTABLE_WATERMARK = "__injectable__";
const SELF_DECLARED_DEPS_METADATA = "self:paramtypes";

function Injectable(): ClassDecorator {
  return target => Reflect.defineMetadata(INJECTABLE_WATERMARK, true, target);
}

function Inject(token: any): ParameterDecorator {
  return (target: object, _key: string | symbol | undefined, index: number) => {
    const deps = Reflect.getMetadata(SELF_DECLARED_DEPS_METADATA, target) || [];
    Reflect.defineMetadata(SELF_DECLARED_DEPS_METADATA, [...deps, { index, param: token }], target);
  };
}

class Repo {}

@Injectable()
class Service {
  constructor(@Inject("CUSTOM_REPO") repo: Repo) {}
}

const deps = Reflect.getMetadata(SELF_DECLARED_DEPS_METADATA, Service);
console.log("constant injectable key", Reflect.getMetadata(INJECTABLE_WATERMARK, Service));
console.log("constant param key", !!deps && deps[0].index === 0 && deps[0].param === "CUSTOM_REPO");
