//! Channel conversations: replies sit under the post they answer, as in the Teams channel view.
//! The bridge keeps chronological order; the view calls `group_replies` before showing a list.

use std::collections::{HashMap, HashSet};

use crate::types::MessageRow;

/// One card per post; an unloaded root leaves one card for its cached replies.
pub fn post_indices(rows: &[MessageRow]) -> Vec<usize> {
    let roots: HashSet<_> = rows.iter().filter(|m| m.reply_to_id.is_none()).map(|m| m.id.as_str()).collect();
    let mut orphans = HashSet::new();
    rows.iter().enumerate().filter_map(|(i, row)| match row.reply_to_id.as_deref() {
        None => Some(i),
        Some(root) if !roots.contains(root) && orphans.insert(root) => Some(i),
        _ => None,
    }).collect()
}

pub fn overview<A: crate::types::Actions + 'static>(s: &crate::ui::UiState<A>) -> rune::El {
    use rune::*;
    use crate::ui::{c, text, ts, UiState};
    let mut tiles = col().w_full().equal_columns(2).gap(16.0);
    for team in &s.teams {
        let team_id = team.id.clone();
        let channel = team.channels.first().map(|c| c.id.clone());
        let mut tile = col().key(format!("team-card-{}", team.id)).w_full().min_w(0.0).h(156.0).padding(20.0).gap(12.0)
            .radius(8.0).border(1.0, c("#E0E0E0")).fill(crate::avatar::rgb(app_backend::schemes::active_palette().surface_raised))
            .child(crate::avatar::avatar(&team.name, 40.0, None))
            .child(text(team.name.clone()).font(ts(16.0, 600.0)).color(c("#242424")).wrap())
            .child(text(format!("{} channels", team.channels.len())).font(ts(12.0, 400.0)).color(c("#616161")));
        if let Some(channel) = channel {
            tile = tile.hover(c("#EBEBEB")).on_press_tx(move |s: &mut UiState<A>, tx| {
                if !s.expanded_teams.contains(&team_id) { s.expanded_teams.push(team_id.clone()); }
                crate::ui::open_chat(s, &channel, tx);
            });
        }
        tiles = tiles.child(tile);
    }
    col().key("teams-overview").grow(1.0).min_w(0.0).h_full().min_h(0.0).padding(24.0).gap(20.0).fill(c("#FFFFFF"))
        .child(text("Your teams").font(ts(20.0, 600.0)).color(c("#242424")))
        .children(s.teams.is_empty().then(|| text("Your teams will appear here when connected.").font(ts(14.0, 400.0)).color(c("#616161"))))
        .child(col().grow(1.0).min_h(0.0).w_full().scroll(ScrollSpec { stick_bottom: false }).child(tiles))
}

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
            sender_id: None,
            system: false,
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
    fn feed_has_one_card_per_post_even_with_unloaded_roots() {
        let rows = vec![row("old1", Some("gone")), row("old2", Some("gone")), row("a", None), row("a1", Some("a")), row("a2", Some("a"))];
        assert_eq!(post_indices(&rows), [0, 2]);
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
