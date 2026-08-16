use gtk4::gdk;
use mermaid_rs_renderer::Theme;
use mermaid_rs_renderer::config::{C4Config, LayoutConfig};
use mermaid_rs_renderer::ir::{C4ShapeKind, EdgeStyleOverride, Graph, NodeStyle};

const CANVAS: &str = "#0B0D10";
const LIGHT_TEXT: &str = "#FFFFFF";
const DARK_TEXT: &str = "#000000";
const BODY_TEXT: &str = "#F8FAFC";
const GRAPHIC: &str = "#A8B3C2";
const PRIMARY_FILL: &str = "#1B2633";
const PRIMARY_BORDER: &str = "#8FA2B8";
const TEXT_CONTRAST: f32 = 4.5;
const GRAPHIC_CONTRAST: f32 = 3.0;

const BRANCH_COLORS: [&str; 8] = [
    "#F87171", "#FBBF24", "#A3E635", "#34D399", "#22D3EE", "#60A5FA", "#A78BFA", "#F472B6",
];
const PIE_COLORS: [&str; 12] = [
    "#7F1D1D", "#78350F", "#3F6212", "#14532D", "#155E75", "#1E3A8A", "#4C1D95", "#831843",
    "#7C2D12", "#365314", "#134E4A", "#581C87",
];
const MINDMAP_FILLS: [&str; 12] = [
    "#7F1D1D", "#78350F", "#3F6212", "#14532D", "#155E75", "#1E3A8A", "#4C1D95", "#831843",
    "#7C2D12", "#365314", "#134E4A", "#581C87",
];

pub(super) fn theme() -> Theme {
    let mut theme = Theme::dark();
    theme.primary_color = PRIMARY_FILL.to_owned();
    theme.primary_text_color = BODY_TEXT.to_owned();
    theme.primary_border_color = PRIMARY_BORDER.to_owned();
    theme.line_color = GRAPHIC.to_owned();
    theme.secondary_color = "#253A2E".to_owned();
    theme.tertiary_color = "#372B3F".to_owned();
    theme.edge_label_background = "#111820".to_owned();
    theme.cluster_background = "#141C24".to_owned();
    theme.cluster_border = "#7890A8".to_owned();
    theme.background = "none".to_owned();
    theme.sequence_actor_fill = PRIMARY_FILL.to_owned();
    theme.sequence_actor_border = PRIMARY_BORDER.to_owned();
    theme.sequence_actor_line = GRAPHIC.to_owned();
    theme.sequence_note_fill = "#403714".to_owned();
    theme.sequence_note_border = "#D6B84E".to_owned();
    theme.sequence_activation_fill = "#28384A".to_owned();
    theme.sequence_activation_border = PRIMARY_BORDER.to_owned();
    theme.text_color = BODY_TEXT.to_owned();
    theme.git_colors = BRANCH_COLORS.map(str::to_owned);
    theme.git_inv_colors = [DARK_TEXT; 8].map(str::to_owned);
    theme.git_branch_label_colors = [DARK_TEXT; 8].map(str::to_owned);
    theme.git_commit_label_color = BODY_TEXT.to_owned();
    theme.git_commit_label_background = "#18222D".to_owned();
    theme.git_tag_label_color = BODY_TEXT.to_owned();
    theme.git_tag_label_background = "#253247".to_owned();
    theme.git_tag_label_border = PRIMARY_BORDER.to_owned();
    theme.pie_colors = PIE_COLORS.map(str::to_owned);
    theme.pie_title_text_color = BODY_TEXT.to_owned();
    theme.pie_section_text_color = LIGHT_TEXT.to_owned();
    theme.pie_legend_text_color = BODY_TEXT.to_owned();
    theme.pie_stroke_color = BODY_TEXT.to_owned();
    theme.pie_outer_stroke_color = PRIMARY_BORDER.to_owned();
    theme.pie_opacity = 1.0;
    theme
}

