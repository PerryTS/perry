class FirstModule {}
class SecondModule {}

class ModuleRecord {
  readonly value: typeof FirstModule;

  constructor(value: typeof FirstModule) {
    this.value = value;
  }
  get metatype(): typeof FirstModule {
    return this.value;
  }
}

async function beforeSuspend(metatype: typeof FirstModule, token: string) {
  console.log("before", token, metatype.name);
}

async function scan(modules: Map<string, ModuleRecord>) {
  for (const [token, { metatype }] of modules) {
    await beforeSuspend(metatype, token);
    console.log("after", token, metatype?.name);
  }
}

scan(new Map([
  ["first", new ModuleRecord(FirstModule)],
  ["second", new ModuleRecord(SecondModule)],
])).catch((error: Error) => {
  console.error(error);
  process.exitCode = 1;
});
