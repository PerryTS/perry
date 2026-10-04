// A module-level block has its own lexical environment. Closures created
// before a declaration in that block must capture the block binding, not fall
// through to a global lookup.

{
  const read = () => value;
  const value = 5;
  console.log("const", read());
}

{
  const read = () => value;
  let value = 7;
  value += 2;
  console.log("let", read());
}

{
  const make = () => new Later().value;
  class Later {
    value = 11;
  }
  console.log("class", make());
}

// Each sibling block owns a distinct binding.
{
  const read = () => value;
  const value = 13;
  console.log("sibling-a", read());
}
{
  const read = () => value;
  const value = 17;
  console.log("sibling-b", read());
}
