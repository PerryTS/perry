function show(re: RegExp, input: string): void {
  const match: any = re.exec(input);
  console.log(JSON.stringify(match?.indices));
  console.log(re.hasIndices);
}
show(/hello/d, "say hello world");
show(/(\w+)@(\w+)/d, "email: test@example");
show(/(?<year>\d{4})-(?<month>\d{2})/d, "date: 2024-03");
show(/(\d+)(\.(\d+))?/d, "int: 42");
show(/test/, "test");