pub(super) fn layout_config() -> LayoutConfig {
    let mut config = LayoutConfig::default();

    config.requirement.fill = PRIMARY_FILL.to_owned();
    config.requirement.box_stroke = PRIMARY_BORDER.to_owned();
    config.requirement.stroke = "#C4A7E7".to_owned();
    config.requirement.label_color = BODY_TEXT.to_owned();
    config.requirement.divider_color = "#C4A7E7".to_owned();
    config.requirement.edge_stroke = GRAPHIC.to_owned();
    config.requirement.edge_label_color = BODY_TEXT.to_owned();
    config.requirement.edge_label_background = "#111820".to_owned();

    config.mindmap.section_colors = MINDMAP_FILLS.map(str::to_owned).to_vec();
    config.mindmap.section_label_colors = [LIGHT_TEXT; 12].map(str::to_owned).to_vec();
    config.mindmap.section_line_colors = BRANCH_COLORS
        .into_iter()
        .cycle()
        .take(12)
        .map(str::to_owned)
        .collect();
    config.mindmap.root_fill = Some(PRIMARY_FILL.to_owned());
    config.mindmap.root_text = Some(BODY_TEXT.to_owned());
    config.mindmap.edge_color = Some(GRAPHIC.to_owned());

    config.gitgraph.cherry_pick_accent_color = "#FBBF24".to_owned();
    set_c4_palette(&mut config.c4);
    config
}

pub(super) fn enforce_contrast(graph: &mut Graph, theme: &Theme, config: &LayoutConfig) {
    enforce_node_styles(graph, theme);
    enforce_subgraph_styles(graph, theme);
    enforce_edge_styles(graph, theme);
    enforce_c4_styles(graph, &config.c4);

    for sequence_box in &mut graph.sequence_boxes {
        if sequence_box
            .color
            .as_deref()
            .is_some_and(|color| parse_color(color).is_none())
        {
            sequence_box.color = Some(GRAPHIC.to_owned());
        }
    }
}

fn enforce_node_styles(graph: &mut Graph, theme: &Theme) {
    let node_ids: Vec<String> = graph.nodes.keys().cloned().collect();
    for node_id in node_ids {
        let mut style = resolve_style(
            graph.node_classes.get(&node_id),
            graph.node_styles.get(&node_id),
            &graph.class_defs,
        );
        let before = style_colors(&style);
        normalize_style(
            &mut style,
            &theme.primary_color,
            &theme.primary_text_color,
            &theme.primary_border_color,
        );
        if before != style_colors(&style) {
            apply_style_corrections(
                graph.node_styles.entry(node_id).or_default(),
                before,
                &style,
            );
        }
    }
}

fn enforce_subgraph_styles(graph: &mut Graph, theme: &Theme) {
    let subgraph_ids: Vec<String> = graph
        .subgraphs
        .iter()
        .filter_map(|subgraph| subgraph.id.clone())
        .collect();
    for subgraph_id in subgraph_ids {
        let mut style = resolve_style(
            graph.subgraph_classes.get(&subgraph_id),
            graph.subgraph_styles.get(&subgraph_id),
            &graph.class_defs,
        );
        let before = style_colors(&style);
        normalize_style(
            &mut style,
            &theme.cluster_background,
            &theme.primary_text_color,
            &theme.cluster_border,
        );
        if before != style_colors(&style) {
            apply_style_corrections(
                graph.subgraph_styles.entry(subgraph_id).or_default(),
                before,
                &style,
            );
        }
    }
}

fn enforce_edge_styles(graph: &mut Graph, theme: &Theme) {
    let canvas = canvas_color();
    let label_background = color_over_canvas(&theme.edge_label_background)
        .unwrap_or_else(|| color_over_canvas(CANVAS).expect("canvas color is valid"));

    for edge_index in 0..graph.edges.len() {
        let mut style = graph.edge_style_default.clone().unwrap_or_default();
        if let Some(override_style) = graph.edge_styles.get(&edge_index) {
            merge_edge_style(&mut style, override_style);
        }
        let before = edge_colors(&style);

        let stroke = style.stroke.as_deref().unwrap_or(&theme.line_color);
        if contrast_against(stroke, canvas).is_none_or(|ratio| ratio < GRAPHIC_CONTRAST) {
            style.stroke = Some(GRAPHIC.to_owned());
        }
        let label = style
            .label_color
            .as_deref()
            .unwrap_or(&theme.primary_text_color);
        if contrast_against(label, label_background).is_none_or(|ratio| ratio < TEXT_CONTRAST) {
            style.label_color = Some(best_text_color(label_background).to_owned());
        }

        if before != edge_colors(&style) {
            apply_edge_corrections(
                graph.edge_styles.entry(edge_index).or_default(),
                before,
                &style,
            );
        }
    }
}

