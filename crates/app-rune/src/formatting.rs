//! Formatting tags emitted by the composer, with a finite HTML allowlist.
use rune::TextEdit;

pub const COLORS: [(&str, &str); 6] = [
    ("Default", "#242424"),
    ("Red", "#C4314B"),
    ("Green", "#237B4B"),
    ("Blue", "#5B5FC7"),
    ("Yellow", "#FFF100"),
    ("White", "#FFFFFF"),
];

pub fn tags() -> Vec<String> {
    let mut tags: Vec<_> = [
        "<u>",
        "</u>",
        "<sup>",
        "</sup>",
        "<sub>",
        "</sub>",
        "</span>",
        "<hr>",
        "<div style=\"margin-left:40px\">",
        "</div>",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    for (_, color) in COLORS {
        for property in ["color", "background-color"] {
            tags.push(format!("<span style=\"{property}:{color}\">"));
        }
    }
    for size in ["xx-small", "x-large"] {
        tags.push(format!("<span style=\"font-size:{size}\">"));
    }
    tags
}

pub fn outdent(edit: &mut TextEdit) {
    let (start, end) = edit.range();
    let prefix = "<div style=\"margin-left:40px\">";
    if edit.text[..start].ends_with(prefix) && edit.text[end..].starts_with("</div>") {
        let text = edit.text[start..end].to_owned();
        edit.set_selection(start - prefix.len(), end + "</div>".len());
        edit.insert(&text);
        edit.set_selection(start - prefix.len(), start - prefix.len() + text.len());
        return;
    }
    let selected = &edit.text[start..end];
    let text = selected
        .strip_prefix("<div style=\"margin-left:40px\">")
        .and_then(|text| text.strip_suffix("</div>"))
        .unwrap_or(selected)
        .to_owned();
    edit.insert(&text);
    edit.set_selection(start, start + text.len());
}

pub fn clear(edit: &mut TextEdit) {
    let (start, end) = edit.range();
    let html = crate::composed::html(&edit.text[start..end], &[]);
    let text = teams_core::Draft::from_spans(&teams_core::html_to_spans(&html)).plain_text();
    edit.insert(&text);
    edit.set_selection(start, start + text.len());
}

pub fn table(edit: &mut TextEdit, action: teams_core::TableEdit) -> bool {
    let lines: Vec<_> = edit.text.split_inclusive('\n').collect();
    let mut offset = 0;
    let mut current = 0;
    for (index, line) in lines.iter().enumerate() {
        if offset <= edit.focus {
            current = index;
        }
        offset += line.len();
    }
    if lines.is_empty() || !lines[current].trim().starts_with('|') {
        return false;
    }
    let mut first = current;
    let mut last = current + 1;
    while first > 0 && lines[first - 1].trim().starts_with('|') {
        first -= 1;
    }
    while last < lines.len() && lines[last].trim().starts_with('|') {
        last += 1;
    }
    let start: usize = lines[..first].iter().map(|line| line.len()).sum();
    let end: usize = lines[..last].iter().map(|line| line.len()).sum();
    let draft = teams_core::Draft::from_markdown(&edit.text[start..end]);
    let Some(mut table) = draft
        .lines()
        .iter()
        .enumerate()
        .find_map(|(index, _)| draft.table(index).cloned())
    else {
        return false;
    };
    let line_start: usize = lines[..current].iter().map(|line| line.len()).sum();
    let prefix = &edit.text[line_start..edit.focus.min(line_start + lines[current].len())];
    let mut escaped = false;
    let mut pipes: usize = 0;
    for character in prefix.chars() {
        if character == '|' && !escaped {
            pipes += 1;
        }
        escaped = character == '\\' && !escaped;
    }
    let row = (current - first).saturating_sub(usize::from(current > first));
    let cell = teams_core::Cell::new(
        row.min(table.row_count() - 1),
        pipes.saturating_sub(1).min(table.column_count() - 1),
    );
    let replacement = if table.apply(action, cell).is_none() {
        String::new()
    } else {
        let mut rows: Vec<_> = table
            .rows
            .iter()
            .map(|row| {
                format!(
                    "| {} |",
                    row.iter()
                        .map(|cell| cell.replace('|', "\\|"))
                        .collect::<Vec<_>>()
                        .join(" | ")
                )
            })
            .collect();
        rows.insert(
            1,
            format!("| {} |", vec!["---"; table.column_count()].join(" | ")),
        );
        format!("{}\n", rows.join("\n"))
    };
    edit.set_selection(start, end);
    edit.insert(&replacement);
    edit.set_selection(start, start);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn table_changes_keep_surrounding_text_and_escaped_cells() {
        let mut edit = TextEdit::new("Before\n| A | B |\n| --- | --- |\n| c | d |\nAfter");
        let at = edit.text.find("c |").unwrap();
        edit.set_selection(at, at);
        assert!(table(&mut edit, teams_core::TableEdit::InsertColumnRight));
        assert!(edit.text.starts_with("Before\n| A |  | B |"));
        assert!(edit.text.ends_with("After"));
    }
    #[test]
    fn outdent_removes_the_indent_around_the_selected_content() {
        let prefix = "<div style=\"margin-left:40px\">";
        let mut edit = TextEdit::new(format!("{prefix}Hello</div>"));
        edit.set_selection(prefix.len(), prefix.len() + 5);
        outdent(&mut edit);
        assert_eq!(edit.text, "Hello");
    }

    #[test]
    fn formatting_survives_send_but_arbitrary_tags_stay_escaped() {
        let html = crate::composed::html(
            "<span style=\"color:#C4314B\">Red</span><script>bad</script>",
            &[],
        );
        assert!(html.contains("<span style=\"color:#C4314B\">Red</span>"));
        assert!(html.contains("&lt;script&gt;"));
    }
}
