use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GfmDocument {
    pub blocks: Vec<GfmBlock>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum GfmBlock {
    Paragraph {
        inlines: Vec<GfmInline>,
    },
    Heading {
        level: u8,
        inlines: Vec<GfmInline>,
    },
    CodeBlock {
        language: Option<String>,
        code: String,
    },
    BlockQuote {
        blocks: Vec<GfmBlock>,
    },
    List {
        ordered: bool,
        start: Option<u64>,
        items: Vec<GfmListItem>,
    },
    Table {
        alignments: Vec<GfmAlignment>,
        headers: Vec<Vec<GfmInline>>,
        rows: Vec<Vec<Vec<GfmInline>>>,
    },
    ThematicBreak,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GfmListItem {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checked: Option<bool>,
    pub blocks: Vec<GfmBlock>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GfmAlignment {
    None,
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum GfmInline {
    Text {
        text: String,
    },
    Code {
        code: String,
    },
    Strong {
        children: Vec<GfmInline>,
    },
    Emphasis {
        children: Vec<GfmInline>,
    },
    Strikethrough {
        children: Vec<GfmInline>,
    },
    Link {
        url: String,
        title: Option<String>,
        children: Vec<GfmInline>,
    },
    Image {
        url: String,
        alt: String,
        title: Option<String>,
    },
    SoftBreak,
    HardBreak,
}

enum Frame {
    Root(Vec<GfmBlock>),
    Paragraph(Vec<GfmInline>),
    Heading(u8, Vec<GfmInline>),
    CodeBlock {
        language: Option<String>,
        code: String,
    },
    BlockQuote(Vec<GfmBlock>),
    List {
        ordered: bool,
        start: Option<u64>,
        items: Vec<GfmListItem>,
    },
    Item {
        checked: Option<bool>,
        blocks: Vec<GfmBlock>,
        trailing_inlines: Vec<GfmInline>,
    },
    Table {
        alignments: Vec<GfmAlignment>,
        in_head: bool,
        headers: Vec<Vec<GfmInline>>,
        rows: Vec<Vec<Vec<GfmInline>>>,
        current_row: Vec<Vec<GfmInline>>,
    },
    TableCell(Vec<GfmInline>),
    Strong(Vec<GfmInline>),
    Emphasis(Vec<GfmInline>),
    Strikethrough(Vec<GfmInline>),
    Link {
        url: String,
        title: Option<String>,
        children: Vec<GfmInline>,
    },
    Image {
        url: String,
        title: Option<String>,
        children: Vec<GfmInline>,
    },
}

pub fn parse_gfm_markdown(source: &str) -> GfmDocument {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(source, opts);
    let mut stack = vec![Frame::Root(Vec::new())];

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => stack.push(Frame::Paragraph(Vec::new())),
                Tag::Heading { level, .. } => {
                    let num = match level {
                        HeadingLevel::H1 => 1,
                        HeadingLevel::H2 => 2,
                        HeadingLevel::H3 => 3,
                        HeadingLevel::H4 => 4,
                        HeadingLevel::H5 => 5,
                        HeadingLevel::H6 => 6,
                    };
                    stack.push(Frame::Heading(num, Vec::new()));
                }
                Tag::CodeBlock(kind) => {
                    let language = match kind {
                        CodeBlockKind::Fenced(lang) => {
                            let trimmed = lang.split_whitespace().next().unwrap_or("").trim();
                            (!trimmed.is_empty()).then(|| trimmed.to_string())
                        }
                        CodeBlockKind::Indented => None,
                    };
                    stack.push(Frame::CodeBlock {
                        language,
                        code: String::new(),
                    });
                }
                Tag::BlockQuote(_) => stack.push(Frame::BlockQuote(Vec::new())),
                Tag::List(start) => stack.push(Frame::List {
                    ordered: start.is_some(),
                    start,
                    items: Vec::new(),
                }),
                Tag::Item => stack.push(Frame::Item {
                    checked: None,
                    blocks: Vec::new(),
                    trailing_inlines: Vec::new(),
                }),
                Tag::Table(alignments) => stack.push(Frame::Table {
                    alignments: alignments
                        .into_iter()
                        .map(|a| match a {
                            Alignment::None => GfmAlignment::None,
                            Alignment::Left => GfmAlignment::Left,
                            Alignment::Center => GfmAlignment::Center,
                            Alignment::Right => GfmAlignment::Right,
                        })
                        .collect(),
                    in_head: false,
                    headers: Vec::new(),
                    rows: Vec::new(),
                    current_row: Vec::new(),
                }),
                Tag::TableHead => {
                    if let Some(Frame::Table {
                        in_head,
                        current_row,
                        ..
                    }) = stack.last_mut()
                    {
                        *in_head = true;
                        current_row.clear();
                    }
                }
                Tag::TableRow => {
                    if let Some(Frame::Table { current_row, .. }) = stack.last_mut() {
                        current_row.clear();
                    }
                }
                Tag::TableCell => stack.push(Frame::TableCell(Vec::new())),
                Tag::Strong => stack.push(Frame::Strong(Vec::new())),
                Tag::Emphasis => stack.push(Frame::Emphasis(Vec::new())),
                Tag::Strikethrough => stack.push(Frame::Strikethrough(Vec::new())),
                Tag::Link {
                    dest_url, title, ..
                } => stack.push(Frame::Link {
                    url: dest_url.into_string(),
                    title: (!title.is_empty()).then(|| title.into_string()),
                    children: Vec::new(),
                }),
                Tag::Image {
                    dest_url, title, ..
                } => stack.push(Frame::Image {
                    url: dest_url.into_string(),
                    title: (!title.is_empty()).then(|| title.into_string()),
                    children: Vec::new(),
                }),
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Paragraph => {
                    if let Some(Frame::Paragraph(inlines)) = stack.pop() {
                        push_block(
                            &mut stack,
                            GfmBlock::Paragraph {
                                inlines: expand_gfm_autolinks_in_inlines(inlines),
                            },
                        );
                    }
                }
                TagEnd::Heading(_) => {
                    if let Some(Frame::Heading(level, inlines)) = stack.pop() {
                        push_block(
                            &mut stack,
                            GfmBlock::Heading {
                                level,
                                inlines: expand_gfm_autolinks_in_inlines(inlines),
                            },
                        );
                    }
                }
                TagEnd::CodeBlock => {
                    if let Some(Frame::CodeBlock { language, mut code }) = stack.pop() {
                        if code.ends_with('\n') {
                            code.pop();
                        }
                        push_block(&mut stack, GfmBlock::CodeBlock { language, code });
                    }
                }
                TagEnd::BlockQuote(_) => {
                    if let Some(Frame::BlockQuote(blocks)) = stack.pop() {
                        push_block(&mut stack, GfmBlock::BlockQuote { blocks });
                    }
                }
                TagEnd::List(_) => {
                    if let Some(Frame::List {
                        ordered,
                        start,
                        items,
                    }) = stack.pop()
                    {
                        push_block(
                            &mut stack,
                            GfmBlock::List {
                                ordered,
                                start,
                                items,
                            },
                        );
                    }
                }
                TagEnd::Item => {
                    if let Some(Frame::Item {
                        checked,
                        mut blocks,
                        trailing_inlines,
                    }) = stack.pop()
                    {
                        if !trailing_inlines.is_empty() {
                            blocks.push(GfmBlock::Paragraph {
                                inlines: expand_gfm_autolinks_in_inlines(trailing_inlines),
                            });
                        }
                        if let Some(Frame::List { items, .. }) = stack.last_mut() {
                            items.push(GfmListItem { checked, blocks });
                        }
                    }
                }
                TagEnd::TableHead => {
                    if let Some(Frame::Table {
                        in_head,
                        headers,
                        current_row,
                        ..
                    }) = stack.last_mut()
                    {
                        *headers = std::mem::take(current_row);
                        *in_head = false;
                    }
                }
                TagEnd::TableRow => {
                    if let Some(Frame::Table {
                        in_head,
                        rows,
                        current_row,
                        ..
                    }) = stack.last_mut()
                    {
                        if !*in_head {
                            rows.push(std::mem::take(current_row));
                        }
                    }
                }
                TagEnd::TableCell => {
                    if let Some(Frame::TableCell(inlines)) = stack.pop() {
                        if let Some(Frame::Table { current_row, .. }) = stack.last_mut() {
                            current_row.push(expand_gfm_autolinks_in_inlines(inlines));
                        }
                    }
                }
                TagEnd::Table => {
                    if let Some(Frame::Table {
                        alignments,
                        headers,
                        rows,
                        ..
                    }) = stack.pop()
                    {
                        push_block(
                            &mut stack,
                            GfmBlock::Table {
                                alignments,
                                headers,
                                rows,
                            },
                        );
                    }
                }
                TagEnd::Strong => {
                    if let Some(Frame::Strong(children)) = stack.pop() {
                        let inside_link = stack
                            .iter()
                            .any(|f| matches!(f, Frame::Link { .. } | Frame::Image { .. }));
                        let children = if inside_link {
                            children
                        } else {
                            expand_gfm_autolinks_in_inlines(children)
                        };
                        push_inline(&mut stack, GfmInline::Strong { children });
                    }
                }
                TagEnd::Emphasis => {
                    if let Some(Frame::Emphasis(children)) = stack.pop() {
                        let inside_link = stack
                            .iter()
                            .any(|f| matches!(f, Frame::Link { .. } | Frame::Image { .. }));
                        let children = if inside_link {
                            children
                        } else {
                            expand_gfm_autolinks_in_inlines(children)
                        };
                        push_inline(&mut stack, GfmInline::Emphasis { children });
                    }
                }
                TagEnd::Strikethrough => {
                    if let Some(Frame::Strikethrough(children)) = stack.pop() {
                        let inside_link = stack
                            .iter()
                            .any(|f| matches!(f, Frame::Link { .. } | Frame::Image { .. }));
                        let children = if inside_link {
                            children
                        } else {
                            expand_gfm_autolinks_in_inlines(children)
                        };
                        push_inline(&mut stack, GfmInline::Strikethrough { children });
                    }
                }
                TagEnd::Link => {
                    if let Some(Frame::Link {
                        url,
                        title,
                        children,
                    }) = stack.pop()
                    {
                        push_inline(
                            &mut stack,
                            GfmInline::Link {
                                url,
                                title,
                                children,
                            },
                        );
                    }
                }
                TagEnd::Image => {
                    if let Some(Frame::Image {
                        url,
                        title,
                        children,
                    }) = stack.pop()
                    {
                        let alt = plain_text(&children);
                        push_inline(&mut stack, GfmInline::Image { url, alt, title });
                    }
                }
                _ => {}
            },
            Event::Text(text) => push_text(&mut stack, &text),
            // Align with react-markdown without rehype-raw: raw HTML is literal text, never executed/rendered as DOM.
            Event::Html(html) | Event::InlineHtml(html) => push_text(&mut stack, &html),
            Event::Code(code) => push_inline(
                &mut stack,
                GfmInline::Code {
                    code: code.into_string(),
                },
            ),
            Event::SoftBreak => push_inline(&mut stack, GfmInline::SoftBreak),
            Event::HardBreak => push_inline(&mut stack, GfmInline::HardBreak),
            Event::Rule => push_block(&mut stack, GfmBlock::ThematicBreak),
            Event::TaskListMarker(done) => {
                if let Some(Frame::Item { checked, .. }) = stack.last_mut() {
                    *checked = Some(done);
                }
            }
            _ => {}
        }
    }

    match stack.pop() {
        Some(Frame::Root(blocks)) => GfmDocument { blocks },
        _ => GfmDocument { blocks: Vec::new() },
    }
}

fn plain_text(inlines: &[GfmInline]) -> String {
    let mut out = String::new();
    for item in inlines {
        match item {
            GfmInline::Text { text } => out.push_str(text),
            GfmInline::Code { code } => out.push_str(code),
            GfmInline::Strong { children }
            | GfmInline::Emphasis { children }
            | GfmInline::Strikethrough { children }
            | GfmInline::Link { children, .. } => out.push_str(&plain_text(children)),
            GfmInline::Image { alt, .. } => out.push_str(alt),
            GfmInline::SoftBreak | GfmInline::HardBreak => out.push('\n'),
        }
    }
    out
}

fn push_text(stack: &mut [Frame], value: &str) {
    if let Some(Frame::CodeBlock { code, .. }) = stack.last_mut() {
        code.push_str(value);
        return;
    }
    push_inline(
        stack,
        GfmInline::Text {
            text: value.to_string(),
        },
    );
}

fn expand_gfm_autolinks_in_inlines(inlines: Vec<GfmInline>) -> Vec<GfmInline> {
    let mut out = Vec::with_capacity(inlines.len());
    for inline in inlines {
        match inline {
            GfmInline::Text { text } => {
                for piece in split_gfm_autolink_literals(&text) {
                    merge_or_push_inline(&mut out, piece);
                }
            }
            other => out.push(other),
        }
    }
    out
}

fn is_autolink_boundary_prev(prev: Option<char>) -> bool {
    match prev {
        None => true,
        Some(ch) => ch.is_whitespace() || matches!(ch, '(' | '[' | '{' | '<' | '*' | '_' | '~'),
    }
}

fn trim_gfm_autolink_candidate(raw: &str) -> &str {
    let mut len = raw.len();
    while len > 0 {
        let slice = &raw[..len];
        let Some(last) = slice.chars().next_back() else {
            break;
        };
        let drop_char = match last {
            '?' | '!' | '.' | ',' | ':' | '*' | '_' | '~' | '\'' | '"' => true,
            ')' => {
                let opens = slice.bytes().filter(|&b| b == b'(').count();
                let closes = slice.bytes().filter(|&b| b == b')').count();
                closes > opens
            }
            ']' => {
                let opens = slice.bytes().filter(|&b| b == b'[').count();
                let closes = slice.bytes().filter(|&b| b == b']').count();
                closes > opens
            }
            ';' => {
                // Strip trailing HTML entity-like `&...;` or bare `;`
                true
            }
            _ => false,
        };
        if drop_char {
            len -= last.len_utf8();
        } else {
            break;
        }
    }
    &raw[..len]
}

fn valid_autolink_domain(after_prefix: &str) -> bool {
    let host_end = after_prefix
        .find(['/', '?', '#'])
        .unwrap_or(after_prefix.len());
    let host = &after_prefix[..host_end];
    if host.is_empty() || !host.contains('.') || host.ends_with('.') || host.contains('_') {
        return false;
    }
    host.split('.').all(|part| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == ':')
    })
}

