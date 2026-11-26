//! The composer under the open conversation, drawn as the GPUI app draws it
//! (`crates/app/src/views/composer.rs`): one rounded field with the format, emoji, attach, AI and
//! add icons and the send icon inline on its right. Attachments, a thread banner and send errors
//! stack above it.

use std::time::{Duration, Instant};

use app_backend::schemes::active_palette;
use state::delivery_options::{
    DELIVERY_BUTTON_TITLE, DeliveryContext, DeliveryOption, URGENT_TOO_MANY_MEMBERS, UrgentAvailability,
    allowed_importance, delivery_options, urgent_availability as urgent_rule, urgent_banner,
};
use state::importance::importance_label;
use rune::*;
use crate::ui::text;

use crate::avatar::rgb;
use crate::controls::{self, Size, Variant};
use crate::icons::sym;
use crate::media;
use crate::types::{Actions, Person, SendTarget};
use crate::ui::{INK, MUTED, UiState, active_target, c, select_thread, send, ts};

const BUTTON: f32 = 32.0;
const INSET: f32 = 6.0;
// Same interval as app-state::typing::OutgoingTyping.
const TYPING_INTERVAL: Duration = Duration::from_secs(20);

/// Composer state beyond the text: mentions, pickers and the GIFs waiting to be sent.
#[derive(Default)]
pub struct Compose {
    pub inline: Vec<(String, state::outgoing::InlineImage)>,
    pub inline_loading: std::collections::HashSet<String>,
    pub slash: crate::slash::State,
    pub importance: store::MessageImportance,
    pub subject: TextEdit,
    pub headline: TextEdit,
    pub reply: Option<crate::types::ReplyReference>,
    typing_sent_at: Option<Instant>,
    pub mentions: crate::mentions::State,
    pub emoji: crate::emoji_picker::State,
    pub gif: crate::gif_picker::State,
    pub gifs: Vec<chatsvc::Gif>,
    pub cloud: crate::cloud_picker::State,
    /// The "+" menu: schedule the draft, set its delivery options or schedule a meeting.
    pub plus_menu: bool,
    /// The delivery options, opened from the "+" menu.
    pub delivery_menu: bool,
    /// Conversation and text last handed to `Actions::save_draft`.
    saved: Option<(String, String)>,
    save_retry: Option<Instant>,
}

fn typing_changed<A: Actions>(s: &mut UiState<A>, now: Instant) {
    if s.read_only { return; }
    let Some(target) = active_target(s) else { return };
    if s.composer.text.trim().is_empty() {
        stop_typing(s);
    } else if s.compose.typing_sent_at.is_none_or(|sent| now.duration_since(sent) >= TYPING_INTERVAL) {
        s.compose.typing_sent_at = Some(now);
        s.actions.send_typing(target, true);
    }
}

pub(crate) fn stop_typing<A: Actions>(s: &mut UiState<A>) {
    if s.compose.typing_sent_at.take().is_some() && let Some(target) = active_target(s) {
        s.actions.send_typing(target, false);
    }
}

impl Compose {
    pub(crate) fn draft_save_failed(&self) -> bool { self.save_retry.is_some() }
    pub fn snapshot(&self, text: String) -> crate::types::Composed {
        crate::types::Composed {
            mentions: crate::mentions::resolve(&text, &self.mentions.chosen), text,
            gifs: self.gifs.clone(), reply: self.reply.clone(), inline: self.inline.clone(), importance: self.importance,
            subject: (!self.subject.text.is_empty()).then(|| self.subject.text.clone()),
            banner: (!self.headline.text.is_empty()).then(|| teams_core::AnnouncementBanner { title: self.headline.text.clone(), background: teams_core::BannerBackground::Color("indigo".into()) }),
        }
    }
    /// What is sent with `text`; the mentions and GIFs are used up.
    pub fn take(&mut self, text: String) -> crate::types::Composed {
        let mentions = crate::mentions::resolve(&text, &self.mentions.chosen);
        self.mentions.chosen.clear();
        self.gif.open = false;
        let subject = (!self.subject.text.trim().is_empty()).then(|| self.subject.text.trim().to_owned());
        let banner = (!self.headline.text.trim().is_empty()).then(|| teams_core::AnnouncementBanner {
            title: self.headline.text.trim().to_owned(), background: teams_core::BannerBackground::Color("indigo".into()),
        });
        self.subject.clear();
        self.headline.clear();
        crate::types::Composed { text, mentions, gifs: std::mem::take(&mut self.gifs), reply: self.reply.take(),
            importance: std::mem::take(&mut self.importance), subject, banner, inline: std::mem::take(&mut self.inline) }
    }
}

/// Stores the open conversation's unsent text when it changed. Called on every poll.
pub(crate) fn persist_draft<A: Actions>(s: &mut UiState<A>) {
    if s.compose.save_retry.is_some_and(|at| Instant::now() < at) { return; }
    let Some(id) = s.selected.clone().filter(|_| s.thread_root_id.is_none()) else { return };
    let composed = s.compose.snapshot(s.composer.text.clone());
    let target = active_target(s).expect("selected conversation");
    let files = s.media.files(&target);
    let inline: Vec<_> = composed.inline.iter().map(|(marker, image)| (marker, image.image.id)).collect();
    let now = (id, format!("{:?}", (&composed.text, &composed.mentions, &composed.gifs, &composed.reply, &composed.importance, &composed.subject, &composed.banner, inline, &files)));
    if s.compose.saved.as_ref() != Some(&now) {
        match s.actions.save_composition(&now.0, &composed, &files) {
            Ok(()) => { s.compose.saved = Some(now); s.compose.save_retry = None; }
            Err(error) => { s.toast.show(format!("Draft could not be saved: {error}")); s.compose.save_retry = Some(Instant::now() + Duration::from_secs(10)); }
        }
    }
}

/// Ctrl+B, I, U, then the mention list's keys.
fn composer_key<A: Actions>(s: &mut UiState<A>, key: &KeyEvent, tx: &mut Tx) -> bool {
    let empty = s.composer.text.is_empty() && s.compose.reply.is_none() && s.compose.gifs.is_empty()
        && active_target(s).is_some_and(|target| s.media.files(&target).is_empty() && !s.media.is_uploading(&target));
    if key.key == Key::Up && key.mods == Mods::default() && empty && !s.read_only {
        match s.settings.prefs.chat.settings().up_arrow {
            teams_core::UpArrowBehavior::StayInCompose => return true,
            teams_core::UpArrowBehavior::NavigateToMessages => {
                if let Some(message) = s.messages.last() {
                    tx.scroll_center_in_instant("messages", format!("msg-{}", message.id));
                    tx.focus(format!("msg-{}", message.id), true);
                }
                return true;
            }
            teams_core::UpArrowBehavior::EditLastMessage => {}
        }
        let channel = active_target(s).is_some_and(|target| target.channel);
        if let Some(message) = s.messages.iter().rev().find(|m| m.mine && !m.deleted && !m.system && (!channel || m.reply_to_id == s.thread_root_id)).cloned() {
            crate::message_menu::begin_edit(s, &message, tx);
            return true;
        }
    }
    for (letter, before, after) in [('b', "**", "**"), ('i', "*", "*"), ('u', "<u>", "</u>")] {
        if key.is_command(letter) {
            format_selection(&mut s.composer, before, after);
            return true;
        }
    }
    crate::slash::handle_key(s, key, tx) || crate::emoji_picker::handle_key(s, key) || crate::mentions::handle_key(s, key) || enter_key(s, key)
}

