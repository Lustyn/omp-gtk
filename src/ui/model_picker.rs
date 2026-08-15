use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;

use super::icons;
use crate::bridge::protocol::ModelSummary;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum ContextBand {
    #[default]
    Any,
    UpTo128k,
    UpTo256k,
    Above256k,
}

#[derive(Default)]
struct Filters {
    query: String,
    provider: Option<String>,
    context: ContextBand,
}

struct PickerRow {
    row: gtk::ListBoxRow,
    button: gtk::Button,
    model: ModelSummary,
}

pub fn present(
    parent: &adw::ApplicationWindow,
    mut models: Vec<ModelSummary>,
    selected: Option<(String, String)>,
    on_select: impl Fn(ModelSummary) + 'static,
) {
    models.sort_by(|left, right| {
        icons::provider_label(&left.provider)
            .cmp(&icons::provider_label(&right.provider))
            .then_with(|| left.display_name().cmp(right.display_name()))
    });

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("model-picker");

    let header = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    header.add_css_class("model-picker-header");
    let heading = gtk::Label::new(Some("Choose a model"));
    heading.set_xalign(0.0);
    heading.set_hexpand(true);
    heading.add_css_class("model-picker-heading");
    let close = icons::icon_button(icons::Icon::X, "Close model picker");
    close.add_css_class("model-picker-close");
    header.append(&heading);
    header.append(&close);
    root.append(&header);

    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Search models and providers"));
    search.add_css_class("model-picker-search");
    root.append(&search);

    let filters_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    filters_box.add_css_class("model-picker-filters");
    let provider_heading = filter_heading("Provider");
    filters_box.append(&provider_heading);
    let provider_flow = gtk::FlowBox::new();
    provider_flow.set_selection_mode(gtk::SelectionMode::None);
    provider_flow.set_row_spacing(6);
    provider_flow.set_column_spacing(6);
    provider_flow.set_max_children_per_line(8);
    filters_box.append(&provider_flow);

    let context_heading = filter_heading("Context size");
    filters_box.append(&context_heading);
    let context_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    filters_box.append(&context_box);
    root.append(&filters_box);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("model-picker-list");
    let empty = gtk::Label::new(Some("No models match these filters."));
    empty.add_css_class("model-picker-empty");
    list.set_placeholder(Some(&empty));

    let rows = models
        .into_iter()
        .map(|model| {
            let is_selected = selected
                .as_ref()
                .is_some_and(|(provider, id)| provider == &model.provider && id == &model.id);
            let (row, button) = model_row(&model, is_selected);
            list.append(&row);
            PickerRow { row, button, model }
        })
        .collect::<Vec<_>>();
    let rows = Rc::new(rows);
    let filters = Rc::new(RefCell::new(Filters::default()));

    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&list)
        .build();
    scroll.add_css_class("model-picker-scroll");
    root.append(&scroll);

    let dialog = adw::Dialog::builder()
        .title("Choose a model")
        .content_width(760)
        .content_height(660)
        .child(&root)
        .build();
    let weak_dialog = dialog.downgrade();
    close.connect_clicked(move |_| {
        if let Some(dialog) = weak_dialog.upgrade() {
            dialog.close();
        }
    });

    let rows_for_search = rows.clone();
    let filters_for_search = filters.clone();
    search.connect_search_changed(move |entry| {
        filters_for_search.borrow_mut().query = entry.text().trim().to_ascii_lowercase();
        apply_filters(&rows_for_search, &filters_for_search.borrow());
    });

    let mut providers = rows
        .iter()
        .map(|entry| icons::provider_label(&entry.model.provider))
        .collect::<Vec<_>>();
    providers.sort();
    providers.dedup();
    let all_providers = filter_button("All providers");
    all_providers.set_active(true);
    provider_flow.insert(&all_providers, -1);
    wire_provider_filter(&all_providers, None, rows.clone(), filters.clone());
    for provider in providers {
        let button = filter_button(&provider);
        button.set_group(Some(&all_providers));
        provider_flow.insert(&button, -1);
        wire_provider_filter(&button, Some(provider), rows.clone(), filters.clone());
    }

    let context_filters = [
        ("Any size", ContextBand::Any),
        ("≤ 128K", ContextBand::UpTo128k),
        ("129–256K", ContextBand::UpTo256k),
        ("257K+", ContextBand::Above256k),
    ];
    let mut context_group = None;
    for (label, band) in context_filters {
        let button = filter_button(label);
        if let Some(group) = context_group.as_ref() {
            button.set_group(Some(group));
        } else {
            button.set_active(true);
            context_group = Some(button.clone());
        }
        let rows = rows.clone();
        let filters = filters.clone();
        button.connect_toggled(move |button| {
            if button.is_active() {
                filters.borrow_mut().context = band;
                apply_filters(&rows, &filters.borrow());
            }
        });
        context_box.append(&button);
    }

    let on_select = Rc::new(on_select);
    for entry in rows.iter() {
        let model = entry.model.clone();
        let on_select = on_select.clone();
        let weak_dialog = dialog.downgrade();
        entry.button.connect_clicked(move |_| {
            on_select(model.clone());
            if let Some(dialog) = weak_dialog.upgrade() {
                dialog.close();
            }
        });
    }

    dialog.present(Some(parent));
    search.grab_focus();
}

