//! Markdown 渲染与消毒。修订保存时渲染一次，公开导出与后台预览使用同一份结果。
//!
//! - 原始 HTML 不执行，按文本转义显示。
//! - 图片只允许引用受控媒体：`![说明](media:<UUIDv7>)`，渲染为公开媒体地址并带尺寸。
//! - 标题生成稳定的 `id`，h2–h4 进入目录。
//! - 渲染结果再经 ammonia 按白名单消毒：链接只允许 http、https、mailto 和相对地址。

use ammonia::Builder;
use pulldown_cmark::{CowStr, Event, HeadingLevel, Options, Parser, Tag, TagEnd, html};
use silva_application::ports::{ImageSize, MarkdownRenderer, RenderedMarkdown};
use silva_domain::content::Heading;
use silva_shared::{
    error::{AppError, FieldError, FieldErrorCode, Result},
    ids::EntityId,
};
use std::{
    collections::{HashMap, HashSet},
    sync::LazyLock,
};
use uuid::Uuid;

/// 正文图片引用受控媒体使用的地址前缀。
const MEDIA_SCHEME: &str = "media:";
/// 公开媒体地址前缀；渲染的图片只指向这里。
pub const PUBLIC_MEDIA_PATH: &str = "/api/v1/public/media/";

fn options() -> Options {
    Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS
}

static SANITIZER: LazyLock<Builder<'static>> = LazyLock::new(|| {
    let mut builder = Builder::default();
    builder
        .url_schemes(HashSet::from(["http", "https", "mailto"]))
        .link_rel(Some("noopener noreferrer"))
        .add_tags(["input"])
        .add_tag_attributes("img", ["loading", "decoding"])
        .add_tag_attributes("input", ["type", "checked", "disabled"])
        .add_tag_attributes("code", ["class"])
        .add_tag_attributes("span", ["class", "style"])
        .attribute_filter(|element, attribute, value| match (element, attribute) {
            ("img", "src") => value.starts_with(PUBLIC_MEDIA_PATH).then(|| value.into()),
            ("span", "class") => matches!(value, "silva-img-wrap" | "silva-img-caption")
                .then(|| value.into()),
            ("span", "style") => parse_safe_color_style(value).map(Into::into),
            ("code", "class") => {
                let language = value.strip_prefix("language-")?;
                (!language.is_empty()
                    && language.bytes().all(|b| {
                        b.is_ascii_alphanumeric() || matches!(b, b'-' | b'+' | b'#' | b'_')
                    }))
                .then(|| value.into())
            }
            ("input", "type") => (value == "checkbox").then(|| value.into()),
            _ => Some(value.into()),
        });
    for level in ["h1", "h2", "h3", "h4", "h5", "h6"] {
        builder.add_tag_attributes(level, ["id"]);
    }
    builder
});

/// 基于 pulldown-cmark 与 ammonia 的渲染器。
#[derive(Default)]
pub struct CommonMarkRenderer;

impl MarkdownRenderer for CommonMarkRenderer {
    fn media_references(&self, field: &'static str, markdown: &str) -> Result<Vec<Uuid>> {
        let mut ids = Vec::new();
        let mut errors = Vec::new();
        for event in Parser::new_ext(markdown, options()) {
            if let Event::Start(Tag::Image { dest_url, .. }) = event {
                match parse_media_url(&dest_url) {
                    Some(id) if !ids.contains(&id) => ids.push(id),
                    Some(_) => {}
                    None => errors.push(FieldError::new(
                        field,
                        FieldErrorCode::InvalidValue,
                        "images must reference uploaded media as media:<id>",
                    )),
                }
            }
        }
        if errors.is_empty() {
            Ok(ids)
        } else {
            errors.truncate(1);
            Err(AppError::Validation(errors))
        }
    }

