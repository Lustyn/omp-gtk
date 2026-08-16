use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::prelude::*;
use gtk::{gdk, glib};
use gtk4 as gtk;
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use super::icons;

#[derive(Clone, Copy)]
pub enum MessageRole {
    User,
    Assistant,
}

#[derive(Clone)]
pub struct ThinkingBlock {
    pub root: gtk::Box,
    summary: gtk::Label,
    body: gtk::Label,
    title: gtk::Label,
    active: Rc<Cell<bool>>,
    text: Rc<RefCell<String>>,
    summary_text: Rc<RefCell<String>>,
}

#[derive(Clone)]
pub struct ChatStatus {
    pub root: gtk::Box,
    dot: gtk::Box,
    label: gtk::Label,
    activity: Rc<RefCell<String>>,
    busy: Rc<Cell<bool>>,
    started: Rc<Cell<Instant>>,
}

#[derive(Clone)]
pub struct TelemetryWidgets {
    pub root: gtk::Box,
    pub cwd: gtk::Label,
    pub cwd_button: gtk::Button,
    pub context: gtk::Label,
    pub context_progress: gtk::ProgressBar,
    pub cost: gtk::Label,
    pub throughput: gtk::Label,
}

impl ThinkingBlock {
    pub fn new(text: &str, streaming: bool) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("thinking-row");
        if streaming {
            root.add_css_class("thinking-active");
        }
        let expander = gtk::Expander::new(None);
        expander.set_tooltip_text(Some("Click to expand reasoning"));
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        header.set_margin_top(10);
        header.set_margin_bottom(10);
        header.set_margin_start(12);
        header.set_margin_end(12);
        let icon = icons::icon(icons::Icon::BrainCircuit, 16);
        icon.add_css_class("thinking-icon");
        let labels = gtk::Box::new(gtk::Orientation::Vertical, 2);
        labels.set_hexpand(true);
        let title = gtk::Label::new(Some("Thinking"));
        title.set_xalign(0.0);
        title.add_css_class("thinking-title");
        let summary_text = Rc::new(RefCell::new(thinking_summary(text)));
        let summary = gtk::Label::new(Some(&summary_text.borrow()));
        summary.set_xalign(0.0);
        summary.set_ellipsize(gtk::pango::EllipsizeMode::End);
        summary.add_css_class("thinking-summary");
        labels.append(&title);
        labels.append(&summary);
        header.append(&icon);
        header.append(&labels);
        expander.set_label_widget(Some(&header));

        let body = read_only_label(text, "thinking-body");
        body.set_margin_bottom(12);
        body.set_margin_start(14);
        body.set_margin_end(14);
        expander.set_child(Some(&body));
        root.append(&expander);
        wire_copy_menu(&root, &body, "Copy thinking");

        let block = Self {
            root,
            summary,
            body,
            title,
            active: Rc::new(Cell::new(streaming)),
            text: Rc::new(RefCell::new(text.to_owned())),
            summary_text,
        };
        if streaming {
            block.start_animation();
        }
        block
    }

    pub fn append(&self, delta: &str) {
        if delta.is_empty() {
            return;
        }
        let mut text = self.text.borrow_mut();
        text.push_str(delta);
        self.body.set_text(&text);
        let summary = thinking_summary(&text);
        self.summary_text.replace(summary.clone());
        self.summary.set_text(&summary);
    }

    pub fn finish(&self, final_text: Option<&str>) {
        if let Some(final_text) = final_text.filter(|text| !text.is_empty()) {
            self.text.replace(final_text.to_owned());
            self.body.set_text(final_text);
            self.summary_text.replace(thinking_summary(final_text));
        }
        self.active.set(false);
        self.title.set_text("Thinking");
        self.summary.set_text(&self.summary_text.borrow());
        self.root.remove_css_class("thinking-active");
    }

    fn start_animation(&self) {
        let title = self.title.downgrade();
        let summary = self.summary.downgrade();
        let summary_text = self.summary_text.clone();
        let active = self.active.clone();
        let started = Instant::now();
        self.title
            .set_markup(&shimmer_markup("Thinking", Duration::ZERO));
        self.summary
            .set_markup(&shimmer_markup(&summary_text.borrow(), Duration::ZERO));
        glib::timeout_add_local(Duration::from_millis(33), move || {
            let (Some(title), Some(summary)) = (title.upgrade(), summary.upgrade()) else {
                return glib::ControlFlow::Break;
            };
            if !active.get() {
                return glib::ControlFlow::Break;
            }
            let elapsed = started.elapsed();
            title.set_markup(&shimmer_markup("Thinking", elapsed));
            summary.set_markup(&shimmer_markup(&summary_text.borrow(), elapsed));
            glib::ControlFlow::Continue
        });
    }
}

