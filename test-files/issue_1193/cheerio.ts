import { load } from "cheerio";

const $ = load("<html><body><p class='x'>hello</p><p class='x'>world</p></body></html>");
console.log("text:", $(".x").text());
console.log("len:", $(".x").length);