fn enforce_c4_styles(graph: &mut Graph, config: &C4Config) {
    let canvas = canvas_color();
    for shape in &mut graph.c4.shapes {
        let default_fill = c4_fill(config, shape.kind);
        let fill = normalize_background(&mut shape.bg_color, default_fill);
        ensure_text_color(&mut shape.font_color, fill, LIGHT_TEXT);
        ensure_visible_outline(&mut shape.border_color, fill, c4_border(config, shape.kind));
    }
    for boundary in &mut graph.c4.boundaries {
        let fill = normalize_background(&mut boundary.bg_color, &config.boundary_fill);
        ensure_text_color(&mut boundary.font_color, fill, &config.boundary_stroke);
        ensure_visible_outline(&mut boundary.border_color, fill, &config.boundary_stroke);
    }
    for relation in &mut graph.c4.rels {
        ensure_color_contrast(
            &mut relation.line_color,
            canvas,
            &config.boundary_stroke,
            GRAPHIC_CONTRAST,
            GRAPHIC,
        );
        ensure_color_contrast(
            &mut relation.text_color,
            canvas,
            &config.boundary_stroke,
            TEXT_CONTRAST,
            BODY_TEXT,
        );
    }
}

fn normalize_style(style: &mut NodeStyle, fill: &str, text: &str, stroke: &str) {
    let background = normalize_background(&mut style.fill, fill);
    ensure_text_color(&mut style.text_color, background, text);
    ensure_visible_outline(&mut style.stroke, background, stroke);
    if style.line_color.is_some() {
        ensure_color_contrast(
            &mut style.line_color,
            canvas_color(),
            GRAPHIC,
            GRAPHIC_CONTRAST,
            GRAPHIC,
        );
    }
}

fn normalize_background(color: &mut Option<String>, fallback: &str) -> Rgb {
    let current = color.as_deref().unwrap_or(fallback);
    if let Some(parsed) = color_over_canvas(current) {
        return parsed;
    }
    *color = Some(fallback.to_owned());
    color_over_canvas(fallback).unwrap_or_else(canvas_color)
}

fn ensure_text_color(color: &mut Option<String>, background: Rgb, fallback: &str) {
    let current = color.as_deref().unwrap_or(fallback);
    if contrast_against(current, background).is_none_or(|ratio| ratio < TEXT_CONTRAST) {
        *color = Some(best_text_color(background).to_owned());
    }
}

fn ensure_visible_outline(color: &mut Option<String>, fill: Rgb, fallback: &str) {
    let canvas = canvas_color();
    let current = color.as_deref().unwrap_or(fallback);
    let outline_is_valid = contrast_against(current, canvas).is_some();
    let shape_is_visible = contrast_ratio(fill, canvas) >= GRAPHIC_CONTRAST
        || contrast_against(current, canvas).is_some_and(|ratio| ratio >= GRAPHIC_CONTRAST);
    if !outline_is_valid || !shape_is_visible {
        *color = Some(GRAPHIC.to_owned());
    }
}

fn ensure_color_contrast(
    color: &mut Option<String>,
    background: Rgb,
    fallback: &str,
    minimum: f32,
    replacement: &str,
) {
    let current = color.as_deref().unwrap_or(fallback);
    if contrast_against(current, background).is_none_or(|ratio| ratio < minimum) {
        *color = Some(replacement.to_owned());
    }
}

fn resolve_style(
    classes: Option<&Vec<String>>,
    direct: Option<&NodeStyle>,
    class_defs: &std::collections::HashMap<String, NodeStyle>,
) -> NodeStyle {
    let mut resolved = NodeStyle::default();
    for class_name in classes.into_iter().flatten() {
        if let Some(class_style) = class_defs.get(class_name) {
            merge_node_style(&mut resolved, class_style);
        }
    }
    if let Some(direct) = direct {
        merge_node_style(&mut resolved, direct);
    }
    resolved
}