/// Teams' "press Enter to start a new line": Enter breaks the line and Ctrl+Enter sends.
fn enter_key<A: Actions>(s: &mut UiState<A>, key: &KeyEvent) -> bool {
    if key.key != Key::Enter || s.settings.prefs.chat.settings().enter_key != teams_core::EnterKeyBehavior::NewLine { return false; }
    match key.mods {
        Mods { cmd: true, shift: false, alt: false, .. } => send(s),
        mods if mods == Mods::default() => s.composer.insert("
"),
        _ => return false,
    }
    true
}

fn format_selection(edit: &mut TextEdit, before: &str, after: &str) {
    let (start, end) = edit.range();
    let selected = if start == end { "text" } else { &edit.text[start..end] };
    let insertion = format!("{before}{selected}{after}");
    edit.insert(&insertion);
    edit.set_selection(start + before.len(), start + insertion.len() - after.len());
}

/// Emoji and GIFs share one panel; either picker flag keeps it open.
fn fun_open<A: Actions>(s: &UiState<A>) -> bool {
    s.emoji_open || s.compose.gif.open
}

/// The emoji/GIF panel: a tab row over whichever picker is selected.
fn fun_panel<A: Actions + 'static>(s: &UiState<A>) -> El {
    let gif = s.compose.gif.open;
    let tabs = row().key("fun-tabs").gap(6.0)
        .child(controls::button::<A>("fun-tab-emoji", "Emoji", if gif { Variant::Menu } else { Variant::Secondary }, Size::Sm, |s: &mut UiState<A>, tx: &mut Tx| {
            s.compose.gif.open = false;
            s.emoji_open = true;
            tx.focus("emoji-search", false);
        }))
        .child(controls::button::<A>("fun-tab-gif", "GIF", if gif { Variant::Secondary } else { Variant::Menu }, Size::Sm, |s: &mut UiState<A>, tx: &mut Tx| {
            if !s.compose.gif.open { crate::gif_picker::toggle(s); }
            tx.focus("gif-search", false);
        }));
    col().key("fun-panel").w_full().gap(6.0)
        .child(tabs)
        .child(if gif { crate::gif_picker::picker(s) } else { crate::emoji_picker::picker(s) })
}

/// Teams' Urgent rule for the conversation, from the shared `app-state` rule.
pub(crate) fn urgent_availability<A: Actions>(s: &UiState<A>, target: Option<&SendTarget>) -> UrgentAvailability {
    let Some(target) = target else { return UrgentAvailability::Hidden };
    let id = target.conversation_id.as_str();
    let details = s.details.get(id);
    urgent_rule(&DeliveryContext {
        conversation_id: id,
        chat_kind: details.map_or("", |details| details.kind.as_str()),
        member_count: details.map_or(0, |details| details.members.len()),
        channel: target.channel,
        one_on_one_bot: details.is_some_and(|details| details.kind == "oneOnOne" && details.members.iter().any(store::MemberRecord::is_bot)),
        editing: s.menu.edit_id.is_some(),
        policy_allows_urgent: s.allow_priority_messages,
    })
}

/// The importance that is sent: Standard while editing, and Urgent falls back to Standard where
/// Teams no longer offers it.
pub(crate) fn sendable_importance<A: Actions>(s: &UiState<A>, target: Option<&SendTarget>) -> store::MessageImportance {
    if s.menu.edit_id.is_some() { return store::MessageImportance::Normal; }
    allowed_importance(s.compose.importance, urgent_availability(s, target))
}

/// The other people of the open chat, to invite to a meeting made from it (no bots, not me).
fn invitees<A: Actions>(s: &UiState<A>) -> Vec<Person> {
    let me = s.own.me.as_ref().map(|me| me.user_id.as_str());
    let Some(target) = active_target(s) else { return Vec::new() };
    let Some(details) = s.details.get(&target.conversation_id) else { return Vec::new() };
    details.members.iter().filter(|member| !member.is_bot()).filter_map(|member| {
        let user_id = member.user_id.clone()?;
        (Some(user_id.as_str()) != me).then(|| Person { user_id, name: member.display_name.clone(), mail: None })
    }).collect()
}

/// A menu anchored above the "+" button, like GPUI's popovers.
fn anchored_menu<A: Actions + 'static>(key: &'static str, close: fn(&mut UiState<A>)) -> El {
    let palette = active_palette();
    col().key(key).w(260.0).padding(6.0).gap(4.0).radius(10.0)
        .fill(rgb(palette.popup)).border(1.0, rgb(palette.border))
        .anchored("composer-add", Side::Top, Align::End, 4.0)
        .modal(true, close)
}

/// The "+" menu, GPUI's "Actions and apps".
fn plus_menu<A: Actions + 'static>() -> El {
    anchored_menu::<A>("composer-plus-menu", |s| s.compose.plus_menu = false)
        .child(controls::menu_item::<A>("composer-plus/schedule-message", "Schedule message", Some("schedule"), false, |s: &mut UiState<A>| {
            s.compose.plus_menu = false;
            crate::schedule::open(s);
        }))
        .child(controls::menu_item::<A>("composer-plus/delivery-options", DELIVERY_BUTTON_TITLE, Some("priority_high"), false, |s: &mut UiState<A>| {
            s.compose.plus_menu = false;
            s.compose.delivery_menu = true;
        }))
        .child(controls::menu_item::<A>("composer-plus/schedule-meeting", "Schedule meeting", Some("calendar_month"), false, |s: &mut UiState<A>| {
            s.compose.plus_menu = false;
            crate::meeting_form::open(s);
            s.meeting.people.chips = invitees(s);
        }))
}

/// The delivery options: Urgent is listed only where Teams offers it, and disabled above 20 people.
fn delivery_menu<A: Actions + 'static>(s: &UiState<A>) -> El {
    let target = active_target(s);
    let selected = sendable_importance(s, target.as_ref());
    let urgent = urgent_availability(s, target.as_ref());
    let mut menu = anchored_menu::<A>("delivery-menu", |s| s.compose.delivery_menu = false);
    for (option, enabled) in delivery_options(urgent) {
        menu = menu.child(delivery_row::<A>(option, enabled, option.importance == selected));
    }
    menu
}

/// Rune's icon set lacks GPUI's `notification_important`; `notifications` is the nearest.
fn glyph(symbol: &'static str) -> &'static str {
    if symbol == "notification_important" { "notifications" } else { symbol }
}

fn delivery_row<A: Actions + 'static>(option: DeliveryOption, enabled: bool, selected: bool) -> El {
    let palette = active_palette();
    let accent = if option.importance.is_marked() { rgb(palette.red) } else { rgb(palette.foreground) };
    let mut row_el = row().key(option.tag).w_full().min_h(32.0).px(10.0).py(8.0).gap(10.0).center_y().radius(6.0)
        .role("menuitem")
        .label(option.header)
        .focus_selected(selected)
        .child(sym(glyph(option.symbol), 20.0).color(accent))
        .child(col().grow(1.0).min_w(0.0).gap(2.0)
            .child(text(option.header).font(ts(13.0, 600.0)).color(rgb(palette.foreground)))
            .child(text(option.content).font(ts(12.0, 400.0)).color(rgb(palette.text_muted)).wrap()))
        .children(selected.then(|| sym("check", 16.0).color(rgb(palette.foreground))));
    if enabled {
        let importance = option.importance;
        row_el = row_el.focusable(true).hover(rgb(palette.popup_hover)).on_press_tx(move |s: &mut UiState<A>, tx: &mut Tx| {
            s.compose.importance = importance;
            s.compose.delivery_menu = false;
            tx.focus("composer", false);
        });
    } else {
        row_el = row_el.opacity(0.4).disabled(true).help(URGENT_TOO_MANY_MEMBERS.to_owned());
    }
    row_el
}

