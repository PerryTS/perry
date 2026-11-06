// Exact anchored literals share program data but keep per-evaluation state.
for (const flags of ["", "g", "y", "gy", "u", "gu", "yu", "m", "gm", "i", "gi"]) {
    const re = new RegExp("^--", flags);
    for (const s of ["--option", "x--option", "\n--option", "-", ""]) {
        re.lastIndex = 1;
        console.log(flags, JSON.stringify(s), JSON.stringify(s.replace(re, "[$&]")), re.lastIndex);
        re.lastIndex = 0;
        console.log(flags, JSON.stringify(s.replace(re, (match, offset) => match + ":" + offset)), re.lastIndex);
    }
}
for (const pattern of ["^é", "^😀", "^\\ud800"]) {
    for (const flags of ["", "u", "gu", "yu", "m"]) {
        const re = new RegExp(pattern, flags);
        for (const s of ["éclair", "😀tail", "\ud800tail", "\n😀tail"]) {
            re.lastIndex = 1;
            console.log(pattern, flags, JSON.stringify(s.replace(re, "<$&>")), re.lastIndex);
        }
    }
}
const saved: RegExp[] = [];
for (let i = 0; i < 4; i++) {
    const re = /^--/g;
    console.log("fresh", i, re.lastIndex, "--x".replace(re, ""));
    re.lastIndex = i + 2;
    saved.push(re);
}
console.log("distinct", saved[0] !== saved[1], saved.map(r => r.lastIndex).join(","));
console.log("templates", "--option".replace(/^--/, "$$:$&:$`:$':$1"));
console.log("captured", "--option".replace(/^(--)/, "<$1>"));
console.log("multiline", "x\n--option".replace(/^--/m, ""));
const changing = /^--/g;
console.log("callback", "--option".replace(changing, (match, offset, source) => {
    changing.lastIndex = 7;
    return match + ":" + offset + ":" + source;
}), changing.lastIndex);
const overridden: any = /^--/;
overridden.exec = () => null;
console.log("ownexec", "--option".replace(overridden, "gone"));
