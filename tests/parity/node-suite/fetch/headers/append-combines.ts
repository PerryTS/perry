const h = new Headers();
h.append("x-test", "a");
h.append("X-Test", "b");
console.log("append get:", h.get("x-test"));

h.set("x-test", "c");
console.log("set replaces:", h.get("x-test"));
h.append("x-test", "d");
console.log("append after set:", h.get("x-test"));

const withCookies = new Headers();
withCookies.append("set-cookie", "a=1");
withCookies.append("set-cookie", "b=2");
console.log("set-cookie get:", withCookies.get("set-cookie"));