/// Urgent's notice above the field: who is notified, and a way back to Standard.
fn urgent_notice<A: Actions + 'static>(member_count: usize) -> El {
    let palette = active_palette();
    row().key("composer-urgent-banner").w_full().h(36.0).px(12.0).gap(8.0).center_y().radius(10.0)
        .fill(rgb(palette.red).alpha(0.14))
        .child(sym("notifications", 16.0).color(rgb(palette.red)))
        .child(text(urgent_banner(member_count)).font(ts(12.0, 600.0)).color(rgb(palette.red)).truncate().grow(1.0).min_w(0.0))
        .child(controls::icon_button_with_ink::<A>("composer-urgent-banner-close", "close", "Send as standard", false, rgb(palette.red), |s: &mut UiState<A>, _| {
            s.compose.importance = store::MessageImportance::Normal;
        }))
}

pub(crate) fn composer<A: Actions + 'static>(s: &UiState<A>, stacked: bool) -> El {
    let palette = active_palette();
    let target = active_target(s);
    let has_text = !s.composer.text.trim().is_empty();
    // A GIF or an uploaded file is a message too: `ui::send` accepts those without typed text.
    let can_send = has_text || !s.compose.gifs.is_empty() || target.as_ref().is_some_and(|target| {
        !s.media.files(target).is_empty() && !s.media.is_uploading(target)
    });
    let importance = sendable_importance(s, target.as_ref());

    let field = text_input(&s.composer, ts(14.0, 400.0))
        .key("composer")
        .label("Message")
        .placeholder("Type a message", c(MUTED))
        .color(c(INK))
        .w_full()
        .on_edit(|s: &mut UiState<A>| &mut s.composer)
        .on_input_paste(|s: &mut UiState<A>, content, _| crate::inline_compose::paste(s, content))
        .on_input_change(|s: &mut UiState<A>, _| typing_changed(s, Instant::now()))
        .on_input_key(|s: &mut UiState<A>, key: &KeyEvent, tx: &mut Tx| composer_key(s, key, tx))
        .on_submit(|s: &mut UiState<A>, _tx: &mut Tx| send(s));
    let field = row()
        .key("composer-field")
        .grow(1.0)
        .min_w(0.0)
        .min_h(BUTTON)
        .center_y()
        .child(auto_grow(field, 1, 8).grow(1.0).min_w(0.0));

    let toolbar = row()
        .key("composer-tools")
        .gap(INSET)
        .center_y()
        .children((!s.settings.prefs.markdown).then(|| {
            controls::icon_button::<A>("composer-format", "text_format", "Formatting", s.format_open, |s: &mut UiState<A>, _| {
                s.format_open = !s.format_open;
            })
        }))
        .child(controls::icon_button::<A>("composer-emoji", "mood", "Emoji, GIFs and stickers", fun_open(s), |s: &mut UiState<A>, tx: &mut Tx| {
            let open = fun_open(s);
            s.emoji_open = !open;
            s.compose.gif.open = false;
            if s.emoji_open { tx.focus("emoji-search", false); }
        }).help("Emoji, GIFs and stickers".to_owned()))
        .children(target.as_ref().map(|_| {
            controls::icon_button::<A>("attach-file", "attach_file", "Attach file", s.compose.cloud.menu, |s: &mut UiState<A>, _| {
                s.compose.cloud.menu = !s.compose.cloud.menu;
            })
        }))
        .child(controls::icon_button::<A>("composer-ai", "auto_fix_high", "Ask AI", false, |s: &mut UiState<A>, _| {
            s.agent.prompt = TextEdit::new(&s.composer.text);
            s.screens.screen = crate::screens::Screen::Agents;
        }))
        .children(target.as_ref().filter(|_| s.menu.edit_id.is_none()).map(|_| {
            let ink = if importance.is_marked() { rgb(palette.red) } else { rgb(palette.foreground) };
            controls::icon_button_with_ink::<A>("composer-add", "add", "Actions and apps", s.compose.plus_menu, ink, |s: &mut UiState<A>, _| {
                s.compose.plus_menu = !s.compose.plus_menu;
                s.compose.delivery_menu = false;
            })
        }));

    // Keep the send button in the tab order even while it is quiet: screen readers can still find
    // it, while the dimmed, disabled look says why it cannot be pressed.
    let mut send_button = controls::icon_button_with_ink::<A>(
        "send",
        "send",
        "Send message",
        false,
        if can_send { rgb(palette.on_accent) } else { c(MUTED) },
        |s: &mut UiState<A>, tx: &mut Tx| {
            send(s);
            tx.focus("composer", false);
        },
    )
    .fill(if can_send { rgb(palette.accent) } else { Color::TRANSPARENT })
    .hover(if can_send { rgb(palette.accent_strong) } else { rgb(palette.row_hover) });
    if !can_send {
        send_button = send_button.opacity(0.4).disabled(true).focusable(true).on_press(|_: &mut UiState<A>| {});
    }
    // Right-click on send schedules the draft, as in GPUI; the scheduler says why it cannot.
    let send_button = send_button.on_secondary_press(|s: &mut UiState<A>, _, _| {
        if !s.schedule.open { crate::schedule::open(s); }
    });
    let divider = el().w(1.0).h(20.0).ml(INSET).fill(rgb(palette.border));
    let actions = row().key("composer-actions").gap(INSET).center_y()
        .child(toolbar).child(divider).child(el().grow(1.0))
        .child(send_button);
    let contents = if stacked {
        col().w_full().gap(INSET).child(field).child(actions.w_full())
    } else {
        row().w_full().gap(INSET).center_y().child(field)
            .child(actions)
    };
    let bar = col()
        .key("composer-bar")
        .w_full()
        .px(INSET)
        .py(INSET)
        .gap(INSET)
        .center_y()
        .radius(12.0)
        .border(1.0, Color::TRANSPARENT)
        .fill(rgb(palette.surface_raised))
        .child(contents);
    let bar = bar.on_drop_files(|s: &mut UiState<A>, paths, _| crate::inline_compose::drop_files(s, paths));

    col()
        .w_full()
        .px(20.0)
        .pb(16.0)
        .gap(6.0)
        .children(target.as_ref().filter(|target| target.channel && target.thread_root_id.is_none()).map(|_| {
            col().w_full().gap(4.0)
                .child(text_input(&s.compose.subject, ts(14.0, 600.0)).key("post-subject").label("Post subject").placeholder("Add a subject", c(MUTED)).w_full()
                    .on_edit(|s: &mut UiState<A>| &mut s.compose.subject))
                .child(text_input(&s.compose.headline, ts(18.0, 600.0)).key("post-headline").label("Announcement headline").placeholder("Announcement headline (optional)", c(MUTED)).w_full()
                    .on_edit(|s: &mut UiState<A>| &mut s.compose.headline))
        }))
        .children((s.format_open && !s.settings.prefs.markdown).then(|| {
            let mut tools = row().key("format-tools").wrap().gap(6.0).max_h(150.0).scroll(ScrollSpec { stick_bottom: false });
            for (label, before, after) in [("Bold", "**", "**"), ("Italic", "*", "*"), ("Underline", "<u>", "</u>"), ("Strike", "~~", "~~"), ("Code", "`", "`"), ("List", "\n- ", ""),
                ("Numbered list", "\n1. ", ""), ("Heading", "\n# ", ""), ("Quote", "\n> ", ""), ("Link", "[", "](https://example.com)"),
                ("Code block", "\n```rust\n", "\n```\n"), ("Table", "\n| Column 1 | Column 2 |\n| --- | --- |\n| ", " | Value |\n")] {
                tools = tools.child(controls::button::<A>(&format!("format-{label}"), label, Variant::Menu, Size::Sm, move |s: &mut UiState<A>, tx| {
                    format_selection(&mut s.composer, before, after); tx.focus("composer", false);
                }));
            }
            for (label, before, after) in [("Superscript", "<sup>", "</sup>"), ("Subscript", "<sub>", "</sub>"), ("Small text", "<span style=\"font-size:xx-small\">", "</span>"), ("Large text", "<span style=\"font-size:x-large\">", "</span>"), ("Indent", "<div style=\"margin-left:40px\">", "</div>"), ("Horizontal rule", "\n<hr>", "\n")] {
                tools = tools.child(controls::button::<A>(&format!("format-{label}"), label, Variant::Menu, Size::Sm, move |s: &mut UiState<A>, tx| { format_selection(&mut s.composer, before, after); tx.focus("composer", false); }));
            }
            for (name, color) in crate::formatting::COLORS {
                for (label, property) in [("Text", "color"), ("Highlight", "background-color")] {
                    let tag = format!("<span style=\"{property}:{color}\">");
                    tools = tools.child(controls::button::<A>(&format!("format-{label}-{name}"), &format!("{label} {name}"), Variant::Menu, Size::Sm, move |s: &mut UiState<A>, tx| { format_selection(&mut s.composer, &tag, "</span>"); tx.focus("composer", false); }));
                }
            }
            tools = tools.child(controls::button::<A>("format-outdent", "Outdent", Variant::Menu, Size::Sm, |s: &mut UiState<A>, tx| { crate::formatting::outdent(&mut s.composer); tx.focus("composer", false); }))
                .child(controls::button::<A>("format-clear", "Clear formatting", Variant::Menu, Size::Sm, |s: &mut UiState<A>, tx| { crate::formatting::clear(&mut s.composer); tx.focus("composer", false); }));
            for action in teams_core::TableEdit::INSERTS.into_iter().chain(teams_core::TableEdit::DELETES) {
                tools = tools.child(controls::button::<A>(&format!("format-table-{action:?}"), action.label(), Variant::Menu, Size::Sm, move |s: &mut UiState<A>, tx| {
                    if !crate::formatting::table(&mut s.composer, action) { s.toast.show("Place the cursor inside a table first"); }
                    tx.focus("composer", false);
                }));
            }
            tools
        }))
        .children(fun_open(s).then(|| fun_panel(s)))
        .children(target.as_ref().filter(|_| s.compose.cloud.menu).map(|target| crate::cloud_picker::menu::<A>(target)))
        .children((target.is_some() && s.compose.cloud.open == target).then(|| crate::cloud_picker::picker(s)))
        .children(crate::gif_picker::chips(s))
        .child(crate::inline_compose::previews(s))
        .children(target.clone().map(|target| media::attachments(s, &target)))
        .children(s.compose.reply.as_ref().map(|reply| row().key("reply-reference").w_full().gap(8.0).px(8.0).py(6.0).center_y()
            .child(text(format!("Replying to {}: {}", reply.sender, reply.text)).font(ts(12.0, 600.0)).color(c(INK)).truncate().grow(1.0).min_w(0.0))
            .child(crate::screens::button("Cancel", "reply-cancel", |s: &mut UiState<A>, _| s.compose.reply = None))))
        .children(s.thread_root_id.as_ref().map(|_| row().key("close-reply").h(28.0).px(8.0).hover(c(crate::ui::HOVER))
            .on_press_tx(|s: &mut UiState<A>, _| select_thread(s, None))
            .child(text("Replying in thread · Close").font(ts(12.0, 600.0)).color(c(INK)))))
        .children(s.send_error.as_ref().map(|e| {
            text(e.clone())
                .key("send-error")
                .font(ts(12.0, 400.0))
                .color(c("#C4314B"))
                .wrap()
        }))
        .children(crate::mentions::list(s))
        .children(crate::emoji_picker::popup(s))
        .children(crate::schedule::view(s))
        .children(crate::slash::view(s))
        .children(importance_label(importance).map(|label| {
            text(label).key("composer-importance-label").font(ts(12.0, 700.0)).color(rgb(palette.red))
        }))
        .children((importance == store::MessageImportance::Urgent).then(|| {
            let member_count = target.as_ref().and_then(|target| s.details.get(&target.conversation_id)).map_or(0, |details| details.members.len());
            urgent_notice::<A>(member_count)
        }))
        .child(bar)
        // Last, so they paint over the bar they anchor to.
        .children(s.compose.plus_menu.then(|| plus_menu::<A>()))
        .children(s.compose.delivery_menu.then(|| delivery_menu(s)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChatRow, SendTarget, Update};
    use rune::{Cx, ElementInfo, Env, Runtime};

    /// Records what the composer asks of the bridge.
    #[derive(Default)]
    struct Quiet {
        calls: std::cell::RefCell<Vec<String>>,
        stored_draft: Option<String>,
    }

    impl Actions for Quiet {
        fn open(&self, _conversation_id: &str) {}
        fn send(&self, _target: SendTarget, _text: String, _files: Vec<teams_core::FileReference>) {}
        fn send_typing(&self, target: SendTarget, active: bool) {
            self.calls.borrow_mut().push(format!("typing {} {:?} {active}", target.conversation_id, target.thread_root_id));
        }
        fn send_composed(&self, _target: SendTarget, composed: crate::types::Composed, _files: Vec<teams_core::FileReference>) {
            let ids: Vec<String> = composed.mentions.iter().map(|m| format!("{:?}", m.target)).collect();
            self.calls.borrow_mut().push(format!("send [{}] mentions {ids:?} gifs {}", composed.text, composed.gifs.len()));
            self.calls.borrow_mut().push(format!("importance {:?}", composed.importance));
        }
        fn search_gifs(&self, query: &str) { self.calls.borrow_mut().push(format!("gifs [{query}]")); }
        fn save_draft(&self, id: &str, text: &str) { self.calls.borrow_mut().push(format!("draft {id} [{text}]")); }
        fn load_draft(&self, _id: &str) -> Option<String> { self.stored_draft.clone() }
        fn list_cloud_files(&self, place: crate::cloud_picker::Place) { self.calls.borrow_mut().push(format!("cloud {place:?}")); }
        fn attach_cloud_file(&self, target: SendTarget, file: teams_core::CloudFile) {
            self.calls.borrow_mut().push(format!("attach {} to {} by id {:?}", file.name, target.conversation_id, file.item_id));
        }
    }

    fn drive_entry(name: &str, folder: bool) -> teams_core::DriveEntry {
        teams_core::DriveEntry {
            drive_id: "d".into(), id: name.into(), name: name.into(), web_url: format!("https://x/{name}"), size: 1,
            modified_at: None, modified_by: Some("Ada".into()), child_count: folder.then_some(2), download_url: None,
        }
    }

    fn open_cloud_picker(rt: &mut Runtime<UiState<Quiet>>) {
        click(rt, "attach-file");
        click(rt, "attach-cloud");
        assert!(rt.find("cloud-picker").is_some() && rt.find("attach-menu").is_none());
        assert!(calls(rt).contains(&"cloud Recent".to_owned()), "{:?}", calls(rt));
        assert!(rt.find("cloud-attach").is_some() && rt.find("cloud-tab-shared").is_some());
    }

    #[test]
    fn composer_throttles_typing_and_stops_on_send_empty_or_switch() {
        let mut rt = rt();
        click(&mut rt, "composer");
        press(&mut rt, Key::Text("hi".into()));
        press(&mut rt, Key::Text(" there".into()));
        assert_eq!(calls(&rt).iter().filter(|c| c.starts_with("typing")).count(), 1);
        let sent = rt.state.compose.typing_sent_at.unwrap();
        rt.update(move |s, _| typing_changed(s, sent + TYPING_INTERVAL));
        assert_eq!(calls(&rt).iter().filter(|c| c.ends_with("true")).count(), 2);
        rt.update(|s, _| { s.composer.clear(); typing_changed(s, Instant::now()); });
        assert_eq!(calls(&rt).last().unwrap(), "typing b None false");
        press(&mut rt, Key::Text("again".into()));
        rt.key(KeyEvent { key: Key::Enter, mods: Mods::default() });
        assert!(calls(&rt).iter().rev().take(2).any(|c| c == "typing b None false"));
        press(&mut rt, Key::Text("draft".into()));
        rt.update(|s, tx| crate::ui::open_chat(s, "a", tx));
        assert_eq!(calls(&rt).last().unwrap(), "typing b None false");
        rt.update(|s, _| { s.read_only = true; s.composer = TextEdit::new("hello"); typing_changed(s, Instant::now()); });
        assert!(rt.state.compose.typing_sent_at.is_none());
    }

    #[test]
    fn thread_typing_is_cleared_before_leaving_the_thread_or_screen() {
        let mut rt = rt();
        rt.update(|s, _| crate::ui::select_thread(s, Some("root".into())));
        click(&mut rt, "composer");
        press(&mut rt, Key::Text("reply".into()));
        assert_eq!(calls(&rt).last().unwrap(), "typing b Some(\"root\") true");
        rt.update(|s, _| crate::ui::select_thread(s, None));
        assert_eq!(calls(&rt).last().unwrap(), "typing b Some(\"root\") false");
        click(&mut rt, "composer");
        press(&mut rt, Key::Text("chat".into()));
        rt.update(|s, _| crate::screens::navigate(s, crate::screens::Screen::Agents));
        assert_eq!(calls(&rt).last().unwrap(), "typing b None false");
    }

    #[test]
    fn picked_cloud_files_join_the_draft_attachments() {
        let mut rt = rt();
        open_cloud_picker(&mut rt);
        rt.update(|s, _| s.apply(Update::CloudFiles { place: crate::cloud_picker::Place::MyFiles, result: Ok(vec![drive_entry("stale.txt", false)]) }));
        rt.update(|s, _| s.apply(Update::CloudFiles { place: crate::cloud_picker::Place::Recent,
            result: Ok(vec![drive_entry("Plans", true), drive_entry("a.docx", false)]) }));
        rt.frame(1.0 / 60.0);
        click(&mut rt, "cloud-row-0");
        assert!(calls(&rt).iter().any(|call| call.starts_with("cloud Folder")), "a folder opens");
        assert!(rt.find("cloud-crumb-0").is_some());
        rt.update(|s, _| {
            let place = crate::cloud_picker::Place::Folder(drive_entry("Plans", true).folder());
            s.apply(Update::CloudFiles { place, result: Ok(vec![drive_entry("a.docx", false), drive_entry("b.xlsx", false)]) });
        });
        rt.frame(1.0 / 60.0);
        click(&mut rt, "cloud-row-0");
        click(&mut rt, "cloud-row-1");
        click(&mut rt, "cloud-attach");
        assert!(rt.find("cloud-picker").is_none());
        let attached: Vec<String> = calls(&rt).into_iter().filter(|call| call.starts_with("attach")).collect();
        assert_eq!(attached, ["attach a.docx to b by id Some(\"a.docx\")", "attach b.xlsx to b by id Some(\"b.xlsx\")"]);
        assert!(rt.find("attachment-0").is_some() && rt.find("attachment-1").is_some(), "pending files show at once");
        let target = SendTarget { conversation_id: "b".into(), thread_root_id: None, channel: false };
        let reference = teams_core::FileReference { attachment_id: "e1".into(), content_url: "https://x/a.docx".into(), name: "a.docx".into() };
        rt.update(|s, _| s.apply(Update::CloudAttached { target: target.clone(), name: "a.docx".into(), result: Ok(reference.clone()) }));
        assert_eq!(rt.state.media.files(&target), vec![reference]);
        assert!(rt.state.media.is_uploading(&target), "b.xlsx is still on its way");
    }

    #[test]
    fn cancel_leaves_the_draft_unchanged() {
        let mut rt = rt();
        open_cloud_picker(&mut rt);
        rt.update(|s, _| s.apply(Update::CloudFiles { place: crate::cloud_picker::Place::Recent, result: Ok(vec![drive_entry("a.docx", false)]) }));
        rt.frame(1.0 / 60.0);
        click(&mut rt, "cloud-row-0");
        click(&mut rt, "cloud-cancel");
        assert!(rt.find("cloud-picker").is_none() && rt.find("attachment-0").is_none());
        assert!(calls(&rt).iter().all(|call| !call.starts_with("attach")));
    }

    #[test]
    fn a_refused_listing_shows_the_error_line_and_an_empty_one_says_so() {
        let mut rt = rt();
        open_cloud_picker(&mut rt);
        click(&mut rt, "cloud-tab-shared");
        assert!(calls(&rt).contains(&"cloud Shared".to_owned()));
        rt.update(|s, _| s.apply(Update::CloudFiles { place: crate::cloud_picker::Place::Shared, result: Err("403 Forbidden: accessDenied".into()) }));
        rt.frame(1.0 / 60.0);
        assert!(rt.find("cloud-error").is_some());
        click(&mut rt, "cloud-tab-mine");
        rt.update(|s, _| s.apply(Update::CloudFiles { place: crate::cloud_picker::Place::MyFiles, result: Ok(vec![]) }));
        rt.frame(1.0 / 60.0);
        assert!(rt.find("cloud-error").is_none() && rt.find("cloud-empty").is_some());
    }

    #[test]
    fn up_arrow_respects_the_preference_and_preserves_a_nonempty_draft() {
        let mut rt = rt();
        rt.update(|s, _| s.messages.push(crate::types::MessageRow {
            id: "mine".into(), sender: "Me".into(), sender_id: None,
            text: "Previous message".into(), when: "14:00".into(), mine: true,
            forwarded: false, importance: Default::default(), system: false,
            reply_to_id: None, deleted: false, can_delete: true,
            reactions: vec![], media: Default::default(),
        }));
        for behavior in [teams_core::UpArrowBehavior::StayInCompose, teams_core::UpArrowBehavior::NavigateToMessages] {
            rt.update(move |s, tx| {
                s.settings.prefs.chat.apply(teams_core::ChatSettingChange::UpArrow(behavior));
                assert!(composer_key(s, &KeyEvent { key: Key::Up, mods: Mods::default() }, tx));
                assert!(s.menu.edit_id.is_none());
            });
        }
        rt.update(|s, tx| {
            s.settings.prefs.chat.apply(teams_core::ChatSettingChange::UpArrow(teams_core::UpArrowBehavior::EditLastMessage));
            s.composer = TextEdit::new("Unsent draft");
            composer_key(s, &KeyEvent { key: Key::Up, mods: Mods::default() }, tx);
            assert!(s.menu.edit_id.is_none());
            assert_eq!(s.composer.text, "Unsent draft");
            s.composer.clear();
            assert!(composer_key(s, &KeyEvent { key: Key::Up, mods: Mods::default() }, tx));
            assert_eq!(s.menu.edit_id.as_deref(), Some("mine"));
        });
    }

    #[test]
    fn enter_breaks_the_line_and_ctrl_enter_sends_when_the_setting_says_so() {
        let mut rt = rt();
        rt.update(|s, _| { s.settings.prefs.chat.apply(teams_core::ChatSettingChange::EnterKey(teams_core::EnterKeyBehavior::NewLine)); });
        click(&mut rt, "composer");
        press(&mut rt, Key::Text("one".into()));
        press(&mut rt, Key::Enter);
        press(&mut rt, Key::Text("two".into()));
        assert_eq!(rt.state.composer.text, "one\ntwo");
        assert!(calls(&rt).iter().all(|c| !c.starts_with("send")));
        rt.key(KeyEvent { key: Key::Enter, mods: Mods { cmd: true, ctrl: true, ..Mods::default() } });
        rt.frame(1.0 / 60.0);
        assert!(calls(&rt).iter().any(|c| c.starts_with("send [one\ntwo]")), "{:?}", calls(&rt));
    }

    fn rt() -> Runtime<UiState<Quiet>> {
        rt_with(Quiet::default())
    }

    fn rt_with(actions: Quiet) -> Runtime<UiState<Quiet>> {
        let env = Env { system_dark: false, scale: 1.0 };
        let mut rt = Runtime::new(UiState::new(actions), crate::ui::view::<Quiet>, (1000.0, 700.0), env);
        rt.frame(0.0);
        rt.update(|s, _| {
            s.apply(Update::Chats(vec![ChatRow {
                id: "b".into(),
                title: "Bo".into(),
                preview: "hi".into(),
                unread: false,
                when: "14:05".into(),
            }]))
        });
        rt.frame(1.0 / 60.0);
        let (x, y) = rt.find("chat-b").expect("chat row");
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
        rt
    }

    /// Laid-out rect `[x, y, w, h]` of the built element at `key`.
    fn rect(rt: &Runtime<UiState<Quiet>>, key: &str) -> [f32; 4] {
        let suffix = format!("/{key}");
        rt.elements()
            .into_iter()
            .find(|e| e.path.ends_with(&suffix))
            .unwrap_or_else(|| panic!("no built element {key}"))
            .rect
    }

    #[test]
    fn formatting_wraps_selected_unicode_and_keeps_it_selected() {
        let mut edit = TextEdit::new("Hi été!");
        edit.set_selection(3, 8);
        format_selection(&mut edit, "**", "**");
        assert_eq!(edit.text, "Hi **été**!");
        let (start, end) = edit.range();
        assert_eq!(&edit.text[start..end], "été");
    }

    #[test]
    fn emoji_and_format_tools_insert_without_sending_or_losing_the_draft() {
        let mut rt = rt();
        let click = |rt: &mut Runtime<UiState<Quiet>>, key: &str| {
            let (x, y) = rt.find(key).expect("tool is visible");
            rt.pointer_down(x, y); rt.pointer_up(x, y); rt.frame(1.0);
        };
        click(&mut rt, "composer-format");
        click(&mut rt, "format-Bold");
        assert_eq!(rt.state.composer.text, "**text**");
        click(&mut rt, "composer-emoji");
        click(&mut rt, "emoji-tab-1");
        click(&mut rt, "emoji-👍");
        assert!(rt.state.composer.text.contains("👍"));
        let draft = rt.state.composer.text.clone();
        click(&mut rt, "composer-ai");
        assert_eq!(rt.state.agent.prompt.text, draft);
        assert_eq!(rt.state.composer.text, draft);
        assert!(!rt.state.agent.running);
    }

    #[test]
    fn attach_icon_and_send_sit_inline_right_of_the_field() {
        let mut rt = rt();
        rt.viewport = (1440.0, 900.0);
        rt.update(|_, _| {});
        rt.frame(0.1);
        rt.frame(0.2);
        let [field_x, _, field_w, _] = rect(&rt, "composer");
        let [attach_x, attach_y, attach_w, attach_h] = rect(&rt, "attach-file");
        let [send_x, send_y, send_w, send_h] = rect(&rt, "send");
        assert!(attach_x >= field_x + field_w, "attach icon at {attach_x} sits inside the field");
        assert!(send_x > attach_x, "send icon should follow the attach icon");
        assert_eq!([attach_w, attach_h], [BUTTON, BUTTON]);
        assert_eq!([send_w, send_h], [BUTTON, BUTTON]);
        assert!((attach_y - send_y).abs() < 0.5, "toolbar icons share one row");
    }

    fn click(rt: &mut Runtime<UiState<Quiet>>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
    }

    fn press(rt: &mut Runtime<UiState<Quiet>>, key: Key) {
        rt.key(KeyEvent { key, mods: Mods::default() });
        rt.frame(1.0 / 60.0);
    }

    fn calls(rt: &Runtime<UiState<Quiet>>) -> Vec<String> {
        rt.state.actions.calls.borrow().clone()
    }

    fn member(id: &str, name: &str) -> store::MemberRecord {
        store::MemberRecord { user_id: Some(id.into()), display_name: name.into() }
    }

    #[test]
    fn typing_an_at_lists_members_and_enter_picks_one_that_is_sent_as_a_mention() {
        let mut rt = rt();
        let details = crate::types::ChatDetails { members: vec![member("u1", "Ada Lovelace"), member("u2", "Bo Peep")], ..Default::default() };
        rt.update(|s, _| s.apply(Update::Details([("b".to_owned(), details)].into())));
        click(&mut rt, "composer");
        press(&mut rt, Key::Text("hey @b".into()));
        assert!(rt.find("mention-0").is_some(), "list opens");
        assert!(rt.find("mention-1").is_none(), "only Bo Peep matches");
        press(&mut rt, Key::Enter);
        assert_eq!(rt.state.composer.text, "hey @Bo Peep ");
        assert!(calls(&rt).iter().all(|c| !c.starts_with("send")), "Enter picked, it did not send");
        assert!(rt.find("mention-0").is_none());
        press(&mut rt, Key::Text("hi".into()));
        press(&mut rt, Key::Enter);
        let sent = calls(&rt).into_iter().find(|c| c.starts_with("send")).expect("sent");
        assert!(sent.contains("[hey @Bo Peep hi]") && sent.contains("u2"), "{sent}");
    }

    #[test]
    fn mention_suggestions_stay_inside_a_short_window() {
        let mut rt = rt();
        rt.set_viewport((900.0, 360.0), 1.0);
        let members = (0..8).map(|i| member(&format!("u{i}"), &format!("Ada {i}"))).collect();
        let details = crate::types::ChatDetails { members, ..Default::default() };
        rt.update(|s, _| s.apply(Update::Details([("b".to_owned(), details)].into())));
        click(&mut rt, "composer");
        press(&mut rt, Key::Text("@a".into()));
        let (list, _) = rt.presented("mention-list").expect("list opens");
        let (field, _) = rt.presented("composer").unwrap();
        assert!(list[1] >= 0.0 && list[1] + list[3] <= field[1], "list {list:?} inside the window above the field {field:?}");
        assert!(rt.find("mention-5").is_some(), "all six suggestions show");
    }

    #[test]
    fn arrows_move_the_mention_highlight_and_escape_closes_the_list() {
        let mut rt = rt();
        let details = crate::types::ChatDetails { members: vec![member("u1", "Ada"), member("u2", "Alan")], ..Default::default() };
        rt.update(|s, _| s.apply(Update::Details([("b".to_owned(), details)].into())));
        click(&mut rt, "composer");
        press(&mut rt, Key::Text("@a".into()));
        press(&mut rt, Key::Down);
        press(&mut rt, Key::Tab);
        assert_eq!(rt.state.composer.text, "@Alan ");
        press(&mut rt, Key::Text("@a".into()));
        press(&mut rt, Key::Escape);
        assert!(rt.find("mention-0").is_none());
    }

    #[test]
    fn the_emoji_picker_searches_and_inserts() {
        let mut rt = rt();
        click(&mut rt, "composer-emoji");
        assert!(rt.find("emoji-search").is_some() && rt.find("emoji-tab-8").is_some());
        rt.update(|s, _| s.compose.emoji.query = TextEdit::new("daumen"));
        rt.frame(1.0 / 60.0);
        click(&mut rt, "emoji-👍");
        assert_eq!(rt.state.composer.text, "👍");
        assert!(!rt.state.emoji_open);
    }

    #[test]
    fn the_selected_skin_tone_is_inserted_in_the_draft() {
        let mut rt = rt();
        click(&mut rt, "composer-emoji");
        click(&mut rt, "emoji-tone-3");
        click(&mut rt, "emoji-tab-1");
        click(&mut rt, "emoji-👍");
        assert_eq!(rt.state.composer.text, "👍🏽");
    }

    #[test]
    fn typing_a_colon_opens_emoji_suggestions_and_enter_picks_without_sending() {
        let mut rt = rt();
        click(&mut rt, "composer");
        press(&mut rt, Key::Text(":thumbsup".into()));
        assert!(rt.find("emoji-suggestions").is_some());
        press(&mut rt, Key::Enter);
        assert_eq!(rt.state.composer.text, "👍 ");
        assert!(calls(&rt).iter().all(|call| !call.starts_with("send")));
    }

    #[test]
    fn a_picked_gif_waits_as_a_chip_and_goes_out_with_the_message() {
        let mut rt = rt();
        click(&mut rt, "composer-emoji");
        click(&mut rt, "fun-tab-gif");
        assert!(rt.find("gif-search").is_some(), "the GIF tab shows the GIF picker");
        assert_eq!(calls(&rt), ["gifs []"]);
        let gif = chatsvc::Gif { url: "https://g.test/a.gif".into(), preview_url: String::new(), title: "wave".into(), width: 10, height: 10, preview_width: 10, preview_height: 10 };
        rt.update(|s, _| s.apply(Update::Gifs { query: "old".into(), result: Ok(vec![]) }));
        rt.update(|s, _| s.apply(Update::Gifs { query: String::new(), result: Ok(vec![gif]) }));
        rt.frame(1.0 / 60.0);
        click(&mut rt, "gif-0");
        assert!(rt.find("gif-chip-0").is_some() && rt.find("gif-picker").is_none());
        click(&mut rt, "send");
        assert!(calls(&rt).contains(&"send [] mentions [] gifs 1".to_owned()), "{:?}", calls(&rt));
        assert!(rt.find("gif-chip-0").is_none());
    }

    #[test]
    fn ctrl_b_and_the_underline_button_wrap_the_selection() {
        let mut rt = rt();
        click(&mut rt, "composer");
        press(&mut rt, Key::Command('b'));
        assert_eq!(rt.state.composer.text, "**text**");
        click(&mut rt, "composer-format");
        click(&mut rt, "format-Underline");
        assert!(rt.state.composer.text.contains("<u>"), "{}", rt.state.composer.text);
    }

    #[test]
    fn drafts_are_saved_when_they_change_and_come_back_on_open() {
        let mut rt = rt_with(Quiet { stored_draft: Some("from last run".into()), ..Default::default() });
        assert_eq!(rt.state.composer.text, "from last run");
        rt.update(|s, _| persist_draft(s));
        rt.update(|s, _| persist_draft(s));
        rt.update(|s, _| s.composer = TextEdit::new("typed"));
        rt.update(|s, _| persist_draft(s));
        let saved: Vec<String> = calls(&rt).into_iter().filter(|c| c.starts_with("draft")).collect();
        assert_eq!(saved, ["draft b [from last run]", "draft b [typed]"]);
    }

    #[test]
    fn switching_chats_preserves_delivery_options_and_gifs_without_leaking_them() {
        let mut rt = rt();
        rt.update(|s, tx| {
            s.compose.importance = store::MessageImportance::High;
            s.compose.subject = TextEdit::new("Subject");
            s.compose.headline = TextEdit::new("Headline");
            crate::ui::open_chat(s, "a", tx);
        });
        assert_eq!(rt.state.compose.importance, store::MessageImportance::Normal);
        assert!(rt.state.compose.subject.text.is_empty());
        rt.update(|s, tx| crate::ui::open_chat(s, "b", tx));
        assert_eq!(rt.state.compose.importance, store::MessageImportance::High);
        assert_eq!(rt.state.compose.subject.text, "Subject");
        assert_eq!(rt.state.compose.headline.text, "Headline");
    }

    /// A bare composer (no surrounding screen) so the buttons can be inspected without a chat list.
    fn composer_view(s: &UiState<Quiet>, _cx: &mut Cx<UiState<Quiet>>) -> El {
        composer::<Quiet>(s, false)
    }

    fn composer_fixture(text: &str, format_open: bool, importance: store::MessageImportance) -> Runtime<UiState<Quiet>> {
        let mut state = UiState::new(Quiet::default());
        state.selected = Some("b".into());
        state.composer = TextEdit::new(text);
        state.format_open = format_open;
        state.compose.importance = importance;
        let env = Env { system_dark: false, scale: 1.0 };
        let mut rt = Runtime::new(
            state,
            composer_view as fn(&UiState<Quiet>, &mut Cx<UiState<Quiet>>) -> El,
            (1000.0, 700.0),
            env,
        );
        rt.frame(0.0);
        rt
    }

    fn element<'a>(elements: &'a [ElementInfo], key: &str) -> &'a ElementInfo {
        elements
            .iter()
            .find(|e| e.key.as_deref() == Some(key))
            .unwrap_or_else(|| panic!("no element {key}"))
    }

    #[test]
    fn send_button_is_accessible_and_switches_from_quiet_to_accent() {
        let empty = composer_fixture("", false, store::MessageImportance::Normal);
        let typed = composer_fixture("hello", false, store::MessageImportance::Normal);
        let empty_elements = empty.elements();
        let typed_elements = typed.elements();
        let quiet = element(&empty_elements, "send");
        let accent = element(&typed_elements, "send");
        for (e, state) in [(quiet, "empty"), (accent, "typed")] {
            assert_eq!(e.role, "button", "{state}");
            assert_eq!(e.label.as_deref(), Some("Send message"), "{state}");
            assert!(e.focusable, "send stays in the tab order ({state})");
        }
        assert_eq!(accent.fill, rune::color::hex(&format!("#{:06X}", active_palette().accent)).unwrap());
        assert_eq!(quiet.fill.a, 0.0, "no accent fill without a draft");
        assert!(quiet.opacity < 1.0, "the empty button is dimmed");
        assert_ne!(quiet.fill, accent.fill, "the fill changes with the draft");
    }

    #[test]
    fn every_composer_tool_has_an_accessible_name() {
        let rt = composer_fixture("", false, store::MessageImportance::Normal);
        let elements = rt.elements();
        let tools: Vec<_> = elements
            .iter()
            .filter(|e| e.path.contains("/composer-tools/") && e.role == "button")
            .collect();
        assert!(!tools.is_empty(), "the toolbar has buttons");
        for e in &tools {
            assert!(e.label.as_deref().is_some_and(|l| !l.is_empty()), "unnamed tool at {}", e.path);
            assert!(e.focusable, "{} is keyboard reachable", e.path);
        }
        let mut names: Vec<_> = tools.iter().map(|e| e.label.as_deref().unwrap_or("")).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            ["Actions and apps", "Ask AI", "Attach file", "Emoji, GIFs and stickers", "Formatting"],
            "one explicit name per icon"
        );
    }

    fn right_click(rt: &mut Runtime<UiState<Quiet>>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
        rt.secondary_down(x, y);
        rt.secondary_up(x, y);
        rt.frame(1.0 / 60.0);
    }

    fn large_group(rt: &mut Runtime<UiState<Quiet>>) {
        let members = (0..21).map(|i| member(&format!("u{i}"), "Ada")).collect();
        let details = crate::types::ChatDetails { members, kind: "group".into(), ..Default::default() };
        rt.update(|s, _| s.apply(Update::Details([("b".to_owned(), details)].into())));
    }

    #[test]
    fn the_send_chevron_is_gone_and_right_click_on_send_opens_the_scheduler() {
        let mut rt = rt();
        assert!(rt.find("composer-send-options").is_none() && rt.find("composer-send-menu").is_none());
        click(&mut rt, "composer");
        press(&mut rt, Key::Text("later".into()));
        right_click(&mut rt, "send");
        assert!(rt.find("schedule-picker").is_some(), "right-click on send schedules");
        assert!(calls(&rt).iter().all(|c| !c.starts_with("send")), "right-click did not send");
    }

    #[test]
    fn the_plus_button_opens_the_actions_menu_not_the_file_picker() {
        let mut rt = rt();
        click(&mut rt, "composer-add");
        assert!(rt.find("composer-plus-menu").is_some());
        for key in ["composer-plus/schedule-message", "composer-plus/delivery-options", "composer-plus/schedule-meeting"] {
            assert!(rt.find(key).is_some(), "{key}");
        }
        click(&mut rt, "composer-plus/schedule-message");
        assert!(rt.find("composer-plus-menu").is_none() && rt.find("schedule-picker").is_some());
    }

    #[test]
    fn schedule_meeting_opens_the_form_with_the_chat_members() {
        let mut rt = rt();
        let details = crate::types::ChatDetails { members: vec![member("u1", "Ada"), member("u2", "Bo")], kind: "group".into(), ..Default::default() };
        rt.update(|s, _| s.apply(Update::Details([("b".to_owned(), details)].into())));
        click(&mut rt, "composer-add");
        click(&mut rt, "composer-plus/schedule-meeting");
        assert!(rt.state.meeting.open);
        assert_eq!(rt.state.meeting.people.chips.len(), 2, "the chat's people are invited");
    }

    #[test]
    fn delivery_options_set_the_importance_without_sending() {
        let mut rt = rt();
        click(&mut rt, "composer");
        press(&mut rt, Key::Text("urgent?".into()));
        click(&mut rt, "composer-add");
        click(&mut rt, "composer-plus/delivery-options");
        click(&mut rt, "urgent-delivery-option");
        assert_eq!(rt.state.compose.importance, store::MessageImportance::Urgent);
        assert!(rt.find("delivery-menu").is_none(), "choosing closes the menu");
        assert!(calls(&rt).iter().all(|c| !c.starts_with("send")), "choosing did not send");
    }

    #[test]
    fn urgent_is_hidden_in_channels_and_meetings_and_disabled_above_twenty_people() {
        let target = |id: &str, channel: bool| SendTarget { conversation_id: id.into(), thread_root_id: None, channel };
        let mut s = UiState::new(Quiet::default());
        assert_eq!(urgent_availability(&s, Some(&target("b", true))), UrgentAvailability::Hidden);
        s.details.insert("m".into(), crate::types::ChatDetails { kind: "meeting".into(), ..Default::default() });
        assert_eq!(urgent_availability(&s, Some(&target("m", false))), UrgentAvailability::Hidden);
        s.details.insert("b".into(), crate::types::ChatDetails { members: (0..21).map(|i| member(&format!("u{i}"), "A")).collect(), ..Default::default() });
        assert_eq!(urgent_availability(&s, Some(&target("b", false))), UrgentAvailability::Disabled);
        s.details.insert("b".into(), crate::types::ChatDetails { members: (0..20).map(|i| member(&format!("u{i}"), "A")).collect(), ..Default::default() });
        assert_eq!(urgent_availability(&s, Some(&target("b", false))), UrgentAvailability::Available);
        s.apply(Update::PriorityPolicy(false));
        assert_eq!(urgent_availability(&s, Some(&target("b", false))), UrgentAvailability::Hidden, "the policy denies Urgent");
    }

    #[test]
    fn disabled_urgent_is_shown_dimmed_with_the_limit_and_is_not_sent() {
        let mut rt = rt();
        large_group(&mut rt);
        click(&mut rt, "composer");
        press(&mut rt, Key::Text("hi".into()));
        click(&mut rt, "composer-add");
        click(&mut rt, "composer-plus/delivery-options");
        let elements = rt.elements();
        let urgent = element(&elements, "urgent-delivery-option");
        assert!((urgent.opacity - 0.4).abs() < 0.01, "dimmed");
        assert_eq!(urgent.help.as_deref(), Some(URGENT_TOO_MANY_MEMBERS));
        rt.update(|s, _| { s.compose.importance = store::MessageImportance::Urgent; s.compose.delivery_menu = false; });
        rt.frame(1.0 / 60.0);
        assert!(rt.find("composer-importance-label").is_none(), "the unavailable importance is not shown");
        let elements = rt.elements();
        let plus = elements.iter().position(|e| e.key.as_deref() == Some("composer-add")).expect("plus");
        let glyph = elements.iter().find(|e| e.parent == Some(plus) && e.text_color.is_some()).expect("plus icon");
        assert_ne!(glyph.text_color, rune::color::hex(&format!("#{:06X}", active_palette().red)), "no red plus either");
        right_click(&mut rt, "send");
        assert!(rt.find("schedule-picker").is_some());
        assert!(rt.elements().iter().all(|e| e.text.as_deref() != Some("Scheduled messages use standard importance")),
            "an importance that is not shown does not block scheduling");
        click(&mut rt, "send");
        assert!(calls(&rt).contains(&"importance Normal".to_owned()), "{:?}", calls(&rt));
    }

    #[test]
    fn a_chosen_importance_is_labelled_and_turns_the_plus_red() {
        let mut rt = rt();
        rt.update(|s, _| s.compose.importance = store::MessageImportance::High);
        rt.frame(1.0 / 60.0);
        assert!(rt.find("composer-importance-label").is_some(), "IMPORTANT! label");
        assert!(rt.find("delivery-importance").is_none() && rt.find("importance-Important").is_none(), "no duplicate chips");
        let elements = rt.elements();
        let plus = elements.iter().position(|e| e.key.as_deref() == Some("composer-add")).expect("plus");
        let glyph = elements.iter().find(|e| e.parent == Some(plus) && e.text_color.is_some()).expect("plus icon");
        assert_eq!(glyph.text_color, rune::color::hex(&format!("#{:06X}", active_palette().red)));
    }

    #[test]
    fn format_tools_share_the_small_menu_button_geometry() {
        let mut rt = composer_fixture("", true, store::MessageImportance::Normal);
        let elements = rt.elements();
        let bold = element(&elements, "format-Bold");
        let clear = element(&elements, "format-clear");
        for e in [bold, clear] {
            assert_eq!(e.role, "button");
            assert!(e.label.as_deref().is_some_and(|l| !l.is_empty()));
            assert!(e.focusable);
            assert!((e.rect[3] - 24.0).abs() < 0.5, "Sm height, got {}", e.rect[3]);
            assert_eq!(e.fill.a, 0.0, "Menu has no fill at rest");
        }
        click(&mut rt, "format-Bold");
        assert_eq!(rt.state.composer.text, "**text**", "the tools still format");
    }
}
