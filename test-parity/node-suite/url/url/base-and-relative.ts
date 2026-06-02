for (const [input, base] of [["../x", "https://example.com/a/b/c"], ["?q=1", "https://example.com/a"], ["#h", "https://example.com/a?b=c"], ["//other/path", "https://example.com/a"]]) {
  const u = new URL(input, base);
  console.log("url:", input, "=>", u.href);
}
