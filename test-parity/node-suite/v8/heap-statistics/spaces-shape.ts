import { getHeapSpaceStatistics } from "node:v8";

const spaces: any[] = getHeapSpaceStatistics();
const keys = [
  "physical_space_size",
  "space_available_size",
  "space_name",
  "space_size",
  "space_used_size",
];
console.log("array:", Array.isArray(spaces), spaces.length > 0);
console.log(
  "names unique:",
  new Set(spaces.map((space) => space.space_name)).size === spaces.length,
);
console.log(
  "entry keys:",
  spaces.every((space) =>
    Object.keys(space).sort().join(",") === keys.join(",")
  ),
);
console.log(
  "name types:",
  spaces.every((space) =>
    typeof space.space_name === "string" && space.space_name.length > 0
  ),
);
console.log(
  "number types:",
  spaces.every((space) =>
    keys.slice(0, 2).concat(keys.slice(3)).every((key) =>
      typeof space[key] === "number"
    )
  ),
);
console.log(
  "finite nonnegative:",
  spaces.every((space) =>
    keys.slice(0, 2).concat(keys.slice(3)).every((key) =>
      Number.isFinite(space[key]) && space[key] >= 0
    )
  ),
);