impl ChatStatus {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 7);
        root.add_css_class("chat-status");
        let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        dot.add_css_class("status-dot");
        dot.set_halign(gtk::Align::Center);
        dot.set_valign(gtk::Align::Center);
        dot.set_size_request(7, 7);
        let label = gtk::Label::new(Some("Connecting to runtime"));
        label.set_max_width_chars(20);
        label.set_xalign(0.0);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.add_css_class("chat-status-label");
        root.append(&dot);
        root.append(&label);

        let status = Self {
            root,
            dot,
            label,
            activity: Rc::new(RefCell::new("Connecting to runtime".to_owned())),
            busy: Rc::new(Cell::new(false)),
            started: Rc::new(Cell::new(Instant::now())),
        };
        status.start_animation();
        status
    }

    pub fn idle(&self) {
        self.activity.replace("Ready".to_owned());
        self.busy.set(false);
        self.label.set_text("Ready");
        self.dot.set_opacity(1.0);
        self.root.remove_css_class("chat-status-busy");
        self.root.remove_css_class("chat-status-offline");
    }

    pub fn disconnected(&self) {
        self.activity.replace("Offline".to_owned());
        self.busy.set(false);
        self.label.set_text("Offline");
        self.dot.set_opacity(1.0);
        self.root.remove_css_class("chat-status-busy");
        self.root.add_css_class("chat-status-offline");
    }

    pub fn activity(&self, text: &str) {
        self.activity.replace(text.to_owned());
        self.busy.set(true);
        self.started.set(Instant::now());
        self.label.set_markup(&shimmer_markup(text, Duration::ZERO));
        self.dot.set_opacity(0.0);
        self.root.remove_css_class("chat-status-offline");
        self.root.add_css_class("chat-status-busy");
    }

    fn start_animation(&self) {
        let label = self.label.downgrade();
        let activity = self.activity.clone();
        let busy = self.busy.clone();
        let started = self.started.clone();
        glib::timeout_add_local(Duration::from_millis(33), move || {
            let Some(label) = label.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if busy.get() {
                label.set_markup(&shimmer_markup(&activity.borrow(), started.get().elapsed()));
            }
            glib::ControlFlow::Continue
        });
    }
}

