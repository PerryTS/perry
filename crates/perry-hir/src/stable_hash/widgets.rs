//! `SH` impls for the widget tree (perry/ui declarative widgets).
//! Split out of `stable_hash.rs` (no behavior change).

use super::primitives::{tag, SH};
use super::StableHasher;
use crate::ir::*;

// --- Widget tree -----------------------------------------------------------

impl SH for WidgetDecl {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        let WidgetDecl {
            kind,
            display_name,
            description,
            supported_families,
            entry_fields,
            render_body,
            entry_param_name,
            config_params,
            provider_func_name,
            placeholder,
            family_param_name,
            app_group,
            reload_after_seconds,
        } = self;
        kind.hash(h);
        display_name.hash(h);
        description.hash(h);
        supported_families.hash(h);
        entry_fields.hash(h);
        render_body.hash(h);
        entry_param_name.hash(h);
        config_params.hash(h);
        provider_func_name.hash(h);
        placeholder.hash(h);
        family_param_name.hash(h);
        app_group.hash(h);
        reload_after_seconds.hash(h);
    }
}

impl SH for WidgetConfigParam {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        let WidgetConfigParam {
            name,
            title,
            param_type,
        } = self;
        name.hash(h);
        title.hash(h);
        param_type.hash(h);
    }
}

impl SH for WidgetConfigParamType {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        match self {
            WidgetConfigParamType::Enum { values, default } => {
                tag(h, 0);
                values.hash(h);
                default.hash(h);
            }
            WidgetConfigParamType::Bool { default } => {
                tag(h, 1);
                default.hash(h);
            }
            WidgetConfigParamType::String { default } => {
                tag(h, 2);
                default.hash(h);
            }
        }
    }
}

impl SH for WidgetPlaceholderValue {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        match self {
            WidgetPlaceholderValue::String(s) => {
                tag(h, 0);
                s.hash(h);
            }
            WidgetPlaceholderValue::Number(n) => {
                tag(h, 1);
                n.hash(h);
            }
            WidgetPlaceholderValue::Bool(b) => {
                tag(h, 2);
                b.hash(h);
            }
            WidgetPlaceholderValue::Array(items) => {
                tag(h, 3);
                items.hash(h);
            }
            WidgetPlaceholderValue::Object(fields) => {
                tag(h, 4);
                fields.hash(h);
            }
            WidgetPlaceholderValue::Null => tag(h, 5),
        }
    }
}

impl SH for WidgetFieldType {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        match self {
            WidgetFieldType::String => tag(h, 0),
            WidgetFieldType::Number => tag(h, 1),
            WidgetFieldType::Boolean => tag(h, 2),
            WidgetFieldType::Array(inner) => {
                tag(h, 3);
                inner.as_ref().hash(h);
            }
            WidgetFieldType::Optional(inner) => {
                tag(h, 4);
                inner.as_ref().hash(h);
            }
            WidgetFieldType::Object(fields) => {
                tag(h, 5);
                fields.hash(h);
            }
        }
    }
}

impl SH for WidgetNode {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        match self {
            WidgetNode::Text { content, modifiers } => {
                tag(h, 0);
                content.hash(h);
                modifiers.hash(h);
            }
            WidgetNode::Stack {
                kind,
                spacing,
                children,
                modifiers,
            } => {
                tag(h, 1);
                kind.hash(h);
                spacing.hash(h);
                children.hash(h);
                modifiers.hash(h);
            }
            WidgetNode::Image {
                system_name,
                modifiers,
            } => {
                tag(h, 2);
                system_name.hash(h);
                modifiers.hash(h);
            }
            WidgetNode::Spacer => tag(h, 3),
            WidgetNode::Conditional {
                field,
                op,
                value,
                then_node,
                else_node,
            } => {
                tag(h, 4);
                field.hash(h);
                op.hash(h);
                value.hash(h);
                then_node.as_ref().hash(h);
                else_node.hash(h);
            }
            WidgetNode::ForEach {
                collection_field,
                item_param,
                body,
            } => {
                tag(h, 5);
                collection_field.hash(h);
                item_param.hash(h);
                body.as_ref().hash(h);
            }
            WidgetNode::Divider => tag(h, 6),
            WidgetNode::Label {
                text,
                system_image,
                modifiers,
            } => {
                tag(h, 7);
                text.hash(h);
                system_image.hash(h);
                modifiers.hash(h);
            }
            WidgetNode::FamilySwitch { cases, default } => {
                tag(h, 8);
                cases.hash(h);
                default.hash(h);
            }
            WidgetNode::Gauge {
                value_expr,
                label,
                style,
                modifiers,
            } => {
                tag(h, 9);
                value_expr.hash(h);
                label.hash(h);
                style.hash(h);
                modifiers.hash(h);
            }
        }
    }
}

impl SH for GaugeStyle {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        match self {
            GaugeStyle::Circular => tag(h, 0),
            GaugeStyle::LinearCapacity => tag(h, 1),
        }
    }
}

