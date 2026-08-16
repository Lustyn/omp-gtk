use std::borrow::Cow;

const INCOMPLETE_LINK_TARGET: &str = "streamdown:incomplete-link";

#[derive(Clone, Copy, PartialEq, Eq)]
enum InlineMarker {
    Backticks(usize),
    BoldAsterisk,
    ItalicAsterisk,
    BoldItalicAsterisk,
    BoldUnderscore,
    ItalicUnderscore,
    Strikethrough,
    DisplayMath,
}

impl InlineMarker {
    fn closing(self) -> &'static str {
        match self {
            Self::Backticks(1) => "`",
            Self::Backticks(2) => "``",
            Self::Backticks(_) => "```",
            Self::BoldAsterisk => "**",
            Self::ItalicAsterisk => "*",
            Self::BoldItalicAsterisk => "***",
            Self::BoldUnderscore => "__",
            Self::ItalicUnderscore => "_",
            Self::Strikethrough => "~~",
            Self::DisplayMath => "$$",
        }
    }
}

#[derive(Clone, Copy)]
struct OpenMarker {
    marker: InlineMarker,
    content_start: usize,
}

pub(super) fn mend_streaming_markdown(markdown: &str) -> Cow<'_, str> {
    if markdown.is_empty() {
        return Cow::Borrowed(markdown);
    }

    // A single trailing space is usually a token boundary rather than an intentional hard break.
    let markdown = if markdown.ends_with(' ') && !markdown.ends_with("  ") {
        &markdown[..markdown.len() - 1]
    } else {
        markdown
    };
    if let Some(fence_start) = unclosed_fence_start(markdown) {
        let completed_prefix = mend_streaming_markdown(&markdown[..fence_start]);
        if matches!(completed_prefix, Cow::Borrowed(_)) {
            return Cow::Borrowed(markdown);
        }
        let mut result = completed_prefix.into_owned();
        result.push_str(&markdown[fence_start..]);
        return Cow::Owned(result);
    }

    let mut result = Cow::Borrowed(markdown);
    if needs_setext_guard(&result) {
        result.to_mut().push('\u{200b}');
    }

    if let Some(link) = incomplete_link(&result) {
        let value = result.to_mut();
        if link.image {
            value.truncate(link.start);
        } else if let Some(label_end) = link.label_end {
            value.truncate(label_end + 1);
            value.push('(');
            value.push_str(INCOMPLETE_LINK_TARGET);
            value.push(')');
        } else {
            value.push_str("](");
            value.push_str(INCOMPLETE_LINK_TARGET);
            value.push(')');
        }
        return result;
    }

    let content_end = result.trim_end_matches(['\n', '\r']).len();
    let paragraph_start = result[..content_end]
        .rfind("\n\n")
        .map_or(0, |index| index + 2);
    let closers = incomplete_inline_closers(&result[paragraph_start..content_end]);
    if !closers.is_empty() {
        result.to_mut().insert_str(content_end, &closers);
    }
    result
}

fn unclosed_fence_start(markdown: &str) -> Option<usize> {
    let mut fence: Option<(u8, usize, usize)> = None;
    let mut line_start = 0;
    for line_with_newline in markdown.split_inclusive('\n') {
        let line = line_with_newline
            .trim_end_matches('\n')
            .trim_end_matches('\r');
        let bytes = line.as_bytes();
        let indent = bytes.iter().take_while(|byte| **byte == b' ').count();
        if indent <= 3
            && let Some(&marker) = bytes.get(indent)
            && matches!(marker, b'`' | b'~')
        {
            let length = bytes[indent..]
                .iter()
                .take_while(|byte| **byte == marker)
                .count();
            if length >= 3 {
                match fence {
                    None => fence = Some((marker, length, line_start)),
                    Some((open_marker, open_length, _))
                        if marker == open_marker
                            && length >= open_length
                            && line[indent + length..].trim().is_empty() =>
                    {
                        fence = None;
                    }
                    Some(_) => {}
                }
            }
        }
        line_start += line_with_newline.len();
    }
    fence.map(|(_, _, start)| start)
}

fn needs_setext_guard(markdown: &str) -> bool {
    let Some(newline) = markdown.rfind('\n') else {
        return false;
    };
    let last = markdown[newline + 1..].trim();
    if last.is_empty() || last.len() > 2 || !last.bytes().all(|byte| byte == b'-' || byte == b'=') {
        return false;
    }
    markdown[..newline]
        .lines()
        .next_back()
        .is_some_and(|line| !line.trim().is_empty())
}

#[derive(Clone, Copy)]
struct IncompleteLink {
    start: usize,
    label_end: Option<usize>,
    image: bool,
}

fn incomplete_link(markdown: &str) -> Option<IncompleteLink> {
    let paragraph_start = markdown.rfind("\n\n").map_or(0, |index| index + 2);
    let paragraph = &markdown[paragraph_start..];
    let bytes = paragraph.as_bytes();
    let mut brackets = Vec::new();
    let mut ticks = 0;
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            index = (index + 2).min(bytes.len());
            continue;
        }
        if bytes[index] == b'`' {
            let run = bytes[index..]
                .iter()
                .take_while(|byte| **byte == b'`')
                .count();
            if ticks == 0 {
                ticks = run;
            } else if ticks == run {
                ticks = 0;
            }
            index += run;
            continue;
        }
        if ticks > 0 {
            index += 1;
            continue;
        }
        if bytes[index] == b'[' {
            let image = index > 0 && bytes[index - 1] == b'!';
            brackets.push((index, image));
        } else if bytes[index] == b']'
            && let Some((open, image)) = brackets.pop()
            && bytes.get(index + 1) == Some(&b'(')
            && !paragraph[index + 2..].contains(')')
        {
            return Some(IncompleteLink {
                start: paragraph_start + open - usize::from(image),
                label_end: Some(paragraph_start + index),
                image,
            });
        }
        index += 1;
    }
    brackets.last().map(|(open, image)| IncompleteLink {
        start: paragraph_start + *open - usize::from(*image),
        label_end: None,
        image: *image,
    })
}

