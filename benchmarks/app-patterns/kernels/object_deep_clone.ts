// Pattern: deep-clone a moderately nested object many times. Models
// state-update patterns (immer-style copy-on-write), GraphQL
// resolver shaping, request-context cloning.

const N = 50_000;

interface Inner {
    code: string;
    weight: number;
}

interface Item {
    id: number;
    name: string;
    meta: { created: string; updated: string; tags: string[] };
    inners: Inner[];
}

const proto: Item = {
    id: 0,
    name: "item",
    meta: { created: "2026-01-01", updated: "2026-05-09", tags: ["a", "b", "c"] },
    inners: [
        { code: "x1", weight: 0.5 },
        { code: "x2", weight: 1.5 },
        { code: "x3", weight: 2.5 },
    ],
};

function deepClone(o: Item): Item {
    return {
        id: o.id,
        name: o.name,
        meta: {
            created: o.meta.created,
            updated: o.meta.updated,
            tags: [...o.meta.tags],
        },
        inners: o.inners.map((x) => ({ code: x.code, weight: x.weight })),
    };
}

let totalIds = 0;
for (let i = 0; i < N; i++) {
    proto.id = i;
    const clone = deepClone(proto);
    totalIds += clone.id;
}
console.log("checksum:", totalIds);
