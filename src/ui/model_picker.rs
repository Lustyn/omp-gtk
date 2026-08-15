use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk4 as gtk;

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

#[derive(Clone)]
pub(crate) struct ModelPickerView {
    root: gtk::Box,
    search: gtk::SearchEntry,
}

impl ModelPickerView {
    pub(crate) fn new(
        mut models: Vec<ModelSummary>,
        selected: Option<(String, String)>,
        on_select: impl Fn(ModelSummary) + 'static,
        on_close: impl Fn() + 'static,
    ) -> Self {
        models.sort_by(|left, right| {
            provider_name(left)
                .cmp(&provider_name(right))
                .then_with(|| left.display_name().cmp(right.display_name()))
        });

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.set_size_request(720, 600);
        root.set_accessible_role(gtk::AccessibleRole::Dialog);
        root.update_property(&[gtk::accessible::Property::Label("Choose a model")]);
        root.add_css_class("model-picker");

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        header.add_css_class("model-picker-header");
        let heading_copy = gtk::Box::new(gtk::Orientation::Vertical, 2);
        heading_copy.set_hexpand(true);
        let heading = gtk::Label::new(Some("Choose a model"));
        heading.set_xalign(0.0);
        heading.add_css_class("model-picker-heading");
        let subtitle = gtk::Label::new(Some("Pick the model that best fits your task."));
        subtitle.set_xalign(0.0);
        subtitle.add_css_class("model-picker-subtitle");
        heading_copy.append(&heading);
        heading_copy.append(&subtitle);
        let close = icons::icon_button(icons::Icon::X, "Close model picker");
        close.add_css_class("model-picker-close");
        header.append(&heading_copy);
        header.append(&close);
        root.append(&header);

        let search = gtk::SearchEntry::new();
        search.update_property(&[gtk::accessible::Property::Label("Search models")]);
        search.set_placeholder_text(Some("Search by model or provider"));
        search.add_css_class("model-picker-search");
        root.append(&search);

        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        list.add_css_class("model-picker-list");
        list.set_placeholder(Some(&empty_state()));

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

        let mut providers = rows
            .iter()
            .map(|entry| provider_name(&entry.model))
            .collect::<Vec<_>>();
        providers.sort();
        providers.dedup();
        let providers = Rc::new(providers);
        let mut provider_choices = vec!["All providers".to_owned()];
        provider_choices.extend(providers.iter().cloned());
        let provider_refs = provider_choices
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let provider_filter = gtk::DropDown::from_strings(&provider_refs);
        provider_filter.set_enable_search(true);
        provider_filter.update_property(&[gtk::accessible::Property::Label("Filter by provider")]);
        provider_filter.add_css_class("model-filter-dropdown");

        let context_filter = gtk::DropDown::from_strings(&[
            "Any context",
            "Up to 128K",
            "128K–256K",
            "More than 256K",
        ]);
        context_filter
            .update_property(&[gtk::accessible::Property::Label("Filter by context size")]);
        context_filter.add_css_class("model-filter-dropdown");

        let result_count = gtk::Label::new(None);
        result_count.set_xalign(1.0);
        result_count.set_hexpand(true);
        result_count.add_css_class("model-picker-result-count");
        let clear = gtk::Button::with_label("Clear filters");
        clear.add_css_class("model-picker-clear");
        clear.set_visible(false);

        let filters_bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        filters_bar.add_css_class("model-picker-filters");
        filters_bar.append(&provider_filter);
        filters_bar.append(&context_filter);
        filters_bar.append(&result_count);
        filters_bar.append(&clear);
        root.append(&filters_bar);

        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&list)
            .build();
        scroll.add_css_class("model-picker-scroll");
        root.append(&scroll);

        close.connect_clicked(move |_| on_close());

        let rows_for_search = rows.clone();
        let filters_for_search = filters.clone();
        let count_for_search = result_count.clone();
        let clear_for_search = clear.clone();
        search.connect_search_changed(move |entry| {
            filters_for_search.borrow_mut().query = entry.text().trim().to_ascii_lowercase();
            update_results(
                &rows_for_search,
                &filters_for_search.borrow(),
                &count_for_search,
                &clear_for_search,
            );
        });

