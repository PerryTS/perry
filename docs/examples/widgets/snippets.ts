// demonstrates: home-screen widget declarations from docs/src/widgets/*.md
// docs: docs/src/widgets/overview.md, creating-widgets.md, components.md,
//       configuration.md, data-fetching.md, watchos.md, wearos.md
// platforms: macos, linux
// Windows excluded: this complete-file example uses try/catch, which the
// default Windows RS4GC pipeline intentionally refuses until WinEH funclet
// statepoints are supported (#7354).
// run: false

// `run: false` because `Widget({...})` lowers to a no-op on the host LLVM
// target — the real codegen path runs under `--target ios-widget`,
// `--target android-widget`, or `--target wearos-tile`. The host
// compile-link still catches API drift in the `Widget`/`Text`/`VStack`/etc.
// shapes from `perry/widget`, which is what these snippets are about.

import {
    Widget,
    Text, VStack, HStack, Image, Spacer, Gauge,
} from "perry/widget"

// ANCHOR: minimal
Widget({
    kind: "MyWidget",
    displayName: "My Widget",
    description: "Shows a greeting",
    entryFields: { name: "string" },
    render: (entry) =>
        VStack([
            Text(`Hello, ${entry.name}!`),
        ]),
})
// ANCHOR_END: minimal

// ANCHOR: image-stack
Widget({
    kind: "ImageStack",
    displayName: "Image Stack",
    description: "Image plus a caption",
    entryFields: {
        title: "string",
        image: "string",
    },
    render: (entry) =>
        VStack([
            Image(entry.image),
            Text(entry.title),
            Spacer(),
        ]),
})
// ANCHOR_END: image-stack

// ANCHOR: hstack-row
Widget({
    kind: "HStackRow",
    displayName: "HStack row",
    description: "Side-by-side text + spacer",
    entryFields: {
        left: "string",
        right: "string",
    },
    render: (entry) =>
        HStack([
            Text(entry.left),
            Spacer(),
            Text(entry.right),
        ]),
})
// ANCHOR_END: hstack-row

// ANCHOR: gauge
Widget({
    kind: "GaugeWidget",
    displayName: "Activity Ring",
    description: "Daily move ring",
    entryFields: {
        progress: "number",
    },
    render: (entry) =>
        VStack([
            Gauge(entry.progress, 1.0),
            Text(`${Math.round(entry.progress * 100)}%`),
        ]),
})
// ANCHOR_END: gauge

// ANCHOR: data-fetch
Widget({
    kind: "DataWidget",
    displayName: "Data Widget",
    description: "Reads cached state passed in via the entry payload",
    entryFields: {
        value: "string",
        cacheTimestamp: "number",
    },
    render: (entry) => {
        // Widget render functions are pure — the cache is read by the
        // host app and passed in as `entryFields`, not fetched here.
        const stale = Date.now() - entry.cacheTimestamp > 60_000
        return VStack([
            Text(`Value: ${entry.value}`),
            Text(stale ? "(stale)" : "(fresh)"),
        ])
    },
})
// ANCHOR_END: data-fetch

// ANCHOR: watchos-complication
Widget({
    kind: "QuickStats",
    displayName: "Quick Stats",
    supportedFamilies: ["accessoryCircular", "accessoryRectangular"],

    render(entry: { progress: number; label: string }, family) {
        if (family === "accessoryCircular") {
            return Gauge(entry.progress, 1.0)
        }
        return VStack([
            Text(entry.label),
            Gauge(entry.progress, 1.0),
        ])
    },
})
// ANCHOR_END: watchos-complication

// ANCHOR: wearos-tile
Widget({
    kind: "StepsTile",
    displayName: "Steps",
    description: "Daily step count",
    supportedFamilies: ["accessoryCircular"],

    provider: async () => {
        return {
            entries: [{ steps: 7500, goal: 10000 }],
            reloadPolicy: { after: { minutes: 60 } },
        }
    },

    render(entry: { steps: number; goal: number }) {
        return VStack([
            Gauge(entry.steps / entry.goal, 1.0),
            Text(`${entry.steps}`),
        ])
    },
})
// ANCHOR_END: wearos-tile

// ANCHOR: weather-widget
Widget({
    kind: "WeatherWidget",
    displayName: "Weather",
    description: "Shows current weather",
    entryFields: {
        temperature: "number",
        condition: "string",
        location: "string",
    },
    render: (entry) =>
        VStack([
            HStack([
                Text(entry.location),
                Spacer(),
                Image("cloud.sun.fill"),
            ]),
            Text(`${entry.temperature}°`),
            Text(entry.condition),
        ]),
})
// ANCHOR_END: weather-widget

// ANCHOR: conditional-render
Widget({
    kind: "ConditionalWidget",
    displayName: "Conditional",
    description: "Renders based on entry data",
    entryFields: {
        isActive: "boolean",
        count: "number",
    },
    render: (entry) =>
        VStack([
            Text(entry.isActive ? "Active" : "Inactive"),
            entry.count > 0 ? Text(`${entry.count} items`) : Spacer(),
        ]),
})
// ANCHOR_END: conditional-render