    fn render(&self, markdown: &str, media: &HashMap<Uuid, ImageSize>) -> RenderedMarkdown {
        let mut headings = Vec::new();
        let mut used = HashMap::new();
        let mut events = Vec::new();
        let mut parser = Parser::new_ext(markdown, options());
        while let Some(event) = parser.next() {
            match event {
                // 原始 HTML 不执行，按文本显示。
                Event::Html(raw) | Event::InlineHtml(raw) => events.push(Event::Text(raw)),
                Event::Start(Tag::Heading {
                    level,
                    classes,
                    attrs,
                    ..
                }) => {
                    let inner = collect_until(&mut parser, |end| matches!(end, TagEnd::Heading(_)));
                    let text = plain_text(&inner);
                    let slug = unique_slug(&mut used, &text);
                    let depth = heading_depth(level);
                    if (2..=4).contains(&depth) && !text.is_empty() {
                        headings.push(Heading {
                            depth,
                            slug: slug.clone(),
                            text,
                        });
                    }
                    events.push(Event::Start(Tag::Heading {
                        level,
                        id: Some(CowStr::from(slug)),
                        classes,
                        attrs,
                    }));
                    events.extend(inner.into_iter().map(escape_raw_html));
                    events.push(Event::End(TagEnd::Heading(level)));
                }
                Event::Start(Tag::Image {
                    dest_url, title, ..
                }) => {
                    let alt = plain_text(&collect_until(&mut parser, |end| {
                        matches!(end, TagEnd::Image)
                    }));
                    let trimmed_alt = alt.trim();
                    let alt_color = parse_alt_color_title(&title);
                    let image = parse_media_url(&dest_url).and_then(|id| {
                        media.get(&id).map(|size| {
                            let safe_alt = escape_attribute(trimmed_alt);
                            let img_tag = format!(
                                r#"<img src="{PUBLIC_MEDIA_PATH}{id}" width="{}" height="{}" alt="{safe_alt}" loading="lazy" decoding="async">"#,
                                size.width, size.height,
                            );
                            if trimmed_alt.is_empty() {
                                img_tag
                            } else {
                                let style_attr = alt_color
                                    .as_deref()
                                    .map(|hex| format!(r#" style="color:{hex}""#))
                                    .unwrap_or_default();
                                format!(
                                    r#"<span class="silva-img-wrap">{img_tag}<span class="silva-img-caption"{style_attr}>{safe_alt}</span></span>"#
                                )
                            }
                        })
                    });
                    // 未知媒体只保留说明文字；保存前已校验引用，这里只是兜底。
                    events.push(match image {
                        Some(tag) => Event::InlineHtml(CowStr::from(tag)),
                        None => Event::Text(CowStr::from(alt)),
                    });
                }
                other => events.push(other),
            }
        }
        let mut unsafe_html = String::with_capacity(markdown.len() * 3 / 2);
        html::push_html(&mut unsafe_html, events.into_iter());
        RenderedMarkdown {
            html: SANITIZER.clean(&unsafe_html).to_string(),
            headings,
        }
    }
}

/// 收集到匹配的结束标签为止（不含结束标签）。
fn collect_until<'a>(parser: &mut Parser<'a>, is_end: impl Fn(&TagEnd) -> bool) -> Vec<Event<'a>> {
    let mut depth = 0usize;
    let mut events = Vec::new();
    for event in parser.by_ref() {
        match &event {
            Event::Start(_) => depth += 1,
            Event::End(end) if depth == 0 && is_end(end) => break,
            Event::End(_) => depth = depth.saturating_sub(1),
            _ => {}
        }
        events.push(event);
    }
    events
}