        let rows_for_provider = rows.clone();
        let filters_for_provider = filters.clone();
        let providers_for_filter = providers.clone();
        let count_for_provider = result_count.clone();
        let clear_for_provider = clear.clone();
        provider_filter.connect_selected_notify(move |dropdown| {
            filters_for_provider.borrow_mut().provider = dropdown
                .selected()
                .checked_sub(1)
                .and_then(|index| providers_for_filter.get(index as usize).cloned());
            update_results(
                &rows_for_provider,
                &filters_for_provider.borrow(),
                &count_for_provider,
                &clear_for_provider,
            );
        });

        let rows_for_context = rows.clone();
        let filters_for_context = filters.clone();
        let count_for_context = result_count.clone();
        let clear_for_context = clear.clone();
        context_filter.connect_selected_notify(move |dropdown| {
            filters_for_context.borrow_mut().context = match dropdown.selected() {
                1 => ContextBand::UpTo128k,
                2 => ContextBand::UpTo256k,
                3 => ContextBand::Above256k,
                _ => ContextBand::Any,
            };
            update_results(
                &rows_for_context,
                &filters_for_context.borrow(),
                &count_for_context,
                &clear_for_context,
            );
        });

        let search_for_clear = search.clone();
        let provider_for_clear = provider_filter.clone();
        let context_for_clear = context_filter.clone();
        let rows_for_clear = rows.clone();
        let filters_for_clear = filters.clone();
        let count_for_clear = result_count.clone();
        let clear_for_clear = clear.clone();
        clear.connect_clicked(move |_| {
            filters_for_clear.replace(Filters::default());
            search_for_clear.set_text("");
            provider_for_clear.set_selected(0);
            context_for_clear.set_selected(0);
            update_results(
                &rows_for_clear,
                &filters_for_clear.borrow(),
                &count_for_clear,
                &clear_for_clear,
            );
        });

        let on_select = Rc::new(on_select);
        for entry in rows.iter() {
            let model = entry.model.clone();
            let on_select = on_select.clone();
            entry
                .button
                .connect_clicked(move |_| on_select(model.clone()));
        }
        let rows_for_activate = rows.clone();
        search.connect_activate(move |_| {
            if let Some(entry) = rows_for_activate
                .iter()
                .find(|entry| entry.row.is_visible())
            {
                entry.button.emit_clicked();
            }
        });

        update_results(&rows, &filters.borrow(), &result_count, &clear);
        Self { root, search }
    }

    pub(crate) fn widget(&self) -> &gtk::Widget {
        self.root.upcast_ref()
    }

    pub(crate) fn focus_search(&self) {
        self.search.grab_focus();
    }
}

fn provider_name(model: &ModelSummary) -> String {
    icons::provider_label(&model.provider)
}

fn empty_state() -> gtk::Widget {
    let empty = gtk::Box::new(gtk::Orientation::Vertical, 6);
    empty.set_halign(gtk::Align::Center);
    empty.set_valign(gtk::Align::Center);
    empty.add_css_class("model-picker-empty");
    let icon = icons::icon(icons::Icon::SearchX, 24);
    icon.add_css_class("model-picker-empty-icon");
    let title = gtk::Label::new(Some("No models found"));
    title.add_css_class("model-picker-empty-title");
    let detail = gtk::Label::new(Some("Try a different search or clear the filters."));
    detail.add_css_class("model-picker-empty-detail");
    empty.append(&icon);
    empty.append(&title);
    empty.append(&detail);
    empty.upcast()
}

fn update_results(
    rows: &[PickerRow],
    filters: &Filters,
    result_count: &gtk::Label,
    clear: &gtk::Button,
) {
    let mut visible = 0;
    for entry in rows {
        let matches = model_matches(&entry.model, filters);
        entry.row.set_visible(matches);
        visible += usize::from(matches);
    }
    result_count.set_text(&if visible == 1 {
        "1 model".to_owned()
    } else {
        format!("{visible} models")
    });
    clear.set_visible(
        !filters.query.is_empty()
            || filters.provider.is_some()
            || filters.context != ContextBand::Any,
    );
}

fn model_matches(model: &ModelSummary, filters: &Filters) -> bool {
    let provider = provider_name(model);
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
    let provider = provider_name(model);
    let mut details = vec![provider.clone()];
    if let Some(context) = context_label(model.context_window) {
        details.push(context);
    }
    if model
        .thinking
        .as_ref()
        .is_some_and(|thinking| !thinking.efforts.is_empty())
    {
        details.push("Reasoning".to_owned());
    }
    let detail = gtk::Label::new(Some(&details.join(" · ")));
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

fn context_label(window: Option<u64>) -> Option<String> {
    window.map(|window| format!("{} context", format_tokens(window)))
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