fn split_gfm_autolink_literals(value: &str) -> Vec<GfmInline> {
    if !value.contains("http://") && !value.contains("https://") && !value.contains("www.") {
        return vec![GfmInline::Text {
            text: value.to_string(),
        }];
    }
    let mut out: Vec<GfmInline> = Vec::new();
    let mut plain_start = 0usize;
    let mut idx = 0usize;

    while idx < value.len() {
        let rest = &value[idx..];
        let prefix_info = if rest.starts_with("https://") {
            Some(("https://", false))
        } else if rest.starts_with("http://") {
            Some(("http://", false))
        } else if rest.starts_with("www.") {
            Some(("www.", true))
        } else {
            None
        };

        let prev_char = value[..idx].chars().next_back();
        if let Some((prefix, needs_http_scheme)) = prefix_info {
            if is_autolink_boundary_prev(prev_char) {
                let token_end = rest
                    .find(|c: char| c.is_whitespace() || c == '<' || c == '>')
                    .unwrap_or(rest.len());
                let trimmed = trim_gfm_autolink_candidate(&rest[..token_end]);
                let after_prefix = trimmed.strip_prefix(prefix).unwrap_or("");
                if trimmed.len() > prefix.len() && valid_autolink_domain(after_prefix) {
                    if plain_start < idx {
                        merge_or_push_inline(
                            &mut out,
                            GfmInline::Text {
                                text: value[plain_start..idx].to_string(),
                            },
                        );
                    }
                    let url = if needs_http_scheme {
                        format!("http://{trimmed}")
                    } else {
                        trimmed.to_string()
                    };
                    out.push(GfmInline::Link {
                        url,
                        title: None,
                        children: vec![GfmInline::Text {
                            text: trimmed.to_string(),
                        }],
                    });
                    idx += trimmed.len();
                    plain_start = idx;
                    continue;
                }
            }
        }
        let step = rest.chars().next().map_or(1, char::len_utf8);
        idx += step;
    }

    if plain_start < value.len() {
        merge_or_push_inline(
            &mut out,
            GfmInline::Text {
                text: value[plain_start..].to_string(),
            },
        );
    }
    if out.is_empty() {
        out.push(GfmInline::Text {
            text: String::new(),
        });
    }
    out
}

