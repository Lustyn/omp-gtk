use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::prelude::*;
use gtk::{gdk, glib};
use gtk4 as gtk;
use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratex_layout::{LayoutOptions, layout, to_display_list};
use ratex_parser::parse as parse_latex;
use ratex_svg::{SvgOptions, render_to_svg};
use ratex_types::{Color, MathStyle};

use super::icons;
use super::mermaid;
use super::streaming_markdown::mend_streaming_markdown;

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

#[derive(Clone)]
pub struct ChatHero {
    pub root: gtk::Box,
    logo: gtk::Image,
    title: gtk::Label,
    detail: gtk::Label,
    hint: gtk::Label,
    workspace_choices: gtk::Box,
    state_line: gtk::Box,
    spinner: gtk::Spinner,
    state_dot: gtk::Box,
    state_label: gtk::Label,
    animation_generation: Rc<Cell<u64>>,
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
    pub fn summary(&self) -> String {
        self.summary_text.borrow().clone()
    }
    pub fn is_active(&self) -> bool {
        self.active.get()
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

#[derive(Clone, Debug, PartialEq)]
enum EmbeddedMarkdownContent {
    InlineCode(String),
    CodeBlock(String),
    InlineMath(String),
    DisplayMath(String),
    Table(MarkdownTable),
    HorizontalRule,
    Mermaid(String),
}

#[derive(Clone, Debug, PartialEq)]
struct EmbeddedMarkdown {
    offset: i32,
    content: EmbeddedMarkdownContent,
}

#[derive(Clone, Debug, PartialEq)]
struct MarkdownTable {
    alignments: Vec<Alignment>,
    rows: Vec<Vec<String>>,
    header_rows: usize,
}

struct TableState {
    table: MarkdownTable,
}

impl TableState {
    fn new(alignments: Vec<Alignment>) -> Self {
        Self {
            table: MarkdownTable {
                alignments,
                rows: Vec::new(),
                header_rows: 0,
            },
        }
    }

    fn start_row(&mut self) {
        self.table.rows.push(Vec::new());
    }

    fn start_cell(&mut self) {
        if self.table.rows.is_empty() {
            self.start_row();
        }
        self.table
            .rows
            .last_mut()
            .expect("table row exists")
            .push(String::new());
    }

    fn append(&mut self, text: &str) {
        if self.table.rows.last().is_none_or(|row| row.is_empty()) {
            self.start_cell();
        }
        self.table
            .rows
            .last_mut()
            .and_then(|row| row.last_mut())
            .expect("table cell exists")
            .push_str(text);
    }
}

struct CodeBlockState {
    language: Option<String>,
    text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MarkdownLink {
    start: i32,
    end: i32,
    destination: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MarkdownHeading {
    pub(crate) level: u8,
    pub(crate) offset: i32,
    pub(crate) title: String,
}

struct ActiveHeading {
    level: u8,
    offset: i32,
    title: String,
}

struct RenderedMarkdown {
    markup: String,
    embedded: Vec<EmbeddedMarkdown>,
    links: Vec<MarkdownLink>,
    headings: Vec<MarkdownHeading>,
}

struct MarkdownRenderer {
    markup: String,
    lists: Vec<ListState>,
    active_links: Vec<Option<(i32, String)>>,
    links: Vec<MarkdownLink>,
    embedded: Vec<EmbeddedMarkdown>,
    code_block: Option<CodeBlockState>,
    table: Option<TableState>,
    text_offset: i32,
    line_breaks: usize,
    after_block_marker: bool,
    quote_depth: usize,
    headings: Vec<MarkdownHeading>,
    active_heading: Option<ActiveHeading>,
    explicit_rule_before_next_block: bool,
    seen_heading: bool,
    at_paragraph_start: bool,
    leading_strong: Option<LeadingStrong>,
}

#[derive(Clone, Copy)]
enum LeadingStrong {
    Candidate,
    Attention,
    Plain,
}

impl MarkdownRenderer {
    fn new() -> Self {
        Self {
            markup: String::new(),
            lists: Vec::new(),
            active_links: Vec::new(),
            links: Vec::new(),
            embedded: Vec::new(),
            code_block: None,
            table: None,
            text_offset: 0,
            line_breaks: 0,
            after_block_marker: false,
            quote_depth: 0,
            headings: Vec::new(),
            active_heading: None,
            explicit_rule_before_next_block: false,
            seen_heading: false,
            at_paragraph_start: false,
            leading_strong: None,
        }
    }

    fn render(mut self, event: Event<'_>) -> Self {
        if !matches!(&event, Event::Start(_) | Event::End(_)) {
            self.at_paragraph_start = false;
        }
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => {
                self.capture_heading_text(&text);
                if !self.append_captured(&text) {
                    self.inline_text(&text);
                }
            }
            Event::Code(code) => {
                self.capture_heading_text(&code);
                if !self.append_captured(&code) {
                    self.embed(EmbeddedMarkdownContent::InlineCode(code.into_string()));
                }
            }
            Event::InlineMath(math) => {
                if !self.append_captured(&math) {
                    self.embed(EmbeddedMarkdownContent::InlineMath(math.into_string()));
                }
            }
            Event::DisplayMath(math) => {
                if !self.append_captured(&math) {
                    self.explicit_rule_before_next_block = false;
                    self.block();
                    self.embed(EmbeddedMarkdownContent::DisplayMath(math.into_string()));
                }
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                if !self.append_captured(&html) {
                    self.text(&html);
                }
            }
            Event::FootnoteReference(name) => {
                let reference = format!("[{name}]");
                if !self.append_captured(&reference) {
                    self.text(&reference);
                }
            }
            Event::SoftBreak => {
                if !self.append_captured(" ") {
                    self.text(" ");
                }
            }
            Event::HardBreak => {
                if !self.append_captured("\n") {
                    self.breaks(1);
                }
            }
            Event::Rule => {
                self.block();
                self.embed(EmbeddedMarkdownContent::HorizontalRule);
                self.explicit_rule_before_next_block = true;
            }
            Event::TaskListMarker(checked) => {
                let marker = if checked { "☑ " } else { "☐ " };
                if !self.append_captured(marker) {
                    self.text(marker);
                }
            }
        }
        self
    }

    fn start(&mut self, tag: Tag<'_>) {
        let starts_paragraph = matches!(&tag, Tag::Paragraph);
        let leading_strong =
            matches!(&tag, Tag::Strong) && self.at_paragraph_start && self.table.is_none();
        if !starts_paragraph {
            self.at_paragraph_start = false;
        }
        match tag {
            Tag::Paragraph => {
                self.at_paragraph_start = true;
                self.explicit_rule_before_next_block = false;
                if self.after_block_marker {
                    self.after_block_marker = false;
                } else {
                    self.block();
                    self.quote_prefix();
                }
            }
            Tag::Heading { level, .. } => {
                self.block();
                let follows_explicit_rule =
                    std::mem::take(&mut self.explicit_rule_before_next_block);
                if self.seen_heading && !follows_explicit_rule {
                    self.embed(EmbeddedMarkdownContent::HorizontalRule);
                    self.breaks(1);
                }
                self.seen_heading = true;
                self.quote_prefix();
                self.active_heading = Some(ActiveHeading {
                    level: heading_level(level),
                    offset: self.text_offset,
                    title: String::new(),
                });
                let size = match level {
                    HeadingLevel::H1 => "xx-large",
                    HeadingLevel::H2 => "x-large",
                    HeadingLevel::H3 => "large",
                    HeadingLevel::H4 | HeadingLevel::H5 | HeadingLevel::H6 => "medium",
                };
                self.tag(&format!(
                    "<span weight=\"bold\" size=\"{size}\" foreground=\"#febc38\">"
                ));
            }
            Tag::BlockQuote(_) => {
                self.explicit_rule_before_next_block = false;
                self.block();
                self.quote_depth += 1;
                self.tag("<span foreground=\"#777d88\">");
                self.quote_prefix();
                self.after_block_marker = true;
            }
            Tag::CodeBlock(kind) => {
                self.explicit_rule_before_next_block = false;
                self.block();
                self.quote_prefix();
                let language = match kind {
                    CodeBlockKind::Indented => None,
                    CodeBlockKind::Fenced(info) => info
                        .split_whitespace()
                        .next()
                        .filter(|language| !language.is_empty())
                        .map(|language| language.to_ascii_lowercase()),
                };
                self.code_block = Some(CodeBlockState {
                    language,
                    text: String::new(),
                });
            }
            Tag::HtmlBlock => {
                self.explicit_rule_before_next_block = false;
                self.block();
            }
            Tag::List(start) => {
                self.explicit_rule_before_next_block = false;
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
                self.tag("<span foreground=\"#febc38\">");
                self.text(&marker);
                self.tag("</span>");
                self.after_block_marker = true;
            }
            Tag::FootnoteDefinition(name) => {
                self.explicit_rule_before_next_block = false;
                self.block();
                self.text("[");
                self.text(&name);
                self.text("] ");
                self.after_block_marker = true;
            }
            Tag::DefinitionList => {
                self.explicit_rule_before_next_block = false;
                self.block();
            }
            Tag::DefinitionListTitle => self.tag("<b>"),
            Tag::DefinitionListDefinition => self.text(" — "),
            Tag::Table(alignments) => {
                self.explicit_rule_before_next_block = false;
                self.block();
                self.quote_prefix();
                self.table = Some(TableState::new(alignments));
            }
            Tag::TableHead | Tag::TableRow => {
                if let Some(table) = &mut self.table {
                    table.start_row();
                }
            }
            Tag::TableCell => {
                if let Some(table) = &mut self.table {
                    table.start_cell();
                }
            }
            Tag::Emphasis if self.table.is_none() => self.tag("<i>"),
            Tag::Strong if self.table.is_none() => {
                self.tag("<b>");
                if leading_strong {
                    self.leading_strong = Some(LeadingStrong::Candidate);
                }
            }
            Tag::Strikethrough if self.table.is_none() => {
                self.tag("<span strikethrough=\"true\">");
            }
            Tag::Superscript if self.table.is_none() => self.tag("<sup>"),
            Tag::Subscript if self.table.is_none() => self.tag("<sub>"),
            Tag::Link { dest_url, .. } if self.table.is_none() => {
                let active =
                    safe_link(&dest_url).then(|| (self.text_offset, dest_url.into_string()));
                self.active_links.push(active);
                self.tag("<span foreground=\"#8ab4f8\" underline=\"single\">");
            }
            Tag::Image { .. } if self.table.is_none() => {
                self.tag("<span foreground=\"#9299a6\"><i>Image: ");
            }
            Tag::Image { .. } => {
                if let Some(table) = &mut self.table {
                    table.append("Image: ");
                }
            }
            Tag::Emphasis
            | Tag::Strong
            | Tag::Strikethrough
            | Tag::Superscript
            | Tag::Subscript
            | Tag::Link { .. } => {}
            Tag::MetadataBlock(_) => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::HtmlBlock => {}
            TagEnd::Item => {
                self.after_block_marker = false;
            }
            TagEnd::Heading(_) => {
                self.tag("</span>");
                if let Some(heading) = self.active_heading.take() {
                    self.headings.push(MarkdownHeading {
                        level: heading.level,
                        offset: heading.offset,
                        title: heading.title.trim().to_owned(),
                    });
                }
            }
            TagEnd::BlockQuote(_) => {
                self.after_block_marker = false;
                self.tag("</span>");
                self.quote_depth = self.quote_depth.saturating_sub(1);
            }
            TagEnd::CodeBlock => {
                let code = self.code_block.take().unwrap_or(CodeBlockState {
                    language: None,
                    text: String::new(),
                });
                let text = code.text.trim_end_matches('\n').to_owned();
                if code.language.as_deref() == Some("mermaid") {
                    self.embed(EmbeddedMarkdownContent::Mermaid(text));
                } else {
                    self.embed(EmbeddedMarkdownContent::CodeBlock(text));
                }
            }
            TagEnd::List(_) => {
                self.lists.pop();
            }
            TagEnd::FootnoteDefinition => {
                self.after_block_marker = false;
            }
            TagEnd::DefinitionList => {}
            TagEnd::DefinitionListTitle => self.tag("</b>"),
            TagEnd::DefinitionListDefinition => {}
            TagEnd::Table => {
                if let Some(table) = self.table.take() {
                    self.embed(EmbeddedMarkdownContent::Table(table.table));
                }
            }
            TagEnd::TableHead => {
                if let Some(table) = &mut self.table {
                    table.table.header_rows = table.table.rows.len();
                }
            }
            TagEnd::TableRow | TagEnd::TableCell => {}
            TagEnd::Emphasis if self.table.is_none() => self.tag("</i>"),
            TagEnd::Strong if self.table.is_none() => {
                self.leading_strong = None;
                self.tag("</b>");
            }
            TagEnd::Strikethrough if self.table.is_none() => self.tag("</span>"),
            TagEnd::Superscript if self.table.is_none() => self.tag("</sup>"),
            TagEnd::Subscript if self.table.is_none() => self.tag("</sub>"),
            TagEnd::Emphasis
            | TagEnd::Strong
            | TagEnd::Strikethrough
            | TagEnd::Superscript
            | TagEnd::Subscript => {}
            TagEnd::Link if self.table.is_none() => {
                self.tag("</span>");
                if let Some(Some((start, destination))) = self.active_links.pop() {
                    self.links.push(MarkdownLink {
                        start,
                        end: self.text_offset,
                        destination,
                    });
                }
            }
            TagEnd::Link => {}
            TagEnd::Image if self.table.is_none() => self.tag("</i></span>"),
            TagEnd::Image => {}
            TagEnd::MetadataBlock(_) => {}
        }
    }

    fn block(&mut self) {
        if !self.markup.is_empty() {
            self.breaks(1);
        }
    }

    fn quote_prefix(&mut self) {
        if self.quote_depth > 0 {
            self.tag("<span foreground=\"#3d424a\">");
            self.text(&"│ ".repeat(self.quote_depth));
            self.tag("</span>");
        }
    }

    fn breaks(&mut self, count: usize) {
        while self.line_breaks < count {
            self.markup.push('\n');
            self.text_offset += 1;
            self.line_breaks += 1;
        }
    }

    fn tag(&mut self, markup: &str) {
        self.markup.push_str(markup);
    }

    fn text(&mut self, text: &str) {
        self.markup.push_str(&glib::markup_escape_text(text));
        self.text_offset += text.chars().count() as i32;
        self.line_breaks = text
            .chars()
            .rev()
            .take_while(|character| *character == '\n')
            .count();
    }

    fn inline_text(&mut self, text: &str) {
        let style = match self.leading_strong {
            Some(LeadingStrong::Candidate) if is_attention_kind_lead(text) => {
                self.leading_strong = Some(LeadingStrong::Attention);
                LeadingStrong::Attention
            }
            Some(LeadingStrong::Candidate) => {
                self.leading_strong = Some(LeadingStrong::Plain);
                LeadingStrong::Plain
            }
            Some(style) => style,
            None => LeadingStrong::Plain,
        };
        if matches!(style, LeadingStrong::Attention) {
            self.tag("<span foreground=\"#79a5e3\">");
            self.text(text);
            self.tag("</span>");
        } else {
            self.text(text);
        }
    }

    fn append_captured(&mut self, text: &str) -> bool {
        if let Some(table) = &mut self.table {
            table.append(text);
            true
        } else if let Some(code_block) = &mut self.code_block {
            code_block.text.push_str(text);
            true
        } else {
            false
        }
    }

    fn capture_heading_text(&mut self, text: &str) {
        if let Some(heading) = &mut self.active_heading {
            heading.title.push_str(text);
        }
    }

    fn embed(&mut self, content: EmbeddedMarkdownContent) {
        self.embedded.push(EmbeddedMarkdown {
            offset: self.text_offset,
            content,
        });
        self.text("\u{fffc}");
    }
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn is_attention_kind_lead(text: &str) -> bool {
    let text = text.trim_start();
    if text.starts_with('→') {
        return true;
    }
    let Some(arrow) = text.find('→') else {
        return false;
    };
    let prefix = &text[..arrow];
    let number = prefix.trim_end();
    !number.is_empty()
        && number.bytes().all(|byte| byte.is_ascii_digit())
        && number.len() < prefix.len()
}

fn render_markdown(markdown: &str) -> RenderedMarkdown {
    let markdown = normalize_omp_math(markdown);
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_MATH
        | Options::ENABLE_SMART_PUNCTUATION;
    let renderer =
        Parser::new_ext(&markdown, options).fold(MarkdownRenderer::new(), MarkdownRenderer::render);
    RenderedMarkdown {
        markup: renderer.markup,
        embedded: renderer.embedded,
        links: renderer.links,
        headings: renderer.headings,
    }
}

fn render_streaming_markdown(markdown: &str) -> RenderedMarkdown {
    let markdown = mend_streaming_markdown(markdown);
    render_markdown(&markdown)
}

fn normalize_omp_math(markdown: &str) -> Cow<'_, str> {
    if !markdown.contains("\\(")
        && !markdown.contains("\\[")
        && !markdown.contains("\\begin{")
        && !markdown.contains("$$")
    {
        return Cow::Borrowed(markdown);
    }

    let bytes = markdown.as_bytes();
    let mut output = String::with_capacity(markdown.len());
    let mut index = 0;
    let mut fence = None;
    let mut inline_ticks = 0;
    while index < markdown.len() {
        let at_line_start = index == 0 || bytes[index - 1] == b'\n';
        if at_line_start {
            let line_end = markdown[index..]
                .find('\n')
                .map_or(markdown.len(), |offset| index + offset + 1);
            let content_end = line_end
                .checked_sub(1)
                .filter(|end| bytes[*end] == b'\n')
                .unwrap_or(line_end);
            let line = markdown[index..content_end].trim_end_matches('\r');
            let marker = markdown_fence_marker(line);
            if let Some((open_marker, open_length)) = fence {
                if marker.is_some_and(|(marker, length, marker_end)| {
                    marker == open_marker
                        && length >= open_length
                        && line[marker_end..].trim().is_empty()
                }) {
                    fence = None;
                }
                output.push_str(&markdown[index..line_end]);
                index = line_end;
                continue;
            }
            if let Some((marker, length, _)) = marker {
                fence = Some((marker, length));
                output.push_str(&markdown[index..line_end]);
                index = line_end;
                continue;
            }
            if inline_ticks == 0
                && let Some(environment_end) = bare_math_environment_end(markdown, index)
            {
                output.push_str("$$");
                push_collapsed_math(&mut output, &markdown[index..environment_end]);
                output.push_str("$$");
                index = environment_end;
                continue;
            }
        }

        if bytes[index] == b'`' {
            let run = bytes[index..]
                .iter()
                .take_while(|byte| **byte == b'`')
                .count();
            if inline_ticks == 0 {
                inline_ticks = run;
            } else if inline_ticks == run {
                inline_ticks = 0;
            }
            output.push_str(&markdown[index..index + run]);
            index += run;
            continue;
        }
        if inline_ticks == 0 {
            if markdown[index..].starts_with("$$")
                && let Some(close) = markdown[index + 2..].find("$$")
            {
                let close = index + 2 + close;
                output.push_str("$$");
                push_collapsed_math(&mut output, &markdown[index + 2..close]);
                output.push_str("$$");
                index = close + 2;
                continue;
            }
            if markdown[index..].starts_with("\\(")
                && let Some(close) = markdown[index + 2..].find("\\)")
            {
                let close = index + 2 + close;
                output.push('$');
                output.push_str(&markdown[index + 2..close]);
                output.push('$');
                index = close + 2;
                continue;
            }
            if markdown[index..].starts_with("\\[")
                && let Some(close) = markdown[index + 2..].find("\\]")
            {
                let close = index + 2 + close;
                output.push_str("$$");
                push_collapsed_math(&mut output, &markdown[index + 2..close]);
                output.push_str("$$");
                index = close + 2;
                continue;
            }
        }

        let character = markdown[index..]
            .chars()
            .next()
            .expect("index stays on a character boundary");
        output.push(character);
        index += character.len_utf8();
    }
    Cow::Owned(output)
}

fn push_collapsed_math(output: &mut String, source: &str) {
    let mut previous_was_whitespace = output.chars().next_back().is_some_and(char::is_whitespace);
    for character in source.chars() {
        if matches!(character, '\n' | '\r') {
            if !previous_was_whitespace {
                output.push(' ');
                previous_was_whitespace = true;
            }
        } else {
            output.push(character);
            previous_was_whitespace = character.is_whitespace();
        }
    }
}

fn markdown_fence_marker(line: &str) -> Option<(u8, usize, usize)> {
    let bytes = line.as_bytes();
    let indentation = bytes.iter().take_while(|byte| **byte == b' ').count();
    if indentation > 3 {
        return None;
    }
    let marker = *bytes.get(indentation)?;
    if !matches!(marker, b'`' | b'~') {
        return None;
    }
    let length = bytes[indentation..]
        .iter()
        .take_while(|byte| **byte == marker)
        .count();
    (length >= 3).then_some((marker, length, indentation + length))
}

fn bare_math_environment_end(markdown: &str, line_start: usize) -> Option<usize> {
    let line_end = markdown[line_start..]
        .find('\n')
        .map_or(markdown.len(), |offset| line_start + offset);
    let line = &markdown[line_start..line_end];
    let indentation = line.bytes().take_while(|byte| *byte == b' ').count();
    if indentation > 3 {
        return None;
    }
    let begin = line.get(indentation..)?.strip_prefix("\\begin{")?;
    let name_end = begin.find('}')?;
    let name = &begin[..name_end];
    let base_name = name.strip_suffix('*').unwrap_or(name);
    if !matches!(
        base_name,
        "matrix"
            | "smallmatrix"
            | "pmatrix"
            | "bmatrix"
            | "Bmatrix"
            | "vmatrix"
            | "Vmatrix"
            | "cases"
            | "dcases"
            | "rcases"
            | "drcases"
            | "aligned"
            | "alignedat"
            | "align"
            | "alignat"
            | "split"
            | "gathered"
            | "gatheredat"
            | "gather"
            | "multline"
            | "equation"
            | "eqnarray"
            | "array"
            | "subarray"
    ) {
        return None;
    }
    let close = format!("\\end{{{name}}}");
    let search_start = line_start + indentation + "\\begin{".len() + name_end + 1;
    markdown[search_start..]
        .find(&close)
        .map(|offset| search_start + offset + close.len())
}

fn safe_link(destination: &str) -> bool {
    url::Url::parse(destination)
        .map(|url| matches!(url.scheme(), "http" | "https" | "mailto"))
        .unwrap_or(false)
}

#[derive(Clone)]
pub(crate) struct MessageBody {
    view: gtk::TextView,
    source: Rc<RefCell<String>>,
    links: Rc<RefCell<Vec<MarkdownLink>>>,
    row: gtk::Box,
    headings: Rc<RefCell<Vec<MarkdownHeading>>>,
    heading_observers: Rc<RefCell<Vec<Box<dyn Fn()>>>>,
    embedded_widgets: Rc<RefCell<Vec<(EmbeddedMarkdownContent, gtk::Widget)>>>,
}

impl MessageBody {
    fn new(text: &str, rich: bool, row: &gtk::Box) -> Self {
        let view = gtk::TextView::builder()
            .accepts_tab(false)
            .cursor_visible(false)
            .editable(false)
            .focusable(true)
            .hexpand(true)
            .wrap_mode(gtk::WrapMode::WordChar)
            .build();
        view.add_css_class("message-body");
        let body = Self {
            view,
            source: Rc::new(RefCell::new(String::new())),
            links: Rc::new(RefCell::new(Vec::new())),
            row: row.clone(),
            headings: Rc::new(RefCell::new(Vec::new())),
            heading_observers: Rc::new(RefCell::new(Vec::new())),
            embedded_widgets: Rc::new(RefCell::new(Vec::new())),
        };
        wire_markdown_links(&body.view, &body.links);
        if rich {
            body.set_text(text);
        } else {
            body.set_streaming_text(text);
        }
        body
    }

    pub(crate) fn set_text(&self, text: &str) {
        self.source.replace(text.to_owned());
        self.install_rendered(render_markdown(text));
    }

    pub(crate) fn set_streaming_text(&self, text: &str) {
        self.source.replace(text.to_owned());
        self.install_rendered(render_streaming_markdown(text));
    }

    fn install_rendered(&self, rendered: RenderedMarkdown) {
        let RenderedMarkdown {
            markup,
            embedded,
            links,
            headings,
        } = rendered;
        let mut cached_widgets = self.embedded_widgets.take();
        for (_, widget) in &cached_widgets {
            self.view.remove(widget);
        }
        let buffer = gtk::TextBuffer::new(None::<&gtk::TextTagTable>);
        buffer.insert_markup(&mut buffer.end_iter(), &markup);
        self.view.set_buffer(Some(&buffer));
        let mut current_widgets = Vec::with_capacity(embedded.len());
        for embedded in embedded {
            let mut start = buffer.iter_at_offset(embedded.offset);
            let mut end = start;
            end.forward_char();
            buffer.delete(&mut start, &mut end);
            let anchor = gtk::TextChildAnchor::new();
            buffer.insert_child_anchor(&mut start, &anchor);
            let widget = cached_widgets
                .iter()
                .position(|(content, _)| content == &embedded.content)
                .map(|index| cached_widgets.swap_remove(index).1)
                .unwrap_or_else(|| embedded_markdown_widget(embedded.content.clone()));
            self.view.add_child_at_anchor(&widget, &anchor);
            current_widgets.push((embedded.content, widget));
        }
        self.embedded_widgets.replace(current_widgets);
        self.links.replace(links);
        let headings_changed = *self.headings.borrow() != headings;
        self.headings.replace(headings);
        if headings_changed {
            for observer in self.heading_observers.borrow().iter() {
                observer();
            }
        }
    }

    pub(crate) fn outline_headings(&self) -> Vec<MarkdownHeading> {
        let headings = self.headings.borrow();
        let hierarchical = headings
            .iter()
            .map(|heading| heading.level)
            .min()
            .zip(headings.iter().map(|heading| heading.level).max())
            .is_some_and(|(minimum, maximum)| minimum < maximum);
        if headings.len() >= 3 && hierarchical {
            headings.clone()
        } else {
            Vec::new()
        }
    }

    pub(crate) fn row(&self) -> &gtk::Box {
        &self.row
    }

    pub(crate) fn connect_headings_changed(&self, callback: impl Fn() + 'static) {
        self.heading_observers.borrow_mut().push(Box::new(callback));
    }
    pub(crate) fn heading_y_in(
        &self,
        heading: &MarkdownHeading,
        relative_to: &impl IsA<gtk::Widget>,
    ) -> Option<f64> {
        let iter = self.view.buffer().iter_at_offset(heading.offset);
        let location = self.view.iter_location(&iter);
        let (_, widget_y) = self.view.buffer_to_window_coords(
            gtk::TextWindowType::Widget,
            location.x(),
            location.y(),
        );
        self.view
            .compute_point(
                relative_to,
                &gtk::graphene::Point::new(0.0, widget_y as f32),
            )
            .map(|point| f64::from(point.y()))
    }

    pub(crate) fn scroll_to_heading(
        &self,
        heading: &MarkdownHeading,
        scroller: &gtk::ScrolledWindow,
    ) {
        let iter = self.view.buffer().iter_at_offset(heading.offset);
        let location = self.view.iter_location(&iter);
        let (_, widget_y) = self.view.buffer_to_window_coords(
            gtk::TextWindowType::Widget,
            location.x(),
            location.y(),
        );
        if let Some(point) = self
            .view
            .compute_point(scroller, &gtk::graphene::Point::new(0.0, widget_y as f32))
        {
            let adjustment = scroller.vadjustment();
            adjustment.set_value(
                (adjustment.value() + f64::from(point.y()) - 56.0)
                    .clamp(0.0, (adjustment.upper() - adjustment.page_size()).max(0.0)),
            );
        }
    }

    #[cfg(feature = "ui-stories")]
    pub(crate) fn add_css_class(&self, class: &str) {
        self.view.add_css_class(class);
    }
}

mod inline_code_anchor {
    use super::*;
    use gtk::subclass::prelude::*;

    #[derive(Default)]
    pub(super) struct InlineCodeAnchor;

    #[glib::object_subclass]
    impl ObjectSubclass for InlineCodeAnchor {
        const NAME: &'static str = "OmpInlineCodeAnchor";
        type Type = super::InlineCodeAnchor;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for InlineCodeAnchor {
        fn constructed(&self) {
            self.parent_constructed();

            let label = gtk::Label::new(None);
            label.add_css_class("markdown-inline-code");
            label.set_parent(&*self.obj());
            self.obj().set_overflow(gtk::Overflow::Visible);
        }

        fn dispose(&self) {
            if let Some(label) = self.obj().first_child() {
                label.unparent();
            }
        }
    }

    impl WidgetImpl for InlineCodeAnchor {
        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let Some(label) = self.obj().first_child() else {
                return (0, 0, -1, -1);
            };
            label.measure(orientation, for_size)
        }

        fn size_allocate(&self, width: i32, _height: i32, _baseline: i32) {
            let Some(label) = self.obj().first_child() else {
                return;
            };
            let (_, natural_height, _, natural_baseline) =
                label.measure(gtk::Orientation::Vertical, width);
            // GtkTextLayout bottom-aligns child-anchor boxes to the text baseline.
            // Preserve the full measured box for line sizing, then shift its
            // contents by the measured descent so the label baselines coincide.
            let descent = if natural_baseline >= 0 {
                natural_height.saturating_sub(natural_baseline)
            } else {
                0
            };
            let transform = gtk::gsk::Transform::default()
                .translate(&gtk::graphene::Point::new(0.0, descent as f32));
            label.allocate(width, natural_height, natural_baseline, Some(transform));
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            if let Some(label) = self.obj().first_child() {
                self.obj().snapshot_child(&label, snapshot);
            }
        }
    }
}

glib::wrapper! {
    struct InlineCodeAnchor(ObjectSubclass<inline_code_anchor::InlineCodeAnchor>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl InlineCodeAnchor {
    fn new(text: &str) -> Self {
        let anchor: Self = glib::Object::new();
        let label = anchor
            .first_child()
            .and_downcast::<gtk::Label>()
            .expect("inline code anchor must contain a label");
        label.set_label(text);
        anchor
    }
}

mod horizontal_rule_anchor {
    use super::*;
    use gtk::subclass::prelude::*;

    #[derive(Default)]
    pub(super) struct HorizontalRuleAnchor;

    #[glib::object_subclass]
    impl ObjectSubclass for HorizontalRuleAnchor {
        const NAME: &'static str = "OmpHorizontalRuleAnchor";
        type Type = super::HorizontalRuleAnchor;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for HorizontalRuleAnchor {
        fn constructed(&self) {
            self.parent_constructed();

            let separator = gtk::Separator::new(gtk::Orientation::Horizontal);
            separator.add_css_class("markdown-horizontal-rule");
            separator.set_parent(&*self.obj());
        }

        fn dispose(&self) {
            if let Some(separator) = self.obj().first_child() {
                separator.unparent();
            }
        }
    }

    impl WidgetImpl for HorizontalRuleAnchor {
        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let Some(separator) = self.obj().first_child() else {
                return (0, 0, -1, -1);
            };
            if orientation == gtk::Orientation::Horizontal {
                (560, 560, -1, -1)
            } else {
                separator.measure(orientation, for_size)
            }
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            if let Some(separator) = self.obj().first_child() {
                separator.allocate(width, height, baseline, None);
            }
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            if let Some(separator) = self.obj().first_child() {
                self.obj().snapshot_child(&separator, snapshot);
            }
        }
    }
}

glib::wrapper! {
    struct HorizontalRuleAnchor(ObjectSubclass<horizontal_rule_anchor::HorizontalRuleAnchor>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl HorizontalRuleAnchor {
    fn new() -> Self {
        glib::Object::new()
    }
}

fn embedded_markdown_widget(content: EmbeddedMarkdownContent) -> gtk::Widget {
    match content {
        EmbeddedMarkdownContent::InlineCode(text) => InlineCodeAnchor::new(&text).upcast(),
        EmbeddedMarkdownContent::HorizontalRule => HorizontalRuleAnchor::new().upcast(),
        EmbeddedMarkdownContent::CodeBlock(text) => markdown_code_block_label(&text).upcast(),
        EmbeddedMarkdownContent::InlineMath(source) => latex_widget(&source, false),
        EmbeddedMarkdownContent::DisplayMath(source) => latex_widget(&source, true),
        EmbeddedMarkdownContent::Table(table) => markdown_table_widget(table).upcast(),
        EmbeddedMarkdownContent::Mermaid(source) => mermaid_widget(&source),
    }
}

fn markdown_code_block_label(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_valign(gtk::Align::Center);
    label.set_halign(gtk::Align::Fill);
    label.set_hexpand(true);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::Char);
    label.set_width_chars(48);
    label.add_css_class("markdown-code-block");
    label
}

fn latex_widget(source: &str, display: bool) -> gtk::Widget {
    match render_latex_svg(source, display).and_then(|svg| {
        svg_picture(
            svg,
            "markdown-latex",
            &format!("LaTeX formula: {source}"),
            560,
        )
    }) {
        Ok(widget) => {
            widget.add_css_class(if display {
                "markdown-latex-display"
            } else {
                "markdown-latex-inline"
            });
            widget
        }
        Err(error) => rich_render_fallback(
            source,
            "markdown-latex-error",
            &format!("Could not render LaTeX: {error}"),
        ),
    }
}

fn render_latex_svg(source: &str, display: bool) -> Result<String, String> {
    let ast = parse_latex(source).map_err(|error| error.to_string())?;
    let style = if display {
        MathStyle::Display
    } else {
        MathStyle::Text
    };
    let options = LayoutOptions::default()
        .with_style(style)
        .with_color(Color::rgb(0.87, 0.89, 0.92));
    let layout_box = layout(&ast, &options);
    let display_list = to_display_list(&layout_box);
    let base_font_size = if display { 22.0 } else { 18.0 };
    let base_padding = if display { 6.0 } else { 2.0 };
    let natural_width = display_list.width * base_font_size + base_padding * 2.0;
    let natural_height =
        (display_list.height + display_list.depth) * base_font_size + base_padding * 2.0;
    let raster_scale = (720.0 / natural_width.max(1.0))
        .min(480.0 / natural_height.max(1.0))
        .min(1.0);
    let svg_options = SvgOptions {
        font_size: base_font_size * raster_scale,
        padding: base_padding * raster_scale,
        embed_glyphs: true,
        ..SvgOptions::default()
    };
    Ok(render_to_svg(&display_list, &svg_options))
}

fn markdown_table_widget(table: MarkdownTable) -> gtk::Grid {
    let grid = gtk::Grid::new();
    grid.add_css_class("markdown-table");
    grid.set_halign(gtk::Align::Fill);
    grid.set_hexpand(true);
    let row_count = table.rows.len();
    let column_count = table.rows.iter().map(Vec::len).max().unwrap_or(1);
    let cell_width_chars = (48 / column_count as i32).clamp(10, 24);
    for (row_index, row) in table.rows.into_iter().enumerate() {
        let column_count = row.len();
        for (column_index, text) in row.into_iter().enumerate() {
            let cell = gtk::Label::new(Some(&text));
            cell.set_xalign(
                match table
                    .alignments
                    .get(column_index)
                    .copied()
                    .unwrap_or(Alignment::None)
                {
                    Alignment::Center => 0.5,
                    Alignment::Right => 1.0,
                    Alignment::None | Alignment::Left => 0.0,
                },
            );
            cell.set_yalign(0.0);
            cell.set_hexpand(true);
            cell.set_wrap(true);
            cell.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            cell.set_max_width_chars(24);
            cell.set_width_chars(cell_width_chars);
            cell.set_selectable(true);
            cell.add_css_class("markdown-table-cell");
            if row_index < table.header_rows {
                cell.add_css_class("header");
            }
            if row_index + 1 == row_count {
                cell.add_css_class("last-row");
            }
            if column_index + 1 == column_count {
                cell.add_css_class("last-column");
            }
            grid.attach(&cell, column_index as i32, row_index as i32, 1, 1);
        }
    }
    grid
}

fn mermaid_widget(source: &str) -> gtk::Widget {
    match render_mermaid_svg(source)
        .and_then(|svg| svg_picture(svg, "markdown-mermaid", "Mermaid diagram", 560))
    {
        Ok(widget) => widget,
        Err(error) => rich_render_fallback(
            source,
            "markdown-diagram-error",
            &format!("Could not render Mermaid diagram: {error}"),
        ),
    }
}

fn render_mermaid_svg(source: &str) -> Result<String, String> {
    let mut parsed =
        mermaid_rs_renderer::parse_mermaid(source).map_err(|error| error.to_string())?;
    let theme = mermaid::theme();
    let config = mermaid::layout_config();
    mermaid::enforce_contrast(&mut parsed.graph, &theme, &config);
    let layout = mermaid_rs_renderer::compute_layout(&parsed.graph, &theme, &config);
    let dimensions = mermaid_rs_renderer::measure_svg_dimensions(&layout, &config, None);
    let scale = (720.0 / dimensions.width.max(1.0))
        .min(640.0 / dimensions.height.max(1.0))
        .min(1.0);
    Ok(mermaid_rs_renderer::render::render_svg_with_dimensions(
        &layout,
        &theme,
        &config,
        Some((dimensions.width * scale, dimensions.height * scale)),
    ))
}

fn svg_picture(
    svg: String,
    css_class: &str,
    alternative_text: &str,
    max_width: i32,
) -> Result<gtk::Widget, String> {
    let texture = gdk::Texture::from_bytes(&glib::Bytes::from_owned(svg.into_bytes()))
        .map_err(|error| error.to_string())?;
    let picture = gtk::Picture::for_paintable(&texture);
    picture.set_alternative_text(Some(alternative_text));
    picture.set_content_fit(gtk::ContentFit::Contain);
    picture.set_can_shrink(true);
    picture.set_halign(gtk::Align::Start);
    picture.add_css_class(css_class);
    let natural_width = texture.width().max(1);
    let natural_height = texture.height().max(1);
    let display_width = natural_width.min(max_width);
    let display_height = (f64::from(natural_height) * f64::from(display_width)
        / f64::from(natural_width))
    .ceil() as i32;
    picture.set_size_request(display_width, display_height);
    Ok(picture.upcast())
}

fn rich_render_fallback(text: &str, css_class: &str, tooltip: &str) -> gtk::Widget {
    let label = markdown_code_block_label(text);
    label.add_css_class(css_class);
    label.set_tooltip_text(Some(tooltip));
    label.upcast()
}

pub fn append_message(messages: &gtk::Box, role: MessageRole, text: &str) -> MessageBody {
    append_message_with_mode(messages, role, text, true)
}

pub(crate) fn append_streaming_message(
    messages: &gtk::Box,
    role: MessageRole,
    text: &str,
) -> MessageBody {
    append_message_with_mode(messages, role, text, false)
}

fn append_message_with_mode(
    messages: &gtk::Box,
    role: MessageRole,
    text: &str,
    rich: bool,
) -> MessageBody {
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

    let body = MessageBody::new(text, rich, &row);
    row.append(&author);
    row.append(&body.view);
    messages.append(&row);
    wire_message_menu(&row, &body, role);
    body
}

#[cfg(feature = "ui-stories")]
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

fn wire_markdown_links(view: &gtk::TextView, links: &Rc<RefCell<Vec<MarkdownLink>>>) {
    let click = gtk::GestureClick::new();
    click.set_button(1);
    let weak_view = view.downgrade();
    let links_for_click = links.clone();
    click.connect_released(move |gesture, presses, x, y| {
        let Some(view) = weak_view.upgrade() else {
            return;
        };
        if presses != 1 {
            return;
        }
        let Some(destination) = markdown_link_at(&view, &links_for_click.borrow(), x, y) else {
            return;
        };
        gesture.set_state(gtk::EventSequenceState::Claimed);
        if let Err(error) = gtk::gio::AppInfo::launch_default_for_uri(
            &destination,
            gtk::gio::AppLaunchContext::NONE,
        ) {
            eprintln!("Could not open link {destination}: {error}");
        }
    });
    view.add_controller(click);

    let motion = gtk::EventControllerMotion::new();
    let weak_view = view.downgrade();
    let links_for_motion = links.clone();
    motion.connect_motion(move |_, x, y| {
        let Some(view) = weak_view.upgrade() else {
            return;
        };
        let cursor = markdown_link_at(&view, &links_for_motion.borrow(), x, y).map(|_| "pointer");
        view.set_cursor_from_name(cursor);
    });
    let weak_view = view.downgrade();
    motion.connect_leave(move |_| {
        if let Some(view) = weak_view.upgrade() {
            view.set_cursor_from_name(None);
        }
    });
    view.add_controller(motion);
}

fn markdown_link_at(
    view: &gtk::TextView,
    links: &[MarkdownLink],
    x: f64,
    y: f64,
) -> Option<String> {
    let (x, y) = view.window_to_buffer_coords(
        gtk::TextWindowType::Widget,
        x.round() as i32,
        y.round() as i32,
    );
    let offset = view.iter_at_location(x, y)?.offset();
    links
        .iter()
        .find(|link| link.start <= offset && offset < link.end)
        .map(|link| link.destination.clone())
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

fn workspace_button(workspace: &Path) -> gtk::Button {
    let button = gtk::Button::new();
    button.set_hexpand(true);
    button.set_tooltip_text(Some(&workspace.to_string_lossy()));
    button.update_property(&[gtk::accessible::Property::Label(&format!(
        "Start in {}",
        workspace.to_string_lossy()
    ))]);
    button.add_css_class("conversation-workspace-button");

    let content = gtk::Box::new(gtk::Orientation::Horizontal, 11);
    content.append(&icons::icon(icons::Icon::Folder, 17));
    let labels = gtk::Box::new(gtk::Orientation::Vertical, 1);
    labels.set_hexpand(true);
    let name = workspace
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| workspace.to_string_lossy().into_owned());
    let title = gtk::Label::new(Some(&name));
    title.set_xalign(0.0);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.add_css_class("conversation-workspace-title");
    let path = gtk::Label::new(Some(&workspace.to_string_lossy()));
    path.set_xalign(0.0);
    path.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    path.add_css_class("conversation-workspace-path");
    labels.append(&title);
    labels.append(&path);
    content.append(&labels);
    button.set_child(Some(&content));
    button
}

impl ChatHero {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 9);
        root.set_halign(gtk::Align::Center);
        root.set_valign(gtk::Align::Center);
        root.set_vexpand(true);
        root.set_margin_start(32);
        root.set_margin_end(32);
        root.add_css_class("conversation-hero");

        let logo = icons::omp_logo(88);
        logo.add_css_class("conversation-hero-logo");
        let title = gtk::Label::new(None);
        title.set_justify(gtk::Justification::Center);
        title.add_css_class("conversation-hero-title");
        let detail = gtk::Label::new(None);
        detail.set_justify(gtk::Justification::Center);
        detail.set_wrap(true);
        detail.set_max_width_chars(58);
        detail.add_css_class("conversation-hero-detail");

        let workspace_choices = gtk::Box::new(gtk::Orientation::Vertical, 7);
        workspace_choices.set_size_request(440, -1);
        workspace_choices.set_margin_top(10);
        workspace_choices.add_css_class("conversation-workspace-choices");
        workspace_choices.set_visible(false);

        let state_line = gtk::Box::new(gtk::Orientation::Horizontal, 7);
        state_line.set_halign(gtk::Align::Center);
        state_line.add_css_class("conversation-hero-state");
        let spinner = gtk::Spinner::new();
        spinner.set_size_request(16, 16);
        let state_dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        state_dot.set_size_request(7, 7);
        state_dot.set_valign(gtk::Align::Center);
        state_dot.add_css_class("conversation-hero-state-dot");
        let state_label = gtk::Label::new(None);
        state_label.add_css_class("conversation-hero-state-label");
        state_line.append(&spinner);
        state_line.append(&state_dot);
        state_line.append(&state_label);

        let hint = gtk::Label::new(None);
        hint.set_halign(gtk::Align::Center);
        hint.add_css_class("conversation-hero-hint");

        root.append(&logo);
        root.append(&title);
        root.append(&detail);
        root.append(&workspace_choices);
        root.append(&state_line);
        root.append(&hint);

        let hero = Self {
            root,
            logo,
            title,
            detail,
            workspace_choices,
            hint,
            state_line,
            spinner,
            state_dot,
            state_label,
            animation_generation: Rc::new(Cell::new(0)),
        };
        hero.show_empty();
        hero
    }

    pub fn show_empty(&self) {
        self.stop_loading_animation();
        self.root.remove_css_class("loading");
        self.root.remove_css_class("offline");
        self.logo.set_opacity(1.0);
        self.workspace_choices.set_visible(false);
        self.title.set_text("What should we work on?");
        self.detail.set_text(
            "Bring a goal, a question, or a complex workflow. omp can plan, delegate, and carry it through.",
        );
        self.state_line.set_visible(false);
        self.spinner.stop();
        self.hint.set_text("Type / to explore commands");
        self.hint.set_visible(true);
    }

    pub fn show_workspace_onboarding<F, G>(
        &self,
        recent_workspaces: &[PathBuf],
        current_workspace: Option<&Path>,
        on_select: F,
        on_browse: G,
    ) where
        F: Fn(PathBuf) + Clone + 'static,
        G: Fn() + 'static,
    {
        self.show_empty();
        self.title.set_text("Choose where to start");
        self.detail.set_text(if recent_workspaces.is_empty() {
            "Choose a folder for this conversation, or start typing to use the current workspace."
        } else {
            "Pick a recent folder, browse somewhere else, or start typing to use the current workspace."
        });
        self.hint.set_visible(false);
        while let Some(child) = self.workspace_choices.first_child() {
            self.workspace_choices.remove(&child);
        }

        if !recent_workspaces.is_empty() {
            let heading = gtk::Label::new(Some("RECENT FOLDERS"));
            heading.set_xalign(0.0);
            heading.add_css_class("conversation-workspace-heading");
            self.workspace_choices.append(&heading);
        }
        for workspace in recent_workspaces {
            let button = workspace_button(workspace);
            let selected_workspace = workspace.clone();
            let on_select = on_select.clone();
            button.connect_clicked(move |_| on_select(selected_workspace.clone()));
            self.workspace_choices.append(&button);
        }

        let browse = icons::labeled_button(icons::Icon::FolderOpen, "Browse folders");
        browse.set_halign(gtk::Align::Fill);
        browse.update_property(&[gtk::accessible::Property::Label("Browse folders")]);
        browse.add_css_class("conversation-workspace-browse");
        browse.connect_clicked(move |_| on_browse());
        self.workspace_choices.append(&browse);
        self.workspace_choices.set_visible(true);

        if let Some(current_workspace) = current_workspace {
            self.workspace_choices.set_tooltip_text(Some(&format!(
                "Current workspace: {}",
                current_workspace.to_string_lossy()
            )));
        } else {
            self.workspace_choices.set_tooltip_text(None);
        }
    }

    pub fn show_loading(&self, title: &str, detail: &str, activity: &str) {
        self.stop_loading_animation();
        self.root.remove_css_class("offline");
        self.root.add_css_class("loading");
        self.title.set_text(title);
        self.detail.set_text(detail);
        self.state_label.set_text(activity);
        self.state_dot.set_visible(false);
        self.workspace_choices.set_visible(false);
        self.spinner.set_visible(true);
        self.spinner.start();
        self.state_line.set_visible(true);
        self.hint.set_visible(false);
        self.start_loading_animation(title);
    }

    pub fn show_disconnected(&self, detail: &str) {
        self.stop_loading_animation();
        self.root.remove_css_class("loading");
        self.root.add_css_class("offline");
        self.logo.set_opacity(0.82);
        self.title.set_text("Can’t connect to omp");
        self.detail.set_text(detail);
        self.workspace_choices.set_visible(false);
        self.spinner.stop();
        self.spinner.set_visible(false);
        self.state_dot.set_visible(true);
        self.state_label.set_text("Runtime unavailable");
        self.state_line.set_visible(true);
        self.hint.set_visible(false);
    }

    pub fn hide(&self) {
        self.stop_loading_animation();
        self.spinner.stop();
        self.root.set_visible(false);
    }
    fn start_loading_animation(&self, text: &str) {
        let animations_enabled = gtk::Settings::default()
            .map(|settings| settings.is_gtk_enable_animations())
            .unwrap_or(true);
        if !animations_enabled {
            self.logo.set_opacity(1.0);
            self.title.set_text(text);
            return;
        }

        let generation = self.animation_generation.get();
        let animation_generation = self.animation_generation.clone();
        let title = self.title.downgrade();
        let logo = self.logo.downgrade();
        let text = text.to_owned();
        let started = Instant::now();
        self.title
            .set_markup(&shimmer_markup(&text, Duration::ZERO));
        glib::timeout_add_local(Duration::from_millis(33), move || {
            let (Some(title), Some(logo)) = (title.upgrade(), logo.upgrade()) else {
                return glib::ControlFlow::Break;
            };
            if animation_generation.get() != generation {
                return glib::ControlFlow::Break;
            }
            let elapsed = started.elapsed();
            title.set_markup(&shimmer_markup(&text, elapsed));
            let pulse = (elapsed.as_secs_f64() * 2.4).sin().mul_add(0.08, 0.9);
            logo.set_opacity(pulse);
            glib::ControlFlow::Continue
        });
    }

    fn stop_loading_animation(&self) {
        self.animation_generation
            .set(self.animation_generation.get().wrapping_add(1));
    }
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
    use super::{
        EmbeddedMarkdownContent, render_latex_svg, render_markdown, render_mermaid_svg,
        render_streaming_markdown, shimmer_markup, thinking_summary,
    };
    use gtk4::pango;
    use std::time::Duration;

    #[test]
    fn markdown_renders_common_chat_content_as_valid_pango_markup() {
        let rendered = render_markdown(
            "# Result\n\nUse **bold**, *italic*, and `code`.\n\n- first\n- [x] done\n\n> quoted",
        );
        let (_, plain_text, _) = pango::parse_markup(&rendered.markup, '\0')
            .expect("renderer must produce valid Pango markup");

        assert_eq!(
            plain_text,
            "Result\nUse bold, italic, and \u{fffc}.\n• first\n• ☑ done\n│ quoted"
        );
        assert!(rendered.markup.contains("weight=\"bold\""));
        assert!(rendered.markup.contains("foreground=\"#febc38\""));
        assert!(rendered.markup.contains("foreground=\"#777d88\""));
        assert!(rendered.markup.contains("<b>bold</b>"));
        assert_eq!(rendered.embedded.len(), 1);
        assert_eq!(
            rendered.embedded[0].content,
            EmbeddedMarkdownContent::InlineCode("code".to_owned())
        );
    }

    #[test]
    fn markdown_colors_attention_kind_leads_only() {
        let rendered = render_markdown(
            "**→ Bottom line.** Supporting detail.\n\n\
             **1 →** First point.\n\n\
             Plain **→ bold, but not a lead.**\n\n\
             `code` **→ also not a lead.**",
        );
        let (_, plain_text, _) = pango::parse_markup(&rendered.markup, '\0')
            .expect("attention-kind renderer must produce valid Pango markup");

        assert_eq!(
            plain_text,
            "→ Bottom line. Supporting detail.\n1 → First point.\nPlain → bold, but not a lead.\n\u{fffc} → also not a lead."
        );
        assert!(
            rendered
                .markup
                .contains("<b><span foreground=\"#79a5e3\">→ Bottom line.</span></b>")
        );
        assert!(
            rendered
                .markup
                .contains("<b><span foreground=\"#79a5e3\">1 →</span></b>")
        );
        assert!(
            rendered
                .markup
                .contains("Plain <b>→ bold, but not a lead.</b>")
        );
        assert!(
            rendered
                .markup
                .contains("\u{fffc} <b>→ also not a lead.</b>")
        );
    }

    #[test]
    fn markdown_keeps_adjacent_blocks_on_distinct_compact_lines() {
        let rendered = render_markdown(
            "## Heading\nParagraph\n\n> Quote\n\n1. first\n\n```text\ncode\n```\n\n***\n\nAfter",
        );
        let (_, plain_text, _) = pango::parse_markup(&rendered.markup, '\0')
            .expect("renderer must produce valid Pango markup");

        assert_eq!(
            plain_text,
            "Heading\nParagraph\n│ Quote\n1. first\n\u{fffc}\n\u{fffc}\nAfter"
        );
        assert!(matches!(
            rendered.embedded.as_slice(),
            [
                super::EmbeddedMarkdown {
                    content: EmbeddedMarkdownContent::CodeBlock(_),
                    ..
                },
                super::EmbeddedMarkdown {
                    content: EmbeddedMarkdownContent::HorizontalRule,
                    ..
                }
            ]
        ));
    }

    #[test]
    fn explicit_rule_replaces_automatic_heading_rule() {
        let rendered = render_markdown("# Overview\nIntro\n\n---\n\n## Design\nDetails");
        let rule_count = rendered
            .embedded
            .iter()
            .filter(|embedded| embedded.content == EmbeddedMarkdownContent::HorizontalRule)
            .count();

        assert_eq!(rule_count, 1);
    }

    #[test]
    fn markdown_separates_only_headings_after_the_first() {
        let rendered = render_markdown(
            "# Overview\nIntro\n\n## Design\nDetails\n\n### Parser\nImplementation",
        );
        let (_, plain_text, _) = pango::parse_markup(&rendered.markup, '\0')
            .expect("renderer must produce valid Pango markup");

        assert_eq!(
            plain_text,
            "Overview\nIntro\n\u{fffc}\nDesign\nDetails\n\u{fffc}\nParser\nImplementation"
        );
        assert_eq!(
            rendered
                .headings
                .iter()
                .map(|heading| (heading.level, heading.title.as_str()))
                .collect::<Vec<_>>(),
            vec![(1, "Overview"), (2, "Design"), (3, "Parser")]
        );
    }

    #[test]
    fn streaming_markdown_formats_incomplete_inline_and_block_syntax() {
        let rendered = render_streaming_markdown(
            "# Live\n\nWriting **formatted text and `inline code\n\n```rust\nfn render() {",
        );
        let (_, plain_text, _) = pango::parse_markup(&rendered.markup, '\0')
            .expect("streaming renderer must produce valid Pango markup");

        assert_eq!(
            plain_text,
            "Live\nWriting formatted text and \u{fffc}\n\u{fffc}"
        );
        assert!(rendered.markup.contains("<b>formatted text and "));
        assert!(matches!(
            &rendered.embedded[0].content,
            EmbeddedMarkdownContent::InlineCode(code) if code == "inline code"
        ));
        assert!(matches!(
            &rendered.embedded[1].content,
            EmbeddedMarkdownContent::CodeBlock(code) if code == "fn render() {"
        ));
    }

    #[test]
    fn markdown_escapes_html_and_only_activates_safe_links() {
        let rendered = render_markdown(
            "[docs](https://example.com/docs) [unsafe](javascript:alert(1)) <button>",
        );
        assert_eq!(rendered.links.len(), 1);
        assert_eq!(rendered.links[0].destination, "https://example.com/docs");
        assert!(rendered.markup.contains("&lt;button&gt;"));
    }

    #[test]
    fn markdown_embeds_code_blocks_without_losing_their_contents() {
        let rendered = render_markdown("Before `x`.\n\n```\nlet y = 1;\n```\n\nAfter.");
        assert_eq!(rendered.embedded.len(), 2);
        assert_eq!(
            rendered.embedded[0].content,
            EmbeddedMarkdownContent::InlineCode("x".to_owned())
        );
        assert_eq!(
            rendered.embedded[1].content,
            EmbeddedMarkdownContent::CodeBlock("let y = 1;".to_owned())
        );
    }

    #[test]
    fn markdown_embeds_latex_tables_and_mermaid_as_rich_content() {
        let rendered = render_markdown(
            "Inline $x^2$.\n\n$$\\frac{1}{2}$$\n\n\
             | Name | Value |\n| :--- | ---: |\n| Alpha | 42 |\n\n\
             ```mermaid\nflowchart LR\n    A --> B\n```",
        );

        assert!(matches!(
            &rendered.embedded[0].content,
            EmbeddedMarkdownContent::InlineMath(source) if source == "x^2"
        ));
        assert!(matches!(
            &rendered.embedded[1].content,
            EmbeddedMarkdownContent::DisplayMath(source) if source == "\\frac{1}{2}"
        ));
        let EmbeddedMarkdownContent::Table(table) = &rendered.embedded[2].content else {
            panic!("third embedded block should be a table");
        };
        assert_eq!(table.header_rows, 1);
        assert_eq!(
            table.rows,
            vec![
                vec!["Name".to_owned(), "Value".to_owned()],
                vec!["Alpha".to_owned(), "42".to_owned()]
            ]
        );
        assert!(matches!(
            &rendered.embedded[3].content,
            EmbeddedMarkdownContent::Mermaid(source)
                if source == "flowchart LR\n    A --> B"
        ));
    }

    #[test]
    fn markdown_accepts_omp_math_delimiters_and_preserves_code_fences() {
        let rendered = render_markdown(
            r#"Inline \(x + 1\).

\[\frac{1}{2}\]

\begin{align}
x &= y + 1
\end{align}

```text
\(literal\)
```"#,
        );

        assert!(matches!(
            &rendered.embedded[0].content,
            EmbeddedMarkdownContent::InlineMath(source) if source == "x + 1"
        ));
        assert!(matches!(
            &rendered.embedded[1].content,
            EmbeddedMarkdownContent::DisplayMath(source) if source == "\\frac{1}{2}"
        ));
        assert!(matches!(
            &rendered.embedded[2].content,
            EmbeddedMarkdownContent::DisplayMath(source)
                if source == "\\begin{align} x &= y + 1 \\end{align}"
        ));
        assert!(matches!(
            &rendered.embedded[3].content,
            EmbeddedMarkdownContent::CodeBlock(source) if source == "\\(literal\\)"
        ));
    }

    #[test]
    fn markdown_keeps_multiline_display_math_in_single_render_blocks() {
        let rendered = render_markdown(
            r#"$$
\begin{aligned}
\mathcal{L}(x,\lambda,\mu) &= f(x), \\
\nabla_x\mathcal{L}(x^\star,\lambda^\star,\mu^\star) &= 0
\end{aligned}
$$

\[
A=
\begin{pmatrix}
4 & 12 & -16 \\
12 & 37 & -43 \\
-16 & -43 & 98
\end{pmatrix}
\]"#,
        );

        assert_eq!(rendered.embedded.len(), 2);
        for embedded in &rendered.embedded {
            let EmbeddedMarkdownContent::DisplayMath(source) = &embedded.content else {
                panic!("multiline formula should remain one display-math block");
            };
            render_latex_svg(source, true).expect("complex display math should render");
        }
        let (_, plain_text, _) = pango::parse_markup(&rendered.markup, '\0')
            .expect("renderer must produce valid Pango markup");
        assert_eq!(plain_text, "\u{fffc}\n\u{fffc}");
    }

    #[test]
    fn reported_complex_formulas_render_without_fallbacks() {
        let formulas = [
            r"\text{Euler's identity is } e^{i\pi}+1=0",
            r"E^2=p^2c^2+m^2c^4",
            r"x=\frac{-b\pm\sqrt{b^2-4ac}}{2a}",
            r"\begin{align}\nabla \cdot \mathbf{E} &= \frac{\rho}{\varepsilon_0}, \\\nabla \cdot \mathbf{B} &= 0, \\\nabla \times \mathbf{E} &= -\frac{\partial \mathbf{B}}{\partial t}, \\\nabla \times \mathbf{B} &= \mu_0\mathbf{J}+\mu_0\varepsilon_0\frac{\partial \mathbf{E}}{\partial t}\end{align}",
            r"\mathcal{L}(x,\lambda,\mu)=f(x)+\sum_{i=1}^{m}\lambda_i h_i(x)+\sum_{j=1}^{p}\mu_j g_j(x)",
            r"f(x)=\begin{cases}\displaystyle \int_0^x e^{-t^2}\,dt,&x\ge 0,\\[8pt]-\displaystyle \int_x^0 e^{-t^2}\,dt,&x<0.\end{cases}\qquad \lim_{x\to\infty}f(x)=\frac{\sqrt{\pi}}{2}",
        ];

        for formula in formulas {
            render_latex_svg(formula, true)
                .unwrap_or_else(|error| panic!("formula did not render: {error}\n{formula}"));
        }
    }

    #[test]
    fn native_rich_renderers_produce_svg_images() {
        let latex = render_latex_svg(r"\frac{1}{2}", true).expect("LaTeX should render");
        assert!(latex.starts_with("<svg"));
        assert!(latex.contains("<path"));

        let mermaid =
            render_mermaid_svg("flowchart LR\n    A --> B").expect("Mermaid should render");
        assert!(mermaid.starts_with("<svg"));
        assert!(mermaid.contains("A"));
        assert!(mermaid.contains("B"));
        assert!(!mermaid.contains("fill=\"#333333\""));
    }

    #[test]
    fn native_rich_renderers_bound_large_svg_rasters() {
        let formula = std::iter::repeat_n("x_i", 180)
            .collect::<Vec<_>>()
            .join("+");
        let latex = render_latex_svg(&formula, true).expect("wide formula should render");
        assert!(svg_dimension(&latex, "width") <= 720.0);
        assert!(svg_dimension(&latex, "height") <= 480.0);

        let mut diagram = String::from("flowchart LR\n");
        for index in 0..32 {
            diagram.push_str(&format!("N{index}[Stage {index}] --> N{}\n", index + 1));
        }
        let mermaid = render_mermaid_svg(&diagram).expect("wide Mermaid diagram should render");
        assert!(svg_dimension(&mermaid, "width") <= 720.0);
        assert!(svg_dimension(&mermaid, "height") <= 640.0);
    }

    fn svg_dimension(svg: &str, attribute: &str) -> f32 {
        let marker = format!("{attribute}=\"");
        svg.split_once(&marker)
            .and_then(|(_, suffix)| suffix.split_once('"'))
            .and_then(|(value, _)| value.trim_end_matches("pt").parse().ok())
            .expect("SVG root should have a numeric dimension")
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
