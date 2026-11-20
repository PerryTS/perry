const mailto = new URL("mailto:user@example.com");
console.log("mailto protocol:", mailto.protocol);
console.log("mailto host:", mailto.host);
console.log("mailto pathname:", mailto.pathname);
console.log("mailto origin:", mailto.origin);

const data = new URL("data:text/plain,Hello%20World");
console.log("data protocol:", data.protocol);
console.log("data pathname:", data.pathname);
console.log("data origin:", data.origin);

const urn = new URL("urn:isbn:0451450523");
console.log("urn protocol:", urn.protocol);
console.log("urn pathname:", urn.pathname);
console.log("urn href:", urn.href);
