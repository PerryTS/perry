fetch("data:text/plain,hello")
  .then((r) => r.text())
  .then((t) => {
    console.log("fetched " + t);
  });
console.log("sync done");