fn push_inline(stack: &mut [Frame], inline: GfmInline) {
    match stack.last_mut() {
        Some(
            Frame::Paragraph(inlines)
            | Frame::Heading(_, inlines)
            | Frame::TableCell(inlines)
            | Frame::Strong(inlines)
            | Frame::Emphasis(inlines)
            | Frame::Strikethrough(inlines),
        ) => merge_or_push_inline(inlines, inline),
        Some(Frame::Link { children, .. } | Frame::Image { children, .. }) => {
            merge_or_push_inline(children, inline)
        }
        Some(Frame::Item {
            trailing_inlines, ..
        }) => merge_or_push_inline(trailing_inlines, inline),
        Some(Frame::Root(blocks) | Frame::BlockQuote(blocks)) => {
            blocks.push(GfmBlock::Paragraph {
                inlines: vec![inline],
            });
        }
        _ => {}
    }
}

fn merge_or_push_inline(inlines: &mut Vec<GfmInline>, inline: GfmInline) {
    if let (Some(GfmInline::Text { text: prev }), GfmInline::Text { text: next }) =
        (inlines.last_mut(), &inline)
    {
        prev.push_str(next);
    } else {
        inlines.push(inline);
    }
}

fn push_block(stack: &mut [Frame], block: GfmBlock) {
    match stack.last_mut() {
        Some(Frame::Root(blocks) | Frame::BlockQuote(blocks)) => blocks.push(block),
        Some(Frame::Item {
            blocks,
            trailing_inlines,
            ..
        }) => {
            if !trailing_inlines.is_empty() {
                blocks.push(GfmBlock::Paragraph {
                    inlines: std::mem::take(trailing_inlines),
                });
            }
            blocks.push(block);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gfm_tables_tasklists_strikethrough_and_escapes_raw_html() {
        let md = "# Title\n\n- [x] ~~done~~ `code`\n- [ ] pending\n\n| Col | Num |\n| :--- | ---: |\n| a | 1 |\n\n<script>alert(1)</script>\n";
        let doc = parse_gfm_markdown(md);
        assert!(matches!(doc.blocks[0], GfmBlock::Heading { level: 1, .. }));
        let GfmBlock::List { ordered, items, .. } = &doc.blocks[1] else {
            panic!("expected list");
        };
        assert!(!ordered);
        assert_eq!(items[0].checked, Some(true));
        assert_eq!(items[1].checked, Some(false));

        let GfmBlock::Table {
            alignments,
            headers,
            rows,
        } = &doc.blocks[2]
        else {
            panic!("expected table");
        };
        assert_eq!(alignments, &[GfmAlignment::Left, GfmAlignment::Right]);
        assert_eq!(headers.len(), 2);
        assert_eq!(rows.len(), 1);

        let last_json = serde_json::to_string(&doc.blocks.last().unwrap()).unwrap();
        assert!(last_json.contains("<script>alert(1)</script>"));
    }

    #[test]
    fn parses_gfm_extended_autolinks_without_corrupting_markdown_links_or_code() {
        let md = "See https://example.com/wiki/Rust_(lang), (visit www.example.com.), `https://example.com/raw`, and [https://example.com](https://github.com/lich13/agenthub).";
        let doc = parse_gfm_markdown(md);
        let GfmBlock::Paragraph { inlines } = &doc.blocks[0] else {
            panic!("expected paragraph");
        };
        let links: Vec<(&str, String)> = inlines
            .iter()
            .filter_map(|inline| match inline {
                GfmInline::Link { url, children, .. } => Some((url.as_str(), plain_text(children))),
                _ => None,
            })
            .collect();
        assert_eq!(
            links,
            vec![
                (
                    "https://example.com/wiki/Rust_(lang)",
                    "https://example.com/wiki/Rust_(lang)".to_string(),
                ),
                ("http://www.example.com", "www.example.com".to_string()),
                (
                    "https://github.com/lich13/agenthub",
                    "https://example.com".to_string(),
                ),
            ]
        );
    }
}