fn merge_node_style(target: &mut NodeStyle, source: &NodeStyle) {
    if source.fill.is_some() {
        target.fill = source.fill.clone();
    }
    if source.stroke.is_some() {
        target.stroke = source.stroke.clone();
    }
    if source.text_color.is_some() {
        target.text_color = source.text_color.clone();
    }
    if source.stroke_width.is_some() {
        target.stroke_width = source.stroke_width;
    }
    if source.stroke_dasharray.is_some() {
        target.stroke_dasharray = source.stroke_dasharray.clone();
    }
    if source.line_color.is_some() {
        target.line_color = source.line_color.clone();
    }
}

fn merge_edge_style(target: &mut EdgeStyleOverride, source: &EdgeStyleOverride) {
    if source.stroke.is_some() {
        target.stroke = source.stroke.clone();
    }
    if source.stroke_width.is_some() {
        target.stroke_width = source.stroke_width;
    }
    if source.dasharray.is_some() {
        target.dasharray = source.dasharray.clone();
    }
    if source.label_color.is_some() {
        target.label_color = source.label_color.clone();
    }
}

type StyleColors = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn style_colors(style: &NodeStyle) -> StyleColors {
    (
        style.fill.clone(),
        style.stroke.clone(),
        style.text_color.clone(),
        style.line_color.clone(),
    )
}

fn apply_style_corrections(target: &mut NodeStyle, before: StyleColors, after: &NodeStyle) {
    if before.0 != after.fill {
        target.fill = after.fill.clone();
    }
    if before.1 != after.stroke {
        target.stroke = after.stroke.clone();
    }
    if before.2 != after.text_color {
        target.text_color = after.text_color.clone();
    }
    if before.3 != after.line_color {
        target.line_color = after.line_color.clone();
    }
}

type EdgeColors = (Option<String>, Option<String>);

fn edge_colors(style: &EdgeStyleOverride) -> EdgeColors {
    (style.stroke.clone(), style.label_color.clone())
}

fn apply_edge_corrections(
    target: &mut EdgeStyleOverride,
    before: EdgeColors,
    after: &EdgeStyleOverride,
) {
    if before.0 != after.stroke {
        target.stroke = after.stroke.clone();
    }
    if before.1 != after.label_color {
        target.label_color = after.label_color.clone();
    }
}

#[derive(Clone, Copy, Debug)]
struct Rgb {
    red: f32,
    green: f32,
    blue: f32,
}

fn parse_color(value: &str) -> Option<(Rgb, f32)> {
    if value.trim().eq_ignore_ascii_case("none") {
        return Some((canvas_color(), 0.0));
    }
    let color = gdk::RGBA::parse(value).ok()?;
    Some((
        Rgb {
            red: color.red(),
            green: color.green(),
            blue: color.blue(),
        },
        color.alpha(),
    ))
}

fn color_over_canvas(value: &str) -> Option<Rgb> {
    let (color, alpha) = parse_color(value)?;
    Some(composite(color, alpha, canvas_color()))
}

fn contrast_against(foreground: &str, background: Rgb) -> Option<f32> {
    let (foreground, alpha) = parse_color(foreground)?;
    Some(contrast_ratio(
        composite(foreground, alpha, background),
        background,
    ))
}

fn composite(foreground: Rgb, alpha: f32, background: Rgb) -> Rgb {
    Rgb {
        red: foreground.red * alpha + background.red * (1.0 - alpha),
        green: foreground.green * alpha + background.green * (1.0 - alpha),
        blue: foreground.blue * alpha + background.blue * (1.0 - alpha),
    }
}

fn contrast_ratio(first: Rgb, second: Rgb) -> f32 {
    let lighter = luminance(first).max(luminance(second));
    let darker = luminance(first).min(luminance(second));
    (lighter + 0.05) / (darker + 0.05)
}