// ANCHOR: template-literal
Widget({
    kind: "TemplateLiteralWidget",
    displayName: "Template Literal",
    description: "Template literals compile to Swift string interpolation",
    entryFields: {
        name: "string",
        score: "number",
    },
    render: (entry) =>
        // Template literal: `${entry.name}: ${entry.score} points`
        // Compiles to: Text("\(entry.name): \(entry.score) points")
        Text(`${entry.name}: ${entry.score} points`),
})
// ANCHOR_END: template-literal

// ANCHOR: city-weather-config
Widget({
    kind: "CityWeather",
    displayName: "City Weather",
    description: "Weather for a chosen city",
    config: {
        city: { type: "string", displayName: "City", default: "New York" },
        units: {
            type: "enum",
            displayName: "Units",
            values: ["Celsius", "Fahrenheit"],
            default: "Celsius",
        },
    },
    entryFields: { temperature: "number", condition: "string" },
    render: (entry) => Text(`${entry.temperature}° ${entry.condition}`),
})
// ANCHOR_END: city-weather-config

// ANCHOR: stock-widget
Widget({
    kind: "StockWidget",
    displayName: "Stock Price",
    description: "Shows current stock price",
    config: {
        symbol: { type: "string", displayName: "Symbol", default: "AAPL" },
    },
    entryFields: { price: "number", change: "string" },
    provider: async (config) => {
        const res = await fetch(`https://api.example.com/stock/${config.symbol}`)
        const data = await res.json()
        return { price: data.price, change: data.change }
    },
    // Inline-options form — the chain form `.font("title")` parses but is
    // dropped at HIR-lowering time (#195).
    render: (entry) =>
        VStack([
            Text(`$${entry.price}`, { font: "title" }),
            Text(entry.change, { color: "green" }),
        ]),
})
// ANCHOR_END: stock-widget

// ANCHOR: stats-widget
Widget({
    kind: "StatsWidget",
    displayName: "Stats",
    description: "Shows daily stats",
    entryFields: {
        steps: "number",
        calories: "number",
        distance: "string",
    },
    // Inline-options modifier form — the `.font("title").bold()` chain form
    // parses but its modifiers don't reach the codegen (#195).
    render: (entry) =>
        VStack([
            HStack([
                Image("figure.walk"),
                Text("Daily Stats", { font: "headline" }),
            ]),
            Spacer(),
            HStack([
                VStack([
                    Text(`${entry.steps}`, { font: "title", fontWeight: "bold" }),
                    Text("steps", { font: "caption", color: "gray" }),
                ]),
                Spacer(),
                VStack([
                    Text(`${entry.calories}`, { font: "title", fontWeight: "bold" }),
                    Text("cal", { font: "caption", color: "gray" }),
                ]),
                Spacer(),
                VStack([
                    Text(entry.distance, { font: "title", fontWeight: "bold" }),
                    Text("km", { font: "caption", color: "gray" }),
                ]),
            ]),
        ], { padding: 16 }),
})
// ANCHOR_END: stats-widget

// ANCHOR: top-sites-widget
Widget({
    kind: "TopSitesWidget",
    displayName: "Top Sites",
    description: "Your top performing sites",
    supportedFamilies: ["systemSmall", "systemMedium"],
    appGroup: "group.com.example.shared",

    config: {
        sortBy: {
            type: "enum",
            values: ["clicks", "impressions", "ctr", "position"],
            default: "clicks",
            title: "Sort By",
        },
        dateRange: {
            type: "enum",
            values: ["7d", "28d", "90d"],
            default: "7d",
            title: "Date Range",
        },
    },

    entryFields: {
        total: "number",
        label: "string",
    },

    provider: async (config: { sortBy: string; dateRange: string }) => {
        const res = await fetch(
            `https://api.example.com/stats?sort=${config.sortBy}&range=${config.dateRange}`,
        )
        const data = await res.json()
        return {
            entries: [{ total: data.total, label: data.label }],
            reloadPolicy: { after: { minutes: 30 } },
        }
    },

    render: (entry) =>
        VStack([
            Text(`${entry.total}`, { font: "title", fontWeight: "bold" }),
            Text(entry.label, { font: "caption", color: "secondary" }),
        ]),
})
// ANCHOR_END: top-sites-widget

// ANCHOR: weather-provider
Widget({
    kind: "WeatherProviderWidget",
    displayName: "Weather",
    description: "Current conditions",
    supportedFamilies: ["systemSmall"],

    entryFields: {
        temperature: "number",
        condition: "string",
    },

    provider: async () => {
        const res = await fetch("https://api.weather.example.com/current")
        const data = await res.json()
        return {
            entries: [
                { temperature: data.temp, condition: data.description },
            ],
            reloadPolicy: { after: { minutes: 15 } },
        }
    },

    render: (entry) =>
        VStack([
            Text(`${entry.temperature}°`, { font: "title" }),
            Text(entry.condition, { font: "caption" }),
        ]),
})
// ANCHOR_END: weather-provider