impl TelemetryWidgets {
    pub fn new(cwd_text: &str) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        root.add_css_class("telemetry-strip");
        root.set_valign(gtk::Align::Center);
        let (cwd_box, cwd) =
            telemetry_item(icons::Icon::Folder, cwd_text, "Current project directory");
        cwd_box.set_hexpand(true);
        cwd.set_hexpand(true);
        cwd.set_xalign(0.0);
        cwd.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        let cwd_button = gtk::Button::builder().child(&cwd_box).build();
        cwd_button.set_hexpand(true);
        cwd_button.set_halign(gtk::Align::Start);
        cwd_button.update_property(&[gtk::accessible::Property::Label("Change workspace")]);
        cwd_button.set_tooltip_text(Some("Change workspace"));
        cwd_button.add_css_class("flat");
        cwd_button.add_css_class("telemetry-workspace-button");
        let (context_box, context) = telemetry_item(
            icons::Icon::Gauge,
            "Context unavailable",
            "Current context usage",
        );
        let context_progress = gtk::ProgressBar::new();
        context_progress.set_fraction(0.0);
        context_progress.set_size_request(64, 4);
        context_progress.set_valign(gtk::Align::Center);
        context_progress.add_css_class("context-progress");
        let (cost_box, cost) = telemetry_item(icons::Icon::Coins, "$0.000", "Session cost");
        let (throughput_box, throughput) =
            telemetry_item(icons::Icon::Zap, "Idle", "Generation throughput");
        root.append(&cwd_button);
        root.append(&context_box);
        root.append(&context_progress);
        root.append(&cost_box);
        root.append(&throughput_box);
        Self {
            root,
            cwd,
            cwd_button,
            context,
            context_progress,
            cost,
            throughput,
        }
    }

    pub fn set_context(&self, tokens: u64, window: u64, percent: f64) {
        self.context.set_text(&format!(
            "{} / {} · {:.0}%",
            format_tokens(tokens),
            format_tokens(window),
            percent
        ));
        self.context_progress
            .set_fraction((percent / 100.0).clamp(0.0, 1.0));
        self.context_progress.remove_css_class("context-warning");
        self.context_progress.remove_css_class("context-danger");
        if percent >= 85.0 {
            self.context_progress.add_css_class("context-danger");
        } else if percent >= 65.0 {
            self.context_progress.add_css_class("context-warning");
        }
    }

    pub fn set_cost(&self, cost: f64) {
        self.cost.set_text(&format!("${cost:.3}"));
    }

    pub fn set_throughput(&self, tokens_per_second: Option<f64>) {
        self.throughput.set_text(
            &tokens_per_second
                .filter(|value| value.is_finite() && *value > 0.0)
                .map(|value| format!("{value:.1} tok/s"))
                .unwrap_or_else(|| "Idle".to_owned()),
        );
    }
}
struct ListState {
    next_number: Option<u64>,
    first_item: bool,
}

struct MarkdownRenderer {
    markup: String,
    lists: Vec<ListState>,
    links: Vec<bool>,
    line_breaks: usize,
    after_block_marker: bool,
    quote_depth: usize,
    table_cell: usize,
    table_rows: usize,
}

impl MarkdownRenderer {
    fn new() -> Self {
        Self {
            markup: String::new(),
            lists: Vec::new(),
            links: Vec::new(),
            line_breaks: 0,
            after_block_marker: false,
            quote_depth: 0,
            table_cell: 0,
            table_rows: 0,
        }
    }

