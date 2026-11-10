//! Channel conversations: replies sit under the post they answer, as in the Teams channel view.
//! The bridge keeps chronological order; the view calls `group_replies` before showing a list.

use std::collections::{HashMap, HashSet};

use crate::types::MessageRow;

/// Moves each reply directly under its root post. Posts keep their order, replies keep theirs.
/// A reply whose root is not in `rows` (older than the loaded window) stays where it is.
pub fn group_replies(rows: Vec<MessageRow>) -> Vec<MessageRow> {
    let roots: HashSet<String> = rows
        .iter()
        .filter(|row| row.reply_to_id.is_none())
        .map(|row| row.id.clone())
        .collect();
    let (replies, others): (Vec<_>, Vec<_>) = rows.into_iter().partition(|row| {
        row.reply_to_id.as_ref().is_some_and(|root| roots.contains(root))
    });
    let mut by_root: HashMap<String, Vec<MessageRow>> = HashMap::new();
    for reply in replies {
        if let Some(root) = reply.reply_to_id.clone() {
            by_root.entry(root).or_default().push(reply);
        }
    }
    others
        .into_iter()
        .flat_map(|row| {
            let replies = by_root.remove(&row.id).unwrap_or_default();
            std::iter::once(row).chain(replies)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, reply_to: Option<&str>) -> MessageRow {
        MessageRow {
            id: id.into(),
            reply_to_id: reply_to.map(str::to_owned),
            deleted: false,
            can_delete: false,
            reactions: vec![],
            sender: "Ada".into(),
            when: String::new(),
            text: id.into(),
            mine: false,
            media: Default::default(),
        }
    }

    fn ids(rows: &[MessageRow]) -> Vec<&str> {
        rows.iter().map(|row| row.id.as_str()).collect()
    }

    #[test]
    fn replies_follow_their_post_and_keep_order() {
        let rows = vec![
            row("a", None),
            row("b", None),
            row("a1", Some("a")),
            row("b1", Some("b")),
            row("a2", Some("a")),
        ];
        assert_eq!(ids(&group_replies(rows)), ["a", "a1", "a2", "b", "b1"]);
    }

    #[test]
    fn orphan_replies_stay_in_place() {
        let rows = vec![row("old", Some("gone")), row("a", None), row("a1", Some("a"))];
        assert_eq!(ids(&group_replies(rows)), ["old", "a", "a1"]);
    }

    #[test]
    fn chats_without_replies_are_unchanged() {
        let rows = vec![row("x", None), row("y", None)];
        assert_eq!(ids(&group_replies(rows)), ["x", "y"]);
    }
}
