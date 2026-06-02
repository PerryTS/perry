//! Widget-extension HIR — WidgetDecl + all child node/modifier enums. Compiles
//! to SwiftUI/Compose source via `perry_codegen`. Re-exported from `super`.

/// A widget extension declaration (WidgetKit on iOS/watchOS, Glance on Android, Tiles on Wear OS)
#[derive(Debug, Clone)]
pub struct WidgetDecl {
    /// Widget kind identifier (e.g., "com.example.MyWidget")
    pub kind: String,
    /// Display name for the widget gallery
    pub display_name: String,
    /// Description for the widget gallery
    pub description: String,
    /// Supported widget families (e.g., "systemSmall", "systemMedium", "systemLarge",
    /// "accessoryCircular", "accessoryRectangular", "accessoryInline")
    pub supported_families: Vec<String>,
    /// Entry type fields: (name, type) — flattened from the TypeScript interface
    pub entry_fields: Vec<(String, WidgetFieldType)>,
    /// The render function body — compiled to SwiftUI/Compose source at compile time
    pub render_body: Vec<WidgetNode>,
    /// The render function's entry parameter name
    pub entry_param_name: String,
    /// AppIntent configuration parameters
    pub config_params: Vec<WidgetConfigParam>,
    /// Name of the lowered provider function (compiled via LLVM)
    pub provider_func_name: Option<String>,
    /// Placeholder data for widget gallery preview
    pub placeholder: Option<Vec<(String, WidgetPlaceholderValue)>>,
    /// Family parameter name in render function (for family-specific rendering)
    pub family_param_name: Option<String>,
    /// App group identifier for shared storage (e.g., "group.io.searchbird.shared")
    pub app_group: Option<String>,
    /// Timeline refresh interval in seconds
    pub reload_after_seconds: Option<u32>,
}

/// Configuration parameter for widget (AppIntent on iOS, Config Activity on Android)
#[derive(Debug, Clone)]
pub struct WidgetConfigParam {
    pub name: String,
    pub title: String,
    pub param_type: WidgetConfigParamType,
}

/// Configuration parameter type
#[derive(Debug, Clone)]
pub enum WidgetConfigParamType {
    Enum {
        values: Vec<String>,
        default: String,
    },
    Bool {
        default: bool,
    },
    String {
        default: String,
    },
}

/// Placeholder value for widget preview
#[derive(Debug, Clone)]
pub enum WidgetPlaceholderValue {
    String(String),
    Number(f64),
    Bool(bool),
    Array(Vec<WidgetPlaceholderValue>),
    Object(Vec<(String, WidgetPlaceholderValue)>),
    Null,
}

/// Supported field types in a widget entry
#[derive(Debug, Clone)]
pub enum WidgetFieldType {
    String,
    Number,
    Boolean,
    /// Array of a given element type (e.g., sites: Site[])
    Array(Box<WidgetFieldType>),
    /// Optional type (e.g., error?: string)
    Optional(Box<WidgetFieldType>),
    /// Nested object type with named fields (e.g., { url: string, clicks: number })
    Object(Vec<(String, WidgetFieldType)>),
}

/// A node in the widget render tree — declarative UI description
#[derive(Debug, Clone)]
pub enum WidgetNode {
    /// Text("hello") or Text(entry.field)
    Text {
        content: WidgetTextContent,
        modifiers: Vec<WidgetModifier>,
    },
    /// VStack/HStack/ZStack container
    Stack {
        kind: WidgetStackKind,
        spacing: Option<f64>,
        children: Vec<WidgetNode>,
        modifiers: Vec<WidgetModifier>,
    },
    /// Image(systemName: "star.fill")
    Image {
        system_name: String,
        modifiers: Vec<WidgetModifier>,
    },
    /// Spacer()
    Spacer,
    /// Conditional rendering: condition ? then : else
    Conditional {
        field: String,
        op: WidgetConditionOp,
        value: WidgetTextContent,
        then_node: Box<WidgetNode>,
        else_node: Option<Box<WidgetNode>>,
    },
    /// ForEach(entry.items, (item) => ...)
    ForEach {
        collection_field: String,
        item_param: String,
        body: Box<WidgetNode>,
    },
    /// Divider()
    Divider,
    /// Label("text", systemImage: "star.fill")
    Label {
        text: WidgetTextContent,
        system_image: String,
        modifiers: Vec<WidgetModifier>,
    },
    /// Family-specific rendering: switch on widget family
    FamilySwitch {
        cases: Vec<(String, WidgetNode)>,
        default: Option<Box<WidgetNode>>,
    },
    /// Gauge for watchOS complications
    Gauge {
        value_expr: String,
        label: String,
        style: GaugeStyle,
        modifiers: Vec<WidgetModifier>,
    },
}

