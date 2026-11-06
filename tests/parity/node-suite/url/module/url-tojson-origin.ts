const urls = [new URL("https://example.com/a?b=c#d"), new URL("file:///tmp/a"), new URL("data:text/plain,hi")];
for (const u of urls) {
  console.log("url:", u.toJSON(), u.origin);
}