    fn render(mut self, event: Event<'_>) -> Self {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.text(&text),
            Event::Code(code) => {
                self.tag("<span font_family=\"monospace\" background=\"#20242c\">");
                self.text(&code);
                self.tag("</span>");
            }
            Event::InlineMath(math) => {
                self.tag("<span font_family=\"monospace\">");
                self.text(&math);
                self.tag("</span>");
            }
            Event::DisplayMath(math) => {
                self.block();
                self.tag("<span font_family=\"monospace\">");
                self.text(&math);
                self.tag("</span>");
            }
            Event::Html(html) | Event::InlineHtml(html) => self.text(&html),
            Event::FootnoteReference(name) => {
                self.text("[");
                self.text(&name);
                self.text("]");
            }
            Event::SoftBreak => self.text(" "),
            Event::HardBreak => self.breaks(1),
            Event::Rule => {
                self.block();
                self.tag("<span foreground=\"#565d69\">────────────────────────</span>");
            }
            Event::TaskListMarker(checked) => self.text(if checked { "☑ " } else { "☐ " }),
        }
        self
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {
                if self.after_block_marker {
                    self.after_block_marker = false;
                } else {
                    self.block();
                    self.quote_prefix();
                }
            }
            Tag::Heading { level, .. } => {
                self.block();
                self.quote_prefix();
                let size = match level {
                    HeadingLevel::H1 => "xx-large",
                    HeadingLevel::H2 => "x-large",
                    HeadingLevel::H3 => "large",
                    HeadingLevel::H4 | HeadingLevel::H5 | HeadingLevel::H6 => "medium",
                };
                self.tag(&format!("<span weight=\"bold\" size=\"{size}\">"));
            }
            Tag::BlockQuote(_) => {
                self.block();
                self.quote_depth += 1;
                self.tag("<span foreground=\"#aeb4bf\">");
                self.quote_prefix();
                self.after_block_marker = true;
            }
            Tag::CodeBlock(_) => {
                self.block();
                self.quote_prefix();
                self.tag(
                    "<span font_family=\"monospace\" foreground=\"#e4e8ef\" background=\"#171a20\">",
                );
            }
            Tag::HtmlBlock => self.block(),
            Tag::List(start) => {
                if self.lists.is_empty() {
                    self.block();
                } else {
                    self.breaks(1);
                }
                self.lists.push(ListState {
                    next_number: start,
                    first_item: true,
                });
            }
            Tag::Item => {
                let depth = self.lists.len();
                let (needs_break, marker) = {
                    let state = self.lists.last_mut().expect("list item belongs to a list");
                    let needs_break = !state.first_item;
                    state.first_item = false;
                    let marker = match &mut state.next_number {
                        Some(number) => {
                            let marker = format!("{number}. ");
                            *number += 1;
                            marker
                        }
                        None => "• ".to_owned(),
                    };
                    (needs_break, marker)
                };
                if needs_break {
                    self.breaks(1);
                }
                self.quote_prefix();
                self.text(&"  ".repeat(depth.saturating_sub(1)));
                self.text(&marker);
                self.after_block_marker = true;
            }
            Tag::FootnoteDefinition(name) => {
                self.block();
                self.text("[");
                self.text(&name);
                self.text("] ");
                self.after_block_marker = true;
            }
            Tag::DefinitionList => self.block(),
            Tag::DefinitionListTitle => self.tag("<b>"),
            Tag::DefinitionListDefinition => self.text(" — "),
            Tag::Table(_) => {
                self.block();
                self.table_rows = 0;
                self.tag("<span font_family=\"monospace\">");
            }
            Tag::TableHead => self.tag("<b>"),
            Tag::TableRow => {
                if self.table_rows > 0 {
                    self.breaks(1);
                }
                self.table_rows += 1;
                self.table_cell = 0;
                self.quote_prefix();
            }
            Tag::TableCell => {
                if self.table_cell > 0 {
                    self.text("  │  ");
                }
                self.table_cell += 1;
            }
            Tag::Emphasis => self.tag("<i>"),
            Tag::Strong => self.tag("<b>"),
            Tag::Strikethrough => self.tag("<span strikethrough=\"true\">"),
            Tag::Superscript => self.tag("<sup>"),
            Tag::Subscript => self.tag("<sub>"),
            Tag::Link { dest_url, .. } => {
                let active = safe_link(&dest_url);
                self.links.push(active);
                if active {
                    let destination = glib::markup_escape_text(&dest_url);
                    self.tag(&format!(
                        "<a href=\"{destination}\"><span foreground=\"#8ab4f8\" underline=\"single\">"
                    ));
                } else {
                    self.tag("<span foreground=\"#8ab4f8\" underline=\"single\">");
                }
            }
            Tag::Image { .. } => {
                self.tag("<span foreground=\"#9299a6\"><i>Image: ");
            }
            Tag::MetadataBlock(_) => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::Item | TagEnd::HtmlBlock => {}
            TagEnd::Heading(_) => self.tag("</span>"),
            TagEnd::BlockQuote(_) => {
                self.tag("</span>");
                self.quote_depth = self.quote_depth.saturating_sub(1);
            }
            TagEnd::CodeBlock => self.tag("</span>"),
            TagEnd::List(_) => {
                self.lists.pop();
            }
            TagEnd::FootnoteDefinition => {}
            TagEnd::DefinitionList => {}
            TagEnd::DefinitionListTitle => self.tag("</b>"),
            TagEnd::DefinitionListDefinition => {}
            TagEnd::Table => self.tag("</span>"),
            TagEnd::TableHead => self.tag("</b>"),
            TagEnd::TableRow | TagEnd::TableCell => {}
            TagEnd::Emphasis => self.tag("</i>"),
            TagEnd::Strong => self.tag("</b>"),
            TagEnd::Strikethrough => self.tag("</span>"),
            TagEnd::Superscript => self.tag("</sup>"),
            TagEnd::Subscript => self.tag("</sub>"),
            TagEnd::Link => {
                if self.links.pop().unwrap_or(false) {
                    self.tag("</span></a>");
                } else {
                    self.tag("</span>");
                }
            }
            TagEnd::Image => self.tag("</i></span>"),
            TagEnd::MetadataBlock(_) => {}
        }
    }

    fn block(&mut self) {
        if !self.markup.is_empty() {
            self.breaks(2);
        }
    }

    fn quote_prefix(&mut self) {
        if self.quote_depth > 0 {
            self.text(&"│ ".repeat(self.quote_depth));
        }
    }

    fn breaks(&mut self, count: usize) {
        while self.line_breaks < count {
            self.markup.push('\n');
            self.line_breaks += 1;
        }
    }

    fn tag(&mut self, markup: &str) {
        self.markup.push_str(markup);
    }

    fn text(&mut self, text: &str) {
        self.markup.push_str(&glib::markup_escape_text(text));
        self.line_breaks = text
            .chars()
            .rev()
            .take_while(|character| *character == '\n')
            .count();
    }
}

