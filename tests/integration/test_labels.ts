const thresholds = [0, 20, 40, 60, 80, 100, 120, 140, 160, 180, 200];
const levels = [
    { name: "very_low", min: 0, max: 20 },
    { name: "low", min: 20, max: 40 },
    { name: "medium_low", min: 40, max: 60 },
    { name: "medium", min: 60, max: 80 },
    { name: "medium_high", min: 80, max: 100 },
    { name: "high", min: 100, max: 120 },
    { name: "very_high", min: 120, max: 140 },
    { name: "extreme", min: 140, max: 160 },
    { name: "critical", min: 160, max: 180 },
    { name: "max", min: 180, max: 200 },
];

export function getLevelInfo(value: number): string {
    for (let i = 0; i < thresholds.length; i++) {
        if (value < thresholds[i]) {
            return levels[i].name;
        }
    }
    return levels[levels.length - 1].name;
}
