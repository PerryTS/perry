const obj: any = {
    nested: { value: 0 },
    fn: undefined,
};

const present = obj.nested?.value ?? 7;
const missing = obj.missing?.value ?? 7;
const call = obj.fn?.("x") ?? "skip";

console.log(`optional:${present}:${missing}:${call}`);
