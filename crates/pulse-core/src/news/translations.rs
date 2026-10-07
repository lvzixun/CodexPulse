use scraper::{Html, Selector};
use std::collections::BTreeMap;

/// Read published Chinese translations, never calendar snippets or executable markup.
pub fn translations_page(html: &str) -> Result<BTreeMap<String, String>, &'static str> {
    if html.len() > 512 * 1024 {
        return Err("translation_body_too_large");
    }
    let page = Html::parse_document(html);
    let rows = Selector::parse(".log-item[data-tweet-id], .watch-tweet[data-tweet-id]").unwrap();
    let paragraphs = Selector::parse(".log-item-text[data-role='tweet-display-text'], .watch-tweet-text[data-role='tweet-display-text']").unwrap();
    let mut result = BTreeMap::new();
    for row in page.select(&rows).take(100) {
        let Some(id) = row.value().attr("data-tweet-id") else {
            continue;
        };
        if id.is_empty() || id.len() > 128 {
            continue;
        }
        let Some(paragraph) = row.select(&paragraphs).next() else {
            continue;
        };
        // Unexpected active markup invalidates a row rather than leaking script text into UI.
        if paragraph
            .select(&Selector::parse("script,style,iframe").unwrap())
            .next()
            .is_some()
        {
            continue;
        }
        let text = paragraph
            .text()
            .collect::<String>()
            .trim()
            .chars()
            .take(4000)
            .collect::<String>();
        if !text.is_empty() {
            result.insert(id.into(), text);
        }
    }
    if result.is_empty() {
        return Err("unsupported_translation_page");
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_full_translations_by_id_without_calendar_snippets() {
        let page = r#"<a data-snippet="截断…"></a><li class="log-item" data-tweet-id="123"><p class="log-item-text" data-role="tweet-display-text">重置 &amp; 改进。<br>完整内容。</p></li><li class="log-item" data-tweet-id="observed-1"><p class="log-item-text" data-role="tweet-display-text">社区观察</p></li><script>private</script>"#;
        let rows = translations_page(page).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows["123"], "重置 & 改进。完整内容。");
        assert_eq!(rows["observed-1"], "社区观察");
    }
    #[test]
    fn rejects_changed_markup_and_bounds_records() {
        assert!(translations_page("<p>unknown</p>").is_err());
        assert!(translations_page(&"x".repeat(512 * 1024 + 1)).is_err());
        let rows = (0..110).map(|i|format!("<li class='log-item' data-tweet-id='{i}'><p class='log-item-text' data-role='tweet-display-text'>{}</p></li>","中".repeat(4100))).collect::<String>();
        // Construct a smaller document to exercise the independent row limit.
        assert!(translations_page(&rows).is_err());
        let rows = rows.replace(&"中".repeat(4100), "中文");
        assert_eq!(translations_page(&rows).unwrap().len(), 100);
    }
    #[test]
    fn watch_clues_use_published_body_not_quotes_or_scripts() {
        let page = r#"<a class="watch-tweet" data-tweet-id="123"><span class="watch-tweet-context">Not the reply body</span><span class="watch-tweet-text" data-role="tweet-display-text">接受你的投票 &amp; 继续观察</span></a><a class="watch-tweet" data-tweet-id="bad"><span class="watch-tweet-text" data-role="tweet-display-text"><script>not_visible</script></span></a>"#;
        let rows = translations_page(page).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows["123"], "接受你的投票 & 继续观察");
    }
}