fn luminance(color: Rgb) -> f32 {
    fn linear(channel: f32) -> f32 {
        if channel <= 0.04045 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * linear(color.red) + 0.7152 * linear(color.green) + 0.0722 * linear(color.blue)
}

fn best_text_color(background: Rgb) -> &'static str {
    let black = Rgb {
        red: 0.0,
        green: 0.0,
        blue: 0.0,
    };
    let white = Rgb {
        red: 1.0,
        green: 1.0,
        blue: 1.0,
    };
    if contrast_ratio(black, background) >= contrast_ratio(white, background) {
        DARK_TEXT
    } else {
        LIGHT_TEXT
    }
}

fn canvas_color() -> Rgb {
    Rgb {
        red: 11.0 / 255.0,
        green: 13.0 / 255.0,
        blue: 16.0 / 255.0,
    }
}

fn set_c4_palette(config: &mut C4Config) {
    config.boundary_stroke = PRIMARY_BORDER.to_owned();
    config.boundary_fill = "none".to_owned();

    config.person_bg_color = "#1E3A5F".to_owned();
    config.person_border_color = "#7CB7E8".to_owned();
    set_c4_shape_group(
        &mut config.system_bg_color,
        &mut config.system_db_bg_color,
        &mut config.system_queue_bg_color,
        "#164E63",
    );
    set_c4_shape_group(
        &mut config.system_border_color,
        &mut config.system_db_border_color,
        &mut config.system_queue_border_color,
        "#67E8F9",
    );
    set_c4_shape_group(
        &mut config.container_bg_color,
        &mut config.container_db_bg_color,
        &mut config.container_queue_bg_color,
        "#365314",
    );
    set_c4_shape_group(
        &mut config.container_border_color,
        &mut config.container_db_border_color,
        &mut config.container_queue_border_color,
        "#BEF264",
    );
    set_c4_shape_group(
        &mut config.component_bg_color,
        &mut config.component_db_bg_color,
        &mut config.component_queue_bg_color,
        "#581C87",
    );
    set_c4_shape_group(
        &mut config.component_border_color,
        &mut config.component_db_border_color,
        &mut config.component_queue_border_color,
        "#D8B4FE",
    );

    config.external_person_bg_color = "#3F3F46".to_owned();
    config.external_person_border_color = "#A1A1AA".to_owned();
    set_c4_shape_group(
        &mut config.external_system_bg_color,
        &mut config.external_system_db_bg_color,
        &mut config.external_system_queue_bg_color,
        "#3F3F46",
    );
    set_c4_shape_group(
        &mut config.external_system_border_color,
        &mut config.external_system_db_border_color,
        &mut config.external_system_queue_border_color,
        "#A1A1AA",
    );
    set_c4_shape_group(
        &mut config.external_container_bg_color,
        &mut config.external_container_db_bg_color,
        &mut config.external_container_queue_bg_color,
        "#3F3F46",
    );
    set_c4_shape_group(
        &mut config.external_container_border_color,
        &mut config.external_container_db_border_color,
        &mut config.external_container_queue_border_color,
        "#A1A1AA",
    );
    set_c4_shape_group(
        &mut config.external_component_bg_color,
        &mut config.external_component_db_bg_color,
        &mut config.external_component_queue_bg_color,
        "#3F3F46",
    );
    set_c4_shape_group(
        &mut config.external_component_border_color,
        &mut config.external_component_db_border_color,
        &mut config.external_component_queue_border_color,
        "#A1A1AA",
    );
}

fn set_c4_shape_group(first: &mut String, second: &mut String, third: &mut String, color: &str) {
    *first = color.to_owned();
    *second = color.to_owned();
    *third = color.to_owned();
}

