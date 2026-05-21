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
}

#[derive(Debug, Clone)]
pub enum WidgetTemplatePart {
    Literal(String),
    Field(String),
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