fn markdown_markup(markdown: &str) -> String {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_SMART_PUNCTUATION;
    Parser::new_ext(markdown, options)
        .fold(MarkdownRenderer::new(), MarkdownRenderer::render)
        .markup
}

fn safe_link(destination: &str) -> bool {
    url::Url::parse(destination)
        .map(|url| matches!(url.scheme(), "http" | "https" | "mailto"))
        .unwrap_or(false)
}

#[derive(Clone)]
pub(crate) struct MessageBody {
    label: gtk::Label,
    source: Rc<RefCell<String>>,
}

impl MessageBody {
    fn new(text: &str) -> Self {
        let body = Self {
            label: read_only_label("", "message-body"),
            source: Rc::new(RefCell::new(String::new())),
        };
        body.set_text(text);
        body
    }

    pub(crate) fn set_text(&self, text: &str) {
        let mut source = self.source.borrow_mut();
        source.clear();
        source.push_str(text);
        drop(source);
        self.label.set_markup(&markdown_markup(text));
    }
}

pub fn append_message(messages: &gtk::Box, role: MessageRole, text: &str) -> MessageBody {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 7);
    row.add_css_class("message-row");
    row.add_css_class(match role {
        MessageRole::User => "user-message",
        MessageRole::Assistant => "assistant-message",
    });

    let author = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    author.add_css_class("message-author");
    match role {
        MessageRole::User => {
            author.append(&icons::icon(icons::Icon::UserRound, 14));
            author.append(&gtk::Label::new(Some("You")));
        }
        MessageRole::Assistant => {
            author.append(&icons::omp_logo(16));
            author.append(&gtk::Label::new(Some("omp")));
        }
    }

    let body = MessageBody::new(text);
    row.append(&author);
    row.append(&body.label);
    messages.append(&row);
    wire_message_menu(&row, &body, role);
    body
}

pub fn append_thinking(messages: &gtk::Box, text: &str, streaming: bool) -> ThinkingBlock {
    let block = ThinkingBlock::new(text, streaming);
    messages.append(&block.root);
    block
}

pub fn append_notice(messages: &gtk::Box, text: &str, error: bool) -> gtk::Box {
    let notice = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    notice.add_css_class("notice");
    if error {
        notice.add_css_class("notice-error");
    }
    let icon = icons::icon(
        if error {
            icons::Icon::TriangleAlert
        } else {
            icons::Icon::Info
        },
        15,
    );
    let label = read_only_label(text, "notice-text");
    notice.append(&icon);
    notice.append(&label);
    messages.append(&notice);
    wire_copy_menu(&notice, &label, "Copy notice");
    notice
}

pub fn read_only_label(text: &str, css_class: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_yalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    label.set_selectable(true);
    label.set_focusable(false);
    label.add_css_class(css_class);
    label
}