fn incomplete_inline_closers(paragraph: &str) -> String {
    let trimmed = paragraph.trim();
    if trimmed.len() >= 3
        && trimmed
            .bytes()
            .all(|byte| matches!(byte, b'*' | b'_' | b'~' | b' '))
    {
        return String::new();
    }

    let bytes = paragraph.as_bytes();
    let mut open = Vec::<OpenMarker>::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            index = (index + 2).min(bytes.len());
            continue;
        }

        if let Some(OpenMarker {
            marker: InlineMarker::Backticks(length),
            ..
        }) = open.last().copied()
        {
            if bytes[index] == b'`' {
                let run = bytes[index..]
                    .iter()
                    .take_while(|byte| **byte == b'`')
                    .count();
                if run == length {
                    open.pop();
                }
                index += run;
            } else {
                index += 1;
            }
            continue;
        }

        let (marker, length) = match bytes[index] {
            b'`' => {
                let run = bytes[index..]
                    .iter()
                    .take_while(|byte| **byte == b'`')
                    .count();
                (InlineMarker::Backticks(run), run)
            }
            b'*' => {
                let run = bytes[index..]
                    .iter()
                    .take_while(|byte| **byte == b'*')
                    .count();
                match run {
                    1 => (InlineMarker::ItalicAsterisk, 1),
                    2 => (InlineMarker::BoldAsterisk, 2),
                    _ => (InlineMarker::BoldItalicAsterisk, 3),
                }
            }
            b'_' => {
                let run = bytes[index..]
                    .iter()
                    .take_while(|byte| **byte == b'_')
                    .count();
                if run >= 2 {
                    (InlineMarker::BoldUnderscore, 2)
                } else {
                    (InlineMarker::ItalicUnderscore, 1)
                }
            }
            b'~' if bytes.get(index + 1) == Some(&b'~') => (InlineMarker::Strikethrough, 2),
            b'$' if bytes.get(index + 1) == Some(&b'$') => (InlineMarker::DisplayMath, 2),
            _ => {
                index += 1;
                continue;
            }
        };

        let previous = index.checked_sub(1).and_then(|offset| bytes.get(offset));
        let next = bytes.get(index + length);
        let underscore_inside_word = matches!(
            marker,
            InlineMarker::BoldUnderscore | InlineMarker::ItalicUnderscore
        ) && previous.is_some_and(u8::is_ascii_alphanumeric)
            && next.is_some_and(u8::is_ascii_alphanumeric);
        if underscore_inside_word {
            index += length;
            continue;
        }
        let can_close = previous.is_some_and(|byte| !byte.is_ascii_whitespace());
        let can_open = next.is_some_and(|byte| !byte.is_ascii_whitespace());

        if open.last().is_some_and(|entry| entry.marker == marker) && can_close {
            open.pop();
        } else if marker == InlineMarker::ItalicAsterisk
            && index + length == bytes.len()
            && open
                .last()
                .is_some_and(|entry| entry.marker == InlineMarker::BoldAsterisk)
        {
            open.pop();
            return "*".to_owned();
        } else if can_open {
            open.push(OpenMarker {
                marker,
                content_start: index + length,
            });
        }
        index += length;
    }

    let mut closers = String::new();
    for entry in open.into_iter().rev() {
        if paragraph[entry.content_start..].chars().any(|character| {
            !character.is_whitespace() && !matches!(character, '*' | '_' | '~' | '`' | '$')
        }) {
            closers.push_str(entry.marker.closing());
        }
    }
    closers
}

#[cfg(test)]
mod tests {
    use super::mend_streaming_markdown;

    #[test]
    fn completes_inline_markers_without_changing_completed_markdown() {
        assert_eq!(
            mend_streaming_markdown("Writing **bold"),
            "Writing **bold**"
        );
        assert_eq!(mend_streaming_markdown("Use `cargo che"), "Use `cargo che`");
        assert_eq!(mend_streaming_markdown("***important"), "***important***");
        assert_eq!(
            mend_streaming_markdown("Already **done**"),
            "Already **done**"
        );
    }

    #[test]
    fn preserves_unterminated_fenced_code_for_the_block_parser() {
        let source = "Before\n\n```rust\nfn main() {";
        assert_eq!(mend_streaming_markdown(source), source);
    }

    #[test]
    fn makes_incomplete_links_safe_and_drops_incomplete_images() {
        assert_eq!(
            mend_streaming_markdown("Read [the guide"),
            "Read [the guide](streamdown:incomplete-link)"
        );
        assert_eq!(
            mend_streaming_markdown("Read [the guide](https://exam"),
            "Read [the guide](streamdown:incomplete-link)"
        );
        assert_eq!(mend_streaming_markdown("Look ![diagram"), "Look ");
    }

    #[test]
    fn guards_partial_setext_underlines_until_the_token_finishes() {
        assert_eq!(mend_streaming_markdown("Heading\n="), "Heading\n=\u{200b}");
        assert_eq!(mend_streaming_markdown("Heading\n==="), "Heading\n===");
    }
}
