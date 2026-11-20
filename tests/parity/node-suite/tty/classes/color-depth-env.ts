import tty from "node:tty";

const proto: any = tty.WriteStream.prototype;

console.log("depth empty:", proto.getColorDepth({}) === 1);
console.log("depth force 0:", proto.getColorDepth({ FORCE_COLOR: "0" }) === 1);
console.log("depth force 1:", proto.getColorDepth({ FORCE_COLOR: "1" }) === 4);
console.log("depth force 2:", proto.getColorDepth({ FORCE_COLOR: "2" }) === 8);
console.log("depth force 3:", proto.getColorDepth({ FORCE_COLOR: "3" }) === 24);
console.log("depth no color:", proto.getColorDepth({ NO_COLOR: "1", TERM: "xterm" }) === 1);
console.log("depth dumb:", proto.getColorDepth({ TERM: "dumb" }) === 1);
console.log("depth xterm:", proto.getColorDepth({ TERM: "xterm" }) === 4);
console.log("depth xterm 256:", proto.getColorDepth({ TERM: "xterm-256color" }) === 8);
console.log("depth screen 256:", proto.getColorDepth({ TERM: "screen-256color" }) === 4);
console.log("depth truecolor:", proto.getColorDepth({ COLORTERM: "truecolor" }) === 24);
console.log("depth tmux:", proto.getColorDepth({ TMUX: "1" }) === 24);
console.log("depth travis:", proto.getColorDepth({ CI: "1", TRAVIS: "1" }) === 8);
console.log("depth github actions:", proto.getColorDepth({ CI: "1", GITHUB_ACTIONS: "true" }) === 24);
console.log("depth teamcity 9.1:", proto.getColorDepth({ TEAMCITY_VERSION: "9.1.0" }) === 4);