/// Gauge display style (for watchOS complications / Wear OS tiles)
#[derive(Debug, Clone)]
pub enum GaugeStyle {
    /// Circular ring gauge (accessoryCircular)
    Circular,
    /// Horizontal bar gauge (accessoryRectangular)
    LinearCapacity,
}

/// Text content — either static string or entry field reference
#[derive(Debug, Clone)]
pub enum WidgetTextContent {
    /// Static string literal
    Literal(String),
    /// Reference to entry field (e.g., entry.title)
    Field(String),
    /// Template literal with parts: `Score: ${entry.score}`
    Template(Vec<WidgetTemplatePart>),
    /// Issue #1179 follow-up: a whitelisted formatting/coercion call
    /// (`String(x)`, `Number(x)`, `x.toFixed(n)`, `x.toString()`,
    /// `Math.round/floor/ceil(x)`) applied to a single argument.
    /// Anything outside this whitelist still degrades to
    /// `Literal(String::new())` so older codepaths don't observe a new
    /// variant they can't interpret; new code MUST handle this variant.
    Formatted(WidgetFormatExpr),
}

#[derive(Debug, Clone)]
pub enum WidgetTemplatePart {
    Literal(String),
    Field(String),
    /// Issue #1179 follow-up: a whitelisted formatting call inside a
    /// template literal hole (e.g., `${Math.round(entry.x)}`).
    Formatted(WidgetFormatExpr),
}

/// Issue #1179 follow-up: whitelisted formatter or coercion that we
/// know how to transpile into each platform's render-text expression
/// (SwiftUI / Kotlin Glance / Wear Tiles). The whitelist is deliberately
/// small — anything richer is the user's job to compute in the provider
/// and pass through as a pre-formatted string field.
#[derive(Debug, Clone)]
pub enum WidgetFormatCall {
    /// `String(x)` — coerce to a display string
    StringCast,
    /// `Number(x)` — coerce to a numeric value
    NumberCast,
    /// `Math.round(x)` — round to the nearest integer
    Round,
    /// `Math.floor(x)`
    Floor,
    /// `Math.ceil(x)`
    Ceil,
    /// `x.toFixed(n)` — fixed-point string with `n` digits after the dot
    ToFixed { digits: u32 },
    /// `x.toString()` — explicit string coercion
    ToString,
}

#[derive(Debug, Clone)]
pub enum WidgetFormatArg {
    /// `entry.<field>` — references a named entry field
    Field(String),
    /// Numeric literal argument (e.g., `Math.round(3.14)`)
    Number(f64),
    /// String literal argument
    String(String),
}

#[derive(Debug, Clone)]
pub struct WidgetFormatExpr {
    pub call: WidgetFormatCall,
    pub arg: WidgetFormatArg,
}

#[derive(Debug, Clone)]
pub enum WidgetStackKind {
    VStack,
    HStack,
    ZStack,
}

#[derive(Debug, Clone)]
pub enum WidgetConditionOp {
    GreaterThan,
    LessThan,
    Equals,
    NotEquals,
    Truthy,
}

/// Style modifiers for widget nodes
#[derive(Debug, Clone)]
pub enum WidgetModifier {
    Font(WidgetFont),
    FontWeight(String),
    ForegroundColor(String),
    Padding(f64),
    Frame {
        width: Option<f64>,
        height: Option<f64>,
    },
    CornerRadius(f64),
    Background(String),
    Opacity(f64),
    LineLimit(u32),
    Multiline,
    /// .minimumScaleFactor(0.5)
    MinimumScaleFactor(f64),
    /// .containerBackground(Color.blue.gradient, for: .widget)
    ContainerBackground(String),
    /// .frame(maxWidth: .infinity)
    FrameMaxWidth,
    /// Deep link URL on a view: .widgetURL(URL(string: "...")!)
    WidgetURL(String),
    /// Edge-specific padding: .padding(.leading, 8)
    PaddingEdge {
        edge: String,
        value: f64,
    },
}

#[derive(Debug, Clone)]
pub enum WidgetFont {
    System(f64),
    Named(String),
    Headline,
    Title,
    Title2,
    Title3,
    Body,
    Caption,
    Caption2,
    Footnote,
    Subheadline,
    LargeTitle,
}
