// parity-node-argv: --experimental-vm-modules --no-warnings
// parity-env: PERRY_EXPERIMENTAL_VM_MODULES=1
import * as vm from "node:vm";

console.log(
    "module keys",
    Object.keys(vm).filter((key) => key.includes("Module")).join(","),
);
console.log(
    "module types",
    typeof vm.Module,
    typeof vm.SourceTextModule,
    typeof vm.SyntheticModule,
    vm.SourceTextModule.length,
    vm.SyntheticModule.length,
);

const dep = new vm.SyntheticModule(
    ["value", "label"],
    () => {
        console.log("synthetic callback");
    },
    { identifier: "dep" },
);

console.log("dep initial", dep.status, dep.identifier, dep.namespace.value === undefined);
dep.setExport("value", 1);
console.log("dep preset", dep.namespace.value);
await dep.link(() => {});
console.log("dep linked", dep.status, dep.namespace.value);
dep.setExport("value", 41);
dep.setExport("label", "dep");
await dep.evaluate();
console.log("dep evaluated", dep.status, dep.namespace.value, dep.namespace.label);

const source = new vm.SourceTextModule(
    [
        'import { value as depValue, label } from "dep";',
        "export const answer = depValue + 1;",
        "export const message = label + ':' + answer;",
    ].join("\n"),
    { identifier: "main" },
);

console.log("source initial", source.status, source.identifier);
console.log("source deps", source.dependencySpecifiers.join(","));
console.log(
    "source requests",
    source.moduleRequests
        .map((request) => `${request.specifier}:${JSON.stringify(request.attributes)}:${request.phase}`)
        .join("|"),
);
console.log("source tla", source.hasTopLevelAwait());
try {
    source.hasAsyncGraph();
} catch (error) {
    console.log("source async precondition", error.code, error.name);
}

await source.link((specifier, referencingModule, extra) => {
    console.log(
        "linker",
        specifier,
        referencingModule.identifier,
        JSON.stringify(extra.attributes),
    );
    return dep;
});
console.log("source linked", source.status, source.hasAsyncGraph());
await source.evaluate();
console.log("source evaluated", source.status, source.namespace.answer, source.namespace.message);
try {
    source.error;
} catch (error) {
    console.log("source error precondition", error.code, error.name);
}

const viaRequests = new vm.SourceTextModule(
    [
        'import { value } from "dep";',
        "export const doubled = value + value;",
    ].join("\n"),
    { identifier: "req" },
);

console.log("req before", viaRequests.status);
viaRequests.linkRequests([dep]);
console.log("req after linkRequests", viaRequests.status);
viaRequests.instantiate();
console.log("req after instantiate", viaRequests.status);
await viaRequests.evaluate();
console.log("req evaluated", viaRequests.status, viaRequests.namespace.doubled);