fn wire_message_menu(row: &gtk::Box, body: &MessageBody, role: MessageRole) {
    let popover = gtk::Popover::builder()
        .has_arrow(true)
        .autohide(true)
        .build();
    popover.add_css_class("context-menu");
    let actions = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let copy = context_button(icons::Icon::Copy, "Copy message");
    let quote = context_button(icons::Icon::TextQuote, "Copy as quote");
    actions.append(&copy);
    actions.append(&quote);
    popover.set_child(Some(&actions));
    popover.set_parent(row);

    let source_for_copy = body.source.clone();
    let popover_for_copy = popover.clone();
    copy.connect_clicked(move |_| {
        set_clipboard(&source_for_copy.borrow());
        popover_for_copy.popdown();
    });
    let source_for_quote = body.source.clone();
    let popover_for_quote = popover.clone();
    quote.connect_clicked(move |_| {
        let prefix = match role {
            MessageRole::User => "> You: ",
            MessageRole::Assistant => "> Assistant: ",
        };
        let quoted = source_for_quote
            .borrow()
            .lines()
            .map(|line| format!("> {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        set_clipboard(&format!("{prefix}\n{quoted}"));
        popover_for_quote.popdown();
    });
    wire_popover_gesture(row, popover);
}

fn wire_copy_menu(root: &gtk::Box, label: &gtk::Label, text: &str) {
    let popover = gtk::Popover::builder()
        .has_arrow(true)
        .autohide(true)
        .build();
    popover.add_css_class("context-menu");
    let copy = context_button(icons::Icon::Copy, text);
    popover.set_child(Some(&copy));
    popover.set_parent(root);
    let label = label.clone();
    let popover_for_copy = popover.clone();
    copy.connect_clicked(move |_| {
        set_clipboard(&label.text());
        popover_for_copy.popdown();
    });
    wire_popover_gesture(root, popover);
}

fn wire_popover_gesture(root: &gtk::Box, popover: gtk::Popover) {
    let gesture = gtk::GestureClick::new();
    gesture.set_button(3);
    gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
    let popover_for_click = popover.clone();
    gesture.connect_pressed(move |_, _, x, y| {
        popover_for_click.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        popover_for_click.popup();
    });
    root.add_controller(gesture);
    root.connect_unrealize(move |_| popover.unparent());
}

fn context_button(icon: icons::Icon, text: &str) -> gtk::Button {
    let button = icons::labeled_button(icon, text);
    button.add_css_class("context-action");
    button
}

fn set_clipboard(text: &str) {
    if let Some(display) = gdk::Display::default() {
        display.clipboard().set_text(text);
    }
}

pub fn empty_chat_hero() -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 9);
    root.set_halign(gtk::Align::Center);
    root.set_valign(gtk::Align::Center);
    root.set_vexpand(true);
    root.set_margin_start(32);
    root.set_margin_end(32);
    root.add_css_class("empty-chat-hero");
    let logo = icons::omp_logo(88);
    logo.add_css_class("empty-chat-logo");
    let title = gtk::Label::new(Some("What should we work on?"));
    title.set_justify(gtk::Justification::Center);
    title.add_css_class("empty-chat-title");
    let detail = gtk::Label::new(Some(
        "Bring a goal, a question, or a complex workflow. omp can plan, delegate, and carry it through.",
    ));
    detail.set_justify(gtk::Justification::Center);
    detail.set_wrap(true);
    detail.set_max_width_chars(58);
    detail.add_css_class("empty-chat-detail");
    let hint = gtk::Label::new(Some("Type / to explore commands"));
    hint.set_halign(gtk::Align::Center);
    hint.add_css_class("empty-chat-hint");
    root.append(&logo);
    root.append(&title);
    root.append(&detail);
    root.append(&hint);
    root
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ShimmerTier {
    Low,
    Mid,
    High,
}

pub(super) fn shimmer_markup(text: &str, elapsed: Duration) -> String {
    const SPEED_CELLS_PER_SECOND: f64 = 30.0;
    const PADDING: f64 = 10.0;
    const BAND_HALF_WIDTH: f64 = 6.0;
    let length = text.chars().count() as f64;
    let period = length + PADDING * 2.0;
    let position = (elapsed.as_secs_f64() * SPEED_CELLS_PER_SECOND) % period;
    let mut output = String::with_capacity(text.len() * 2);
    let mut current = None;
    for (index, character) in text.chars().enumerate() {
        let distance = (index as f64 + PADDING - position).abs();
        let intensity = if distance >= BAND_HALF_WIDTH {
            0.0
        } else {
            0.5 * (1.0 + (std::f64::consts::PI * distance / BAND_HALF_WIDTH).cos())
        };
        let tier = if intensity >= 0.65 {
            ShimmerTier::High
        } else if intensity >= 0.22 {
            ShimmerTier::Mid
        } else {
            ShimmerTier::Low
        };
        if current != Some(tier) {
            if current.is_some() {
                output.push_str("</span>");
            }
            output.push_str(match tier {
                ShimmerTier::Low => "<span foreground=\"#7f8998\">",
                ShimmerTier::Mid => "<span foreground=\"#aebbd0\">",
                ShimmerTier::High => "<span foreground=\"#ecf4ff\">",
            });
            current = Some(tier);
        }
        push_markup_character(&mut output, character);
    }
    if current.is_some() {
        output.push_str("</span>");
    }
    output
}

fn push_markup_character(output: &mut String, character: char) {
    match character {
        '&' => output.push_str("&amp;"),
        '<' => output.push_str("&lt;"),
        '>' => output.push_str("&gt;"),
        '\'' => output.push_str("&apos;"),
        '"' => output.push_str("&quot;"),
        _ => output.push(character),
    }
}

fn thinking_summary(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .rev()
        .find(|line| !line.is_empty() && !line.starts_with("<!--"))
        .map(|line| {
            let line = line.trim_matches('*').trim();
            let mut output = line.chars().take(90).collect::<String>();
            if line.chars().count() > 90 {
                output.push('…');
            }
            output
        })
        .filter(|line| !line.is_empty())
        .unwrap_or_else(|| "Reasoning in progress".to_owned())
}

fn telemetry_item(icon: icons::Icon, text: &str, tooltip: &str) -> (gtk::Box, gtk::Label) {
    let item = gtk::Box::new(gtk::Orientation::Horizontal, 5);
    item.add_css_class("telemetry-item");
    item.set_valign(gtk::Align::Center);
    item.set_tooltip_text(Some(tooltip));
    let icon = icons::icon(icon, 13);
    let label = gtk::Label::new(Some(text));
    item.append(&icon);
    item.append(&label);
    (item, label)
}

fn format_tokens(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}m", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.0}k", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{markdown_markup, shimmer_markup, thinking_summary};
    use gtk4::pango;
    use std::time::Duration;

    #[test]
    fn markdown_renders_common_chat_content_as_valid_pango_markup() {
        let markup = markdown_markup(
            "# Result\n\nUse **bold**, *italic*, and `code`.\n\n- first\n- [x] done\n\n> quoted",
        );
        let (_, plain_text, _) =
            pango::parse_markup(&markup, '\0').expect("renderer must produce valid Pango markup");

        assert_eq!(
            plain_text,
            "Result\n\nUse bold, italic, and code.\n\n• first\n• ☑ done\n\n│ quoted"
        );
        assert!(markup.contains("weight=\"bold\""));
        assert!(markup.contains("<b>bold</b>"));
        assert!(markup.contains("font_family=\"monospace\""));
    }

    #[test]
    fn markdown_escapes_html_and_only_activates_safe_links() {
        let markup = markdown_markup(
            "[docs](https://example.com/docs) [unsafe](javascript:alert(1)) <button>",
        );
        assert!(markup.contains("href=\"https://example.com/docs\""));
        assert!(!markup.contains("href=\"javascript:"));
        assert!(markup.contains("&lt;button&gt;"));
    }

    #[test]
    fn shimmer_moves_color_without_changing_status_copy() {
        let still = shimmer_markup("Thinking", Duration::ZERO);
        let moving = shimmer_markup("Thinking", Duration::from_millis(333));
        assert_ne!(still, moving);
        assert!(moving.contains("#ecf4ff"));
        assert!(!moving.contains("weight="));
        assert!(!moving.contains("size="));
        assert!(!moving.contains("Thinking."));
    }

    #[test]
    fn thinking_summary_uses_last_explanation_line() {
        assert_eq!(
            thinking_summary("Inspecting the input\n\nComparing both implementations\n"),
            "Comparing both implementations"
        );
        assert_eq!(
            thinking_summary("Useful explanation\n<!-- internal marker -->"),
            "Useful explanation"
        );
    }
}