impl SH for WidgetTextContent {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        match self {
            WidgetTextContent::Literal(s) => {
                tag(h, 0);
                s.hash(h);
            }
            WidgetTextContent::Field(s) => {
                tag(h, 1);
                s.hash(h);
            }
            WidgetTextContent::Template(parts) => {
                tag(h, 2);
                parts.hash(h);
            }
            WidgetTextContent::Formatted(expr) => {
                tag(h, 3);
                expr.hash(h);
            }
        }
    }
}

impl SH for WidgetTemplatePart {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        match self {
            WidgetTemplatePart::Literal(s) => {
                tag(h, 0);
                s.hash(h);
            }
            WidgetTemplatePart::Field(s) => {
                tag(h, 1);
                s.hash(h);
            }
            WidgetTemplatePart::Formatted(expr) => {
                tag(h, 2);
                expr.hash(h);
            }
        }
    }
}

impl SH for WidgetFormatExpr {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        self.call.hash(h);
        self.arg.hash(h);
    }
}

impl SH for WidgetFormatCall {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        match self {
            WidgetFormatCall::StringCast => tag(h, 0),
            WidgetFormatCall::NumberCast => tag(h, 1),
            WidgetFormatCall::Round => tag(h, 2),
            WidgetFormatCall::Floor => tag(h, 3),
            WidgetFormatCall::Ceil => tag(h, 4),
            WidgetFormatCall::ToFixed { digits } => {
                tag(h, 5);
                digits.hash(h);
            }
            WidgetFormatCall::ToString => tag(h, 6),
        }
    }
}

impl SH for WidgetFormatArg {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        match self {
            WidgetFormatArg::Field(s) => {
                tag(h, 0);
                s.hash(h);
            }
            WidgetFormatArg::Number(n) => {
                tag(h, 1);
                n.hash(h);
            }
            WidgetFormatArg::String(s) => {
                tag(h, 2);
                s.hash(h);
            }
        }
    }
}

impl SH for WidgetStackKind {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        match self {
            WidgetStackKind::VStack => tag(h, 0),
            WidgetStackKind::HStack => tag(h, 1),
            WidgetStackKind::ZStack => tag(h, 2),
        }
    }
}

impl SH for WidgetConditionOp {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        match self {
            WidgetConditionOp::GreaterThan => tag(h, 0),
            WidgetConditionOp::LessThan => tag(h, 1),
            WidgetConditionOp::Equals => tag(h, 2),
            WidgetConditionOp::NotEquals => tag(h, 3),
            WidgetConditionOp::Truthy => tag(h, 4),
        }
    }
}

impl SH for WidgetModifier {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        match self {
            WidgetModifier::Font(f) => {
                tag(h, 0);
                f.hash(h);
            }
            WidgetModifier::FontWeight(s) => {
                tag(h, 1);
                s.hash(h);
            }
            WidgetModifier::ForegroundColor(s) => {
                tag(h, 2);
                s.hash(h);
            }
            WidgetModifier::Padding(v) => {
                tag(h, 3);
                v.hash(h);
            }
            WidgetModifier::Frame { width, height } => {
                tag(h, 4);
                width.hash(h);
                height.hash(h);
            }
            WidgetModifier::CornerRadius(v) => {
                tag(h, 5);
                v.hash(h);
            }
            WidgetModifier::Background(s) => {
                tag(h, 6);
                s.hash(h);
            }
            WidgetModifier::Opacity(v) => {
                tag(h, 7);
                v.hash(h);
            }
            WidgetModifier::LineLimit(n) => {
                tag(h, 8);
                n.hash(h);
            }
            WidgetModifier::Multiline => tag(h, 9),
            WidgetModifier::MinimumScaleFactor(v) => {
                tag(h, 10);
                v.hash(h);
            }
            WidgetModifier::ContainerBackground(s) => {
                tag(h, 11);
                s.hash(h);
            }
            WidgetModifier::FrameMaxWidth => tag(h, 12),
            WidgetModifier::WidgetURL(s) => {
                tag(h, 13);
                s.hash(h);
            }
            WidgetModifier::PaddingEdge { edge, value } => {
                tag(h, 14);
                edge.hash(h);
                value.hash(h);
            }
        }
    }
}

impl SH for WidgetFont {
    fn hash<H: StableHasher>(&self, h: &mut H) {
        match self {
            WidgetFont::System(v) => {
                tag(h, 0);
                v.hash(h);
            }
            WidgetFont::Named(s) => {
                tag(h, 1);
                s.hash(h);
            }
            WidgetFont::Headline => tag(h, 2),
            WidgetFont::Title => tag(h, 3),
            WidgetFont::Title2 => tag(h, 4),
            WidgetFont::Title3 => tag(h, 5),
            WidgetFont::Body => tag(h, 6),
            WidgetFont::Caption => tag(h, 7),
            WidgetFont::Caption2 => tag(h, 8),
            WidgetFont::Footnote => tag(h, 9),
            WidgetFont::Subheadline => tag(h, 10),
            WidgetFont::LargeTitle => tag(h, 11),
        }
    }
}