fn wire_provider_filter(
    button: &gtk::ToggleButton,
    provider: Option<String>,
    rows: Rc<Vec<PickerRow>>,
    filters: Rc<RefCell<Filters>>,
) {
    button.connect_toggled(move |button| {
        if button.is_active() {
            filters.borrow_mut().provider = provider.clone();
            apply_filters(&rows, &filters.borrow());
        }
    });
}

fn apply_filters(rows: &[PickerRow], filters: &Filters) {
    for entry in rows {
        entry.row.set_visible(model_matches(&entry.model, filters));
    }
}

fn model_matches(model: &ModelSummary, filters: &Filters) -> bool {
    let provider = icons::provider_label(&model.provider);
    if filters
        .provider
        .as_ref()
        .is_some_and(|selected| selected != &provider)
    {
        return false;
    }
    if !context_matches(model.context_window, filters.context) {
        return false;
    }
    if filters.query.is_empty() {
        return true;
    }
    let haystack = format!(
        "{} {} {} {}",
        model.display_name(),
        model.id,
        model.provider,
        provider
    )
    .to_ascii_lowercase();
    filters
        .query
        .split_whitespace()
        .all(|term| haystack.contains(term))
}

fn context_matches(window: Option<u64>, band: ContextBand) -> bool {
    match band {
        ContextBand::Any => true,
        ContextBand::UpTo128k => window.is_some_and(|window| window <= 128_000),
        ContextBand::UpTo256k => window.is_some_and(|window| (128_001..=256_000).contains(&window)),
        ContextBand::Above256k => window.is_some_and(|window| window > 256_000),
    }
}

fn model_row(model: &ModelSummary, selected: bool) -> (gtk::ListBoxRow, gtk::Button) {
    let row = gtk::ListBoxRow::new();
    row.set_activatable(false);
    row.set_selectable(false);
    row.add_css_class("model-picker-row");

    let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    content.set_margin_top(11);
    content.set_margin_bottom(11);
    content.set_margin_start(12);
    content.set_margin_end(12);
    let provider_icon = icons::provider_icon(&model.provider, 24);
    provider_icon
        .root
        .add_css_class("model-picker-provider-icon");
    content.append(&provider_icon.root);

    let text = gtk::Box::new(gtk::Orientation::Vertical, 3);
    text.set_hexpand(true);
    let name = gtk::Label::new(Some(model.display_name()));
    name.set_xalign(0.0);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    name.add_css_class("model-picker-name");
    let provider = icons::provider_label(&model.provider);
    let output_limit = model
        .max_tokens
        .map(|tokens| format!(" · {} max output", format_tokens(tokens)))
        .unwrap_or_default();
    let detail = gtk::Label::new(Some(&format!(
        "{} · {} · {}{}",
        provider,
        model.id,
        context_label(model.context_window),
        output_limit
    )));
    detail.set_xalign(0.0);
    detail.set_ellipsize(gtk::pango::EllipsizeMode::End);
    detail.add_css_class("model-picker-detail");
    text.append(&name);
    text.append(&detail);
    content.append(&text);

    let check = icons::icon(icons::Icon::Check, 17);
    check.set_visible(selected);
    check.add_css_class("model-picker-check");
    content.append(&check);
    let button = gtk::Button::new();
    button.set_child(Some(&content));
    button.add_css_class("model-picker-row-action");
    if selected {
        button.add_css_class("model-picker-row-selected");
    }
    button.set_tooltip_text(Some(&format!(
        "Use {} from {}",
        model.display_name(),
        provider
    )));
    row.set_child(Some(&button));
    (row, button)
}

fn context_label(window: Option<u64>) -> String {
    window.map_or_else(
        || "Context size unknown".to_owned(),
        |window| format!("{} context", format_tokens(window)),
    )
}

fn format_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        let millions = tokens as f64 / 1_000_000.0;
        if tokens.is_multiple_of(1_000_000) {
            format!("{millions:.0}M")
        } else {
            format!("{millions:.1}M")
        }
    } else if tokens >= 1_000 {
        format!("{}K", tokens.div_ceil(1_000))
    } else {
        tokens.to_string()
    }
}

fn filter_heading(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.add_css_class("model-picker-filter-heading");
    label
}

fn filter_button(text: &str) -> gtk::ToggleButton {
    let button = gtk::ToggleButton::with_label(text);
    button.add_css_class("model-filter-chip");
    button
}

#[cfg(test)]
mod tests {
    use super::{ContextBand, Filters, context_matches, model_matches};
    use crate::bridge::protocol::ModelSummary;

    fn model(provider: &str, id: &str, name: &str, context_window: u64) -> ModelSummary {
        ModelSummary {
            provider: provider.to_owned(),
            id: id.to_owned(),
            name: Some(name.to_owned()),
            thinking: None,
            context_window: Some(context_window),
            max_tokens: None,
        }
    }

    #[test]
    fn filters_models_by_friendly_provider_query_and_context_size() {
        let model = model("openai-codex", "gpt-5.6-sol", "GPT-5.6-Sol", 272_000);
        let filters = Filters {
            query: "gpt openai".to_owned(),
            provider: Some("OpenAI".to_owned()),
            context: ContextBand::Above256k,
        };
        assert!(model_matches(&model, &filters));
        assert!(!model_matches(
            &model,
            &Filters {
                provider: Some("Anthropic".to_owned()),
                ..Filters::default()
            }
        ));
        assert!(context_matches(Some(128_000), ContextBand::UpTo128k));
        assert!(context_matches(Some(200_000), ContextBand::UpTo256k));
        assert!(!context_matches(None, ContextBand::Above256k));
    }
}