fn escape_raw_html(event: Event<'_>) -> Event<'_> {
    match event {
        Event::Html(raw) | Event::InlineHtml(raw) => Event::Text(raw),
        other => other,
    }
}

fn plain_text(events: &[Event<'_>]) -> String {
    let mut text = String::new();
    for event in events {
        match event {
            Event::Text(value) | Event::Code(value) => text.push_str(value),
            Event::SoftBreak | Event::HardBreak => text.push(' '),
            _ => {}
        }
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn heading_depth(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// 生成标题 slug：转小写，保留字母、数字（含中文）、连字符与下划线，空白转为连字符；
/// 同一文档内重复时追加 `-1`、`-2`。
fn unique_slug(used: &mut HashMap<String, usize>, text: &str) -> String {
    let mut base = String::new();
    for c in text.to_lowercase().chars() {
        if c.is_alphanumeric() || c == '-' || c == '_' {
            base.push(c);
        } else if c.is_whitespace() {
            base.push('-');
        }
    }
    if base.is_empty() {
        base.push_str("section");
    }
    // used 的键是已使用的 slug，值是该 slug 作为基础时下一个尝试的序号。
    // 带序号的候选也可能与原文就是 `a-1` 这样的标题冲突，因此循环到未使用为止。
    let Some(&start) = used.get(&base) else {
        used.insert(base.clone(), 1);
        return base;
    };
    let mut next = start;
    loop {
        let candidate = format!("{base}-{next}");
        next += 1;
        if !used.contains_key(&candidate) {
            used.insert(base, next);
            used.insert(candidate.clone(), 1);
            return candidate;
        }
    }
}

fn escape_attribute(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '"' => escaped.push_str("&quot;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            _ => escaped.push(c),
        }
    }
    escaped
}

/// 解析 `media:<UUIDv7>`；其他地址返回 None。
fn is_hex_color(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(|b| b.is_ascii_hexdigit())
}

fn parse_alt_color_title(title: &str) -> Option<String> {
    let raw = title.trim().strip_prefix("alt-color:")?;
    is_hex_color(raw).then(|| raw.to_ascii_lowercase())
}

fn parse_safe_color_style(value: &str) -> Option<String> {
    let raw = value.trim().strip_prefix("color:")?.trim().trim_end_matches(';').trim();
    is_hex_color(raw).then(|| format!("color:{}", raw.to_ascii_lowercase()))
}

fn parse_media_url(url: &str) -> Option<Uuid> {
    url.strip_prefix(MEDIA_SCHEME)?
        .parse::<EntityId>()
        .ok()
        .map(EntityId::as_uuid)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(markdown: &str) -> RenderedMarkdown {
        CommonMarkRenderer.render(markdown, &HashMap::new())
    }

    #[test]
    fn renders_gfm_and_heading_ids() {
        let rendered = render(
            "# 标题\n\n## 三个版本指针\n\nText **bold** ~~old~~\n\n## 三个版本指针\n\n### Code `x`\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n- [x] done\n",
        );
        assert!(
            rendered
                .html
                .contains(r#"<h2 id="三个版本指针">三个版本指针</h2>"#)
        );
        assert!(rendered.html.contains(r#"<h2 id="三个版本指针-1">"#));
        assert!(rendered.html.contains("<strong>bold</strong>"));
        assert!(rendered.html.contains("<del>old</del>"));
        assert!(rendered.html.contains("<table>"));
        assert!(rendered.html.contains(r#"type="checkbox""#));
        assert_eq!(
            rendered
                .headings
                .iter()
                .map(|h| (h.depth, h.slug.as_str(), h.text.as_str()))
                .collect::<Vec<_>>(),
            [
                (2, "三个版本指针", "三个版本指针"),
                (2, "三个版本指针-1", "三个版本指针"),
                (3, "code-x", "Code x"),
            ]
        );
    }

    #[test]
    fn heading_ids_stay_unique_when_text_looks_like_a_suffix() {
        let rendered = render("## a-1\n\n## a\n\n## a\n\n## a\n");
        let slugs: Vec<_> = rendered.headings.iter().map(|h| h.slug.as_str()).collect();
        assert_eq!(slugs, ["a-1", "a", "a-2", "a-3"]);
        for slug in slugs {
            assert_eq!(
                rendered.html.matches(&format!(r#"id="{slug}""#)).count(),
                1,
                "{slug}"
            );
        }
    }

    #[test]
    fn raw_html_and_dangerous_links_are_neutralized() {
        let rendered = render(
            "<script>alert(1)</script>\n\nhi <img src=x onerror=alert(1)> [a](javascript:alert(1)) [b](https://example.com) [c](/blog/x/)\n\n## <b>x</b>",
        );
        assert!(!rendered.html.contains("<script"));
        assert!(!rendered.html.contains("<img"));
        assert!(rendered.html.contains("&lt;script&gt;"));
        assert!(!rendered.html.contains("javascript:"));
        assert!(rendered.html.contains(r#"href="https://example.com""#));
        assert!(rendered.html.contains(r#"href="/blog/x/""#));
        assert!(rendered.html.contains("noopener"));
        assert!(!rendered.html.contains("<b>"));
    }

    #[test]
    fn code_blocks_keep_only_safe_language_classes() {
        let rendered = render("```rust\nfn main() {}\n```\n\n```bad\" onclick=\"x\n1\n```");
        assert!(rendered.html.contains(r#"<code class="language-rust">"#));
        assert!(!rendered.html.contains("onclick"));
    }

    #[test]
    fn images_must_reference_media_and_render_with_size() {
        let id = Uuid::now_v7();
        let markdown = format!("![封面 \"图\"](media:{id})\n\n![again](media:{id})");
        assert_eq!(
            CommonMarkRenderer
                .media_references("markdown_body", &markdown)
                .unwrap(),
            [id]
        );
        let media = HashMap::from([(
            id,
            ImageSize {
                width: 1600,
                height: 900,
            },
        )]);
        let html = CommonMarkRenderer.render(&markdown, &media).html;
        assert!(html.contains(&format!(
            r#"<img src="{PUBLIC_MEDIA_PATH}{id}" width="1600" height="900" alt="封面 &quot;图&quot;""#
        )));
        assert!(html.contains(r#"<span class="silva-img-caption">封面 "图"</span>"#));
        let colored_html = CommonMarkRenderer
            .render(
                &format!(r#"![绿色说明](media:{id} "alt-color:#137A4D")"#),
                &media,
            )
            .html;
        assert!(
            colored_html
                .contains(r#"<span class="silva-img-caption" style="color:#137a4d">绿色说明</span>"#)
        );
        let empty_alt_html = CommonMarkRenderer
            .render(&format!("![](media:{id})"), &media)
            .html;
        assert!(!empty_alt_html.contains("silva-img-caption"));
        for invalid in [
            "![x](https://example.com/a.png)",
            "![x](media:not-an-id)",
            "![x](media:67e55044-10b1-426f-9247-bb680e5fe0c8)",
        ] {
            assert!(
                CommonMarkRenderer
                    .media_references("markdown_body", invalid)
                    .is_err(),
                "{invalid}"
            );
        }
        // 未知媒体只保留说明文字。
        assert_eq!(
            CommonMarkRenderer
                .render(&format!("![说明](media:{id})"), &HashMap::new())
                .html,
            "<p>说明</p>\n"
        );
    }
}
