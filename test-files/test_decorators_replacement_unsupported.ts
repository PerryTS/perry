function ReplaceWithOther() {
  return function (_target: any) {
    return class Other {
      static marker = "replacement";
    };
  };
}

@ReplaceWithOther()
class Original {
  static marker = "original";
}

console.log("replacement ignored", Original.marker);
