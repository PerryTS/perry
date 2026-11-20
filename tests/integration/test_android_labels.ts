const thresholds = [0, 20, 40, 60, 80, 100];
const levels = [
    { name: "very_low", min: 0, max: 20 },
    { name: "low", min: 20, max: 40 },
    { name: "medium", min: 40, max: 60 },
    { name: "high", min: 60, max: 80 },
    { name: "very_high", min: 80, max: 100 },
];

export function getLevelInfo(value: number): string {
    for (let i = 0; i < thresholds.length; i++) {
        if (value < thresholds[i]) {
            return levels[i].name;
        }
    }
    return levels[levels.length - 1].name;
}
