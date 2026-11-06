function mapChildren(
  children: number[],
  callback: (value: number) => number,
): number[] {
  console.log("custom-map-entered");
  return children.map(callback);
}

const react: any = {
  default: {
    Children: {
      map: mapChildren,
    },
  },
};

const result = react.default.Children.map(
  [1, 2],
  (value: number) => value * 3,
);
console.log(result.join(","));

// Dynamic dispatch must still recognize a real Array behind an untyped
// property receiver.
const holder: any = { values: [4, 5] };
console.log(holder.values.map((value: number) => value * 2).join(","));