fn c4_fill(config: &C4Config, kind: C4ShapeKind) -> &str {
    match kind {
        C4ShapeKind::Person => &config.person_bg_color,
        C4ShapeKind::ExternalPerson => &config.external_person_bg_color,
        C4ShapeKind::System => &config.system_bg_color,
        C4ShapeKind::SystemDb => &config.system_db_bg_color,
        C4ShapeKind::SystemQueue => &config.system_queue_bg_color,
        C4ShapeKind::ExternalSystem => &config.external_system_bg_color,
        C4ShapeKind::ExternalSystemDb => &config.external_system_db_bg_color,
        C4ShapeKind::ExternalSystemQueue => &config.external_system_queue_bg_color,
        C4ShapeKind::Container => &config.container_bg_color,
        C4ShapeKind::ContainerDb => &config.container_db_bg_color,
        C4ShapeKind::ContainerQueue => &config.container_queue_bg_color,
        C4ShapeKind::ExternalContainer => &config.external_container_bg_color,
        C4ShapeKind::ExternalContainerDb => &config.external_container_db_bg_color,
        C4ShapeKind::ExternalContainerQueue => &config.external_container_queue_bg_color,
        C4ShapeKind::Component => &config.component_bg_color,
        C4ShapeKind::ComponentDb => &config.component_db_bg_color,
        C4ShapeKind::ComponentQueue => &config.component_queue_bg_color,
        C4ShapeKind::ExternalComponent => &config.external_component_bg_color,
        C4ShapeKind::ExternalComponentDb => &config.external_component_db_bg_color,
        C4ShapeKind::ExternalComponentQueue => &config.external_component_queue_bg_color,
    }
}

