use scraper::Html;

pub fn plain_text(html: &str) -> String {
    Html::parse_fragment(html)
        .root_element()
        .descendants()
        .filter_map(|node| node.value().as_text().map(|text| (node, text)))
        .filter(|(node, _)| {
            !node.ancestors().any(|ancestor| {
                ancestor
                    .value()
                    .as_element()
                    .is_some_and(|element| matches!(element.name(), "script" | "style"))
            })
        })
        .map(|(_, text)| text.text.as_ref())
        .flat_map(str::split_whitespace)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excludes_script_and_style_content() {
        assert_eq!(
            plain_text(
                "<p>Hello</p><script>secret()</script><style>.hidden { color: red }</style><p>world</p>"
            ),
            "Hello world"
        );
    }

    #[test]
    fn strips_tags_and_collapses_whitespace() {
        assert_eq!(
            plain_text("<p>Hello <b>big</b></p>\n<p>world &amp; co</p>"),
            "Hello big world & co"
        );
    }
}