fn c4_border(config: &C4Config, kind: C4ShapeKind) -> &str {
    match kind {
        C4ShapeKind::Person => &config.person_border_color,
        C4ShapeKind::ExternalPerson => &config.external_person_border_color,
        C4ShapeKind::System => &config.system_border_color,
        C4ShapeKind::SystemDb => &config.system_db_border_color,
        C4ShapeKind::SystemQueue => &config.system_queue_border_color,
        C4ShapeKind::ExternalSystem => &config.external_system_border_color,
        C4ShapeKind::ExternalSystemDb => &config.external_system_db_border_color,
        C4ShapeKind::ExternalSystemQueue => &config.external_system_queue_border_color,
        C4ShapeKind::Container => &config.container_border_color,
        C4ShapeKind::ContainerDb => &config.container_db_border_color,
        C4ShapeKind::ContainerQueue => &config.container_queue_border_color,
        C4ShapeKind::ExternalContainer => &config.external_container_border_color,
        C4ShapeKind::ExternalContainerDb => &config.external_container_db_border_color,
        C4ShapeKind::ExternalContainerQueue => &config.external_container_queue_border_color,
        C4ShapeKind::Component => &config.component_border_color,
        C4ShapeKind::ComponentDb => &config.component_db_border_color,
        C4ShapeKind::ComponentQueue => &config.component_queue_border_color,
        C4ShapeKind::ExternalComponent => &config.external_component_border_color,
        C4ShapeKind::ExternalComponentDb => &config.external_component_db_border_color,
        C4ShapeKind::ExternalComponentQueue => &config.external_component_queue_border_color,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_palette_meets_wcag_contrast_targets() {
        let theme = theme();
        let config = layout_config();
        assert_pair(
            &theme.primary_text_color,
            &theme.primary_color,
            TEXT_CONTRAST,
        );
        assert_pair(
            &theme.primary_text_color,
            &theme.sequence_note_fill,
            TEXT_CONTRAST,
        );
        assert_pair(
            &config.requirement.label_color,
            &config.requirement.fill,
            TEXT_CONTRAST,
        );
        assert_pair(
            &config.requirement.edge_label_color,
            &config.requirement.edge_label_background,
            TEXT_CONTRAST,
        );
        for graphic in [
            &theme.primary_border_color,
            &theme.line_color,
            &theme.cluster_border,
            &config.requirement.box_stroke,
            &config.requirement.edge_stroke,
        ] {
            assert_pair(graphic, CANVAS, GRAPHIC_CONTRAST);
        }
        for fill in &theme.pie_colors {
            assert_pair(&theme.pie_section_text_color, fill, TEXT_CONTRAST);
        }
        for (fill, text) in theme
            .git_colors
            .iter()
            .zip(theme.git_branch_label_colors.iter())
        {
            assert_pair(text, fill, TEXT_CONTRAST);
            assert_pair(fill, CANVAS, GRAPHIC_CONTRAST);
        }
        for (fill, text) in config
            .mindmap
            .section_colors
            .iter()
            .zip(config.mindmap.section_label_colors.iter())
        {
            assert_pair(text, fill, TEXT_CONTRAST);
        }
        for line in &config.mindmap.section_line_colors {
            assert_pair(line, CANVAS, GRAPHIC_CONTRAST);
        }
        for kind in [
            C4ShapeKind::Person,
            C4ShapeKind::ExternalPerson,
            C4ShapeKind::System,
            C4ShapeKind::SystemDb,
            C4ShapeKind::SystemQueue,
            C4ShapeKind::ExternalSystem,
            C4ShapeKind::ExternalSystemDb,
            C4ShapeKind::ExternalSystemQueue,
            C4ShapeKind::Container,
            C4ShapeKind::ContainerDb,
            C4ShapeKind::ContainerQueue,
            C4ShapeKind::ExternalContainer,
            C4ShapeKind::ExternalContainerDb,
            C4ShapeKind::ExternalContainerQueue,
            C4ShapeKind::Component,
            C4ShapeKind::ComponentDb,
            C4ShapeKind::ComponentQueue,
            C4ShapeKind::ExternalComponent,
            C4ShapeKind::ExternalComponentDb,
            C4ShapeKind::ExternalComponentQueue,
        ] {
            assert_pair(LIGHT_TEXT, c4_fill(&config.c4, kind), TEXT_CONTRAST);
            assert_pair(c4_border(&config.c4, kind), CANVAS, GRAPHIC_CONTRAST);
        }
    }

    #[test]
    fn layered_node_and_edge_overrides_are_corrected() {
        let mut parsed = mermaid_rs_renderer::parse_mermaid(
            "flowchart LR\nA[Light] -->|label| B[Dark]\nclassDef warning fill:#ffcc00,color:#ffff00\nclass A warning\nstyle A fill:#ffffff\nstyle B fill:#000000,color:#111111,stroke:#000000\nlinkStyle 0 stroke:#111111,color:#111111",
        )
        .expect("diagram parses");
        let theme = theme();
        let config = layout_config();
        enforce_contrast(&mut parsed.graph, &theme, &config);

        let layout = mermaid_rs_renderer::compute_layout(&parsed.graph, &theme, &config);
        let light = &layout.nodes["A"].style;
        let dark = &layout.nodes["B"].style;
        assert_eq!(light.text_color.as_deref(), Some(DARK_TEXT));
        assert_eq!(dark.text_color.as_deref(), Some(LIGHT_TEXT));
        assert_eq!(dark.stroke.as_deref(), Some(GRAPHIC));
        assert_eq!(
            layout.edges[0].override_style.stroke.as_deref(),
            Some(GRAPHIC)
        );
        assert_eq!(
            layout.edges[0].override_style.label_color.as_deref(),
            Some(LIGHT_TEXT)
        );
    }

    #[test]
    fn c4_overrides_are_corrected_against_their_own_fill() {
        let mut parsed = mermaid_rs_renderer::parse_mermaid(
            "C4Context\nPerson(admin, \"Admin\")\nSystem(sys, \"System\")\nRel(admin, sys, \"Uses\")",
        )
        .expect("C4 diagram parses");
        let admin = parsed
            .graph
            .c4
            .shapes
            .iter_mut()
            .find(|shape| shape.id == "admin")
            .expect("admin shape exists");
        admin.bg_color = Some("#ffffff".to_owned());
        admin.font_color = Some("#eeeeee".to_owned());
        let theme = theme();
        let config = layout_config();
        enforce_contrast(&mut parsed.graph, &theme, &config);

        let admin = parsed
            .graph
            .c4
            .shapes
            .iter()
            .find(|shape| shape.id == "admin")
            .expect("admin shape exists");
        assert_eq!(admin.font_color.as_deref(), Some(DARK_TEXT));
    }

    fn assert_pair(foreground: &str, background: &str, minimum: f32) {
        let background = color_over_canvas(background).expect("background color parses");
        let ratio = contrast_against(foreground, background).expect("foreground color parses");
        assert!(
            ratio >= minimum,
            "{foreground} on {background:?} has {ratio:.2}:1 contrast, expected {minimum:.1}:1"
        );
    }
}
