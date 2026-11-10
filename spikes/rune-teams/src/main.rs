//! Spike: the new Teams desktop shell in Rune, with static demo data and no network.
//! `cargo run` opens the window. `cargo run -- --snap screenshot.png --size 1600x900 --scale 1`
//! renders the same view headless.

use rune::tok::Family;
use rune::*;

const PURPLE: &str = "#5B5FC7";
const RAIL: &str = "#EBEBEB";
const LIST: &str = "#F5F5F5";
const LINE: &str = "#E0E0E0";
const BUBBLE: &str = "#F5F5F5";
const LAVENDER: &str = "#E8EBFA";
const INK: &str = "#242424";
const MUTED: &str = "#616161";
const PRESENCE: &str = "#6BB700";
const RED: &str = "#C4314B";
const WORD: &str = "#2B579A";
const WHITE: &str = "#FFFFFF";
const NOW: &str = "11:02";

fn c(s: &str) -> Color {
    rune::color::hex(s).expect("hex colour")
}

fn ts(size: f32, weight: f32) -> TextStyle {
    TextStyle::new(size, weight).family(Family::Named("Segoe UI"))
}

// Icons: 20-unit viewBox outlines, drawn in the element's colour.
static BELL: Icon = Icon { name: "bell", view: 20.0, body: r#"<g fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M10 3.5a4.2 4.2 0 0 0-4.2 4.2v2.6L4.5 13.5h11l-1.3-3.2V7.7A4.2 4.2 0 0 0 10 3.5Z"/><path d="M8.3 16a1.8 1.8 0 0 0 3.4 0"/></g>"# };
static CHAT_FILLED: Icon = Icon { name: "chat-filled", view: 20.0, body: r#"<g fill="currentColor" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"><path d="M4.5 5.5A1.5 1.5 0 0 1 6 4h8a1.5 1.5 0 0 1 1.5 1.5v6A1.5 1.5 0 0 1 14 13H9l-3.5 2.8V13H6a1.5 1.5 0 0 1-1.5-1.5Z"/></g>"# };
static TEAMS: Icon = Icon { name: "teams", view: 20.0, body: r#"<g fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"><circle cx="7.5" cy="7" r="2.6"/><path d="M2.8 15.2c.4-2.6 2.2-4.2 4.7-4.2s4.3 1.6 4.7 4.2"/><circle cx="13.6" cy="7.6" r="2.1"/><path d="M13.4 11.2c2.1 0 3.5 1.2 3.8 3.4"/></g>"# };
static CALENDAR: Icon = Icon { name: "calendar", view: 20.0, body: r#"<g fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"><rect x="3" y="4.5" width="14" height="12" rx="2"/><path d="M3 8.5h14M7 3v3M13 3v3"/></g>"# };
static PHONE: Icon = Icon { name: "phone", view: 20.0, body: r#"<g fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round"><path d="M5.4 3.6h2.2l1.1 3.3-1.7 1.3a8.3 8.3 0 0 0 4.8 4.8l1.3-1.7 3.3 1.1v2.2a1.6 1.6 0 0 1-1.7 1.6C8.4 16.2 3.8 11.6 3.8 5.3a1.6 1.6 0 0 1 1.6-1.7Z"/></g>"# };
static CLOUD: Icon = Icon { name: "cloud", view: 20.0, body: r#"<g fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round"><path d="M6 15.5h8.2a3.1 3.1 0 0 0 .4-6.2 4.8 4.8 0 0 0-9.2 1.3A2.6 2.6 0 0 0 6 15.5Z"/></g>"# };
static DOTS: Icon = Icon { name: "dots", view: 20.0, body: r#"<g fill="currentColor" stroke="none"><circle cx="5" cy="10" r="1.3"/><circle cx="10" cy="10" r="1.3"/><circle cx="15" cy="10" r="1.3"/></g>"# };
static APPS: Icon = Icon { name: "apps", view: 20.0, body: r#"<g fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"><rect x="3.5" y="3.5" width="13" height="13" rx="3"/><path d="M10 7v6M7 10h6"/></g>"# };
static FILTER: Icon = Icon { name: "filter", view: 20.0, body: r#"<g fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"><path d="M3.5 5.5h13M6 10h8M8.5 14.5h3"/></g>"# };
static COMPOSE: Icon = Icon { name: "compose", view: 20.0, body: r#"<g fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" stroke-linecap="round"><path d="M9 4.5H5.5A1.5 1.5 0 0 0 4 6v8.5A1.5 1.5 0 0 0 5.5 16H14a1.5 1.5 0 0 0 1.5-1.5V11"/><path d="M14.5 3.2l2.3 2.3L11 11.3H8.7V9Z"/></g>"# };
static CHEVRON_LEFT: Icon = Icon { name: "chevron-left", view: 20.0, body: r#"<g fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M12.5 4.5L7 10l5.5 5.5"/></g>"# };
static CHEVRON_RIGHT: Icon = Icon { name: "chevron-right", view: 20.0, body: r#"<g fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M7.5 4.5L13 10l-5.5 5.5"/></g>"# };
static CHEVRON_DOWN: Icon = Icon { name: "chevron-down", view: 20.0, body: r#"<g fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M5 7.5l5 5 5-5"/></g>"# };
static SEARCH: Icon = Icon { name: "search", view: 20.0, body: r#"<g fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"><circle cx="9" cy="9" r="5.2"/><path d="M13 13l3.8 3.8"/></g>"# };
static ADD_PEOPLE: Icon = Icon { name: "add-people", view: 20.0, body: r#"<g fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"><circle cx="8" cy="7" r="2.7"/><path d="M3 16c.4-2.7 2.4-4.3 5-4.3s4.6 1.6 5 4.3"/><path d="M15 6.5v5M12.5 9h5"/></g>"# };
static POP_OUT: Icon = Icon { name: "pop-out", view: 20.0, body: r#"<g fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M11 4h5v5M16 4l-6.5 6.5M8.5 5.5H5.5A1.5 1.5 0 0 0 4 7v7.5A1.5 1.5 0 0 0 5.5 16H13a1.5 1.5 0 0 0 1.5-1.5v-3"/></g>"# };
static FORMAT: Icon = Icon { name: "format", view: 20.0, body: r#"<g fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M3.5 15.5L7.8 4.5l4.3 11M5.4 11.6h4.8"/></g>"# };
static EMOJI: Icon = Icon { name: "emoji", view: 20.0, body: r#"<g fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"><circle cx="10" cy="10" r="6.5"/><path d="M7 11.2c.8 1.2 1.8 1.7 3 1.7s2.2-.5 3-1.7"/><circle cx="7.6" cy="8.2" r=".6" fill="currentColor"/><circle cx="12.4" cy="8.2" r=".6" fill="currentColor"/></g>"# };
static STICKER: Icon = Icon { name: "sticker", view: 20.0, body: r#"<g fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M4.5 4.5A1.5 1.5 0 0 1 6 3h8a1.5 1.5 0 0 1 1.5 1.5V11l-5.5 5.5H6A1.5 1.5 0 0 1 4.5 15Z"/><path d="M15.5 11h-3A1.5 1.5 0 0 0 11 12.5V16"/></g>"# };
static PLUS: Icon = Icon { name: "plus", view: 20.0, body: r#"<g fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"><path d="M10 4.5v11M4.5 10h11"/></g>"# };
static SEND: Icon = Icon { name: "send", view: 20.0, body: r#"<g fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"><path d="M3.5 10l13-6-2.8 12-3.9-4.3L3.5 10Z"/><path d="M7.9 11.7l4.8-4.2"/></g>"# };
static HEART: Icon = Icon { name: "heart", view: 20.0, body: r#"<path d="M10 16.4 4.4 10.9a3.3 3.3 0 0 1 4.7-4.6L10 7.1l.9-.8a3.3 3.3 0 0 1 4.7 4.6Z" fill="currentColor"/>"# };

#[derive(Clone, Copy, PartialEq)]
enum Section {
    Pinned,
    Recent,
}

#[derive(Clone)]
struct Msg {
    mine: bool,
    who: &'static str,
    time: &'static str,
    text: String,
    heart: u32,
    file: bool,
}

impl Msg {
    fn theirs(who: &'static str, time: &'static str, text: &str) -> Msg {
        Msg { mine: false, who, time, text: text.into(), heart: 0, file: false }
    }
}

#[derive(Clone)]
struct Chat {
    name: &'static str,
    initials: &'static str,
    tint: &'static str,
    section: Section,
    online: bool,
    unread: bool,
    preview: String,
    time: &'static str,
    msgs: Vec<Msg>,
}

struct State {
    chats: Vec<Chat>,
    selected: usize,
    collapsed: [bool; 2],
    composer: TextEdit,
    calling: bool,
}

impl State {
    fn new() -> State {
        // name, initials, tint, section, online, unread, last preview, last time
        let rows: [(&'static str, &'static str, &'static str, Section, bool, bool, &str, &'static str); 12] = [
            ("Ray Tanaka", "RT", "#C7E0F4", Section::Pinned, true, false, "Sounds good, see you then.", "09:12"),
            ("Beth Davies", "BD", "#D4E8D0", Section::Pinned, false, false, "Sent you the notes", "Mon"),
            ("Kayo Miwa", "KM", "#F4D6E8", Section::Pinned, true, false, "Great, thanks for the update", "Mon"),
            ("Dream team", "DT", "#E4D4F4", Section::Pinned, false, false, "Lunch on Friday?", "Tue"),
            ("August Bergman", "AB", "#FCE7B2", Section::Recent, true, true, "Can you review the deck tonight?", "11:20"),
            ("Charlotte and Babak", "CB", "#D0E6F3", Section::Recent, false, false, "Lol yes", "09:55"),
            ("Emiliano Ceballos", "EC", "#F9D5D5", Section::Recent, true, true, "Let's sync after the call", "10:30"),
            ("Marie Beaudouin", "MB", "#F9D5BB", Section::Recent, true, false, "You: Thanks, I'll read it after lunch.", "10:36"),
            ("Oscar Krogh", "OK", "#DDEFD5", Section::Recent, false, false, "Ok, will do", "Sun"),
            ("Marketing team sync", "MT", "#E0DCF7", Section::Recent, false, false, "Deck is final", "Sun"),
            ("Kian Lambert", "KL", "#CFE8E4", Section::Recent, true, false, "See you tomorrow", "Sat"),
            ("Team Design Template", "TD", "#F3E3C3", Section::Recent, false, false, "Template v3 is up", "Fri"),
        ];
        let mut chats: Vec<Chat> = rows
            .iter()
            .map(|&(name, initials, tint, section, online, unread, preview, time)| Chat {
                name,
                initials,
                tint,
                section,
                online,
                unread,
                preview: preview.into(),
                time,
                msgs: vec![Msg::theirs(name, time, preview)],
            })
            .collect();
        chats[7].msgs = marie_messages();
        State { chats, selected: 7, collapsed: [false; 2], composer: TextEdit::new(""), calling: false }
    }

    fn select(&mut self, i: usize) {
        self.selected = i;
        self.chats[i].unread = false;
        self.calling = false;
    }
}

fn marie_messages() -> Vec<Msg> {
    let them = |time: &'static str, text: &str| Msg::theirs("Marie Beaudouin", time, text);
    let me = |time: &'static str, text: &str| Msg { mine: true, who: "You", time, text: text.into(), heart: 0, file: false };
    vec![
        them("09:02", "Hi! Did you get a chance to look at the new onboarding flow?"),
        them("09:02", "I left some comments on the mockups."),
        me("09:10", "Yes, I went through them this morning. Most of them look good."),
        me("09:10", "I have two questions about the empty state."),
        Msg { heart: 1, ..them("09:14", "Perfect, thank you! I'll take a look at those.") },
        them("10:31", "Here is the latest version of the brief."),
        Msg { file: true, ..them("10:31", "Design Brief.docx") },
        me("10:36", "Thanks, I'll read it after lunch."),
    ]
}

fn send(s: &mut State, tx: &mut Tx) {
    let body = s.composer.text.trim().to_string();
    if body.is_empty() {
        return;
    }
    s.composer = TextEdit::new("");
    let chat = &mut s.chats[s.selected];
    chat.msgs.push(Msg { mine: true, who: "You", time: NOW, text: body.clone(), heart: 0, file: false });
    chat.preview = format!("You: {body}");
    chat.time = NOW;
    tx.scroll_to_bottom("messages");
}

fn view(s: &State, cx: &mut Cx<State>) -> El {
    // Rune draws the Windows caption buttons at the trailing edge; the title bar leaves room for them.
    cx.caption_color(c(INK));
    let insets = cx.chrome_insets();
    col()
        .key("root")
        .w_full()
        .h_full()
        .fill(c(LIST))
        .child(titlebar(insets.leading, insets.trailing))
        .child(
            row()
                .key("body")
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .child(rail())
                .child(chat_list(s))
                .child(conversation(s)),
        )
}

fn titlebar(leading: f32, trailing: f32) -> El {
    let side = 200.0 + leading.max(trailing);
    let logo = col()
        .key("logo")
        .size(24.0, 24.0)
        .radius(5.0)
        .fill(c(PURPLE))
        .center()
        .child(text("T").font(ts(14.0, 700.0)).color(c(WHITE)));
    let left = row()
        .w(side)
        .h(48.0)
        .center_y()
        .pl(12.0 + leading)
        .gap(8.0)
        .child(logo)
        .child(icon(&CHEVRON_LEFT, 18.0).color(c(MUTED)))
        .child(icon(&CHEVRON_RIGHT, 18.0).color(c(MUTED)));
    let search = row()
        .key("search")
        .w(480.0)
        .h(32.0)
        .px(10.0)
        .gap(8.0)
        .center_y()
        .radius(6.0)
        .fill(c(WHITE))
        .border(1.0, c(LINE))
        .child(icon(&SEARCH, 16.0).color(c(MUTED)))
        .child(text("Search").font(ts(14.0, 400.0)).color(c(MUTED)));
    let right = row()
        .w(side)
        .h(48.0)
        .justify_end()
        .center_y()
        .gap(4.0)
        .pr(trailing)
        .child(icon_btn(&DOTS, 18.0))
        .child(avatar("YO", "#D6D6F5", 28.0, true));
    row()
        .key("titlebar")
        .w_full()
        .h(48.0)
        .fill(c(RAIL))
        .center_y()
        .window_drag()
        .child(left)
        .child(el().grow(1.0))
        .child(search)
        .child(el().grow(1.0))
        .child(right)
}

fn icon_btn(glyph: &'static Icon, size: f32) -> El {
    row()
        .size(32.0, 32.0)
        .center()
        .radius(6.0)
        .hover(c("#DCDCDC"))
        .child(icon(glyph, size).color(c(INK)))
}

fn rail() -> El {
    col()
        .key("rail")
        .w(68.0)
        .h_full()
        .min_h(0.0)
        .fill(c(RAIL))
        .pt(4.0)
        .pb(8.0)
        .child(rail_item("Activity", &BELL, false, None))
        .child(rail_item("Chat", &CHAT_FILLED, true, Some("2")))
        .child(rail_item("Teams", &TEAMS, false, None))
        .child(rail_item("Calendar", &CALENDAR, false, None))
        .child(rail_item("Calls", &PHONE, false, None))
        .child(rail_item("OneDrive", &CLOUD, false, None))
        .child(rail_item("…", &DOTS, false, None))
        .child(el().grow(1.0))
        .child(rail_item("Apps", &APPS, false, None))
}

fn rail_item(label: &'static str, glyph: &'static Icon, active: bool, badge: Option<&'static str>) -> El {
    let ink = c(if active { INK } else { MUTED });
    col()
        .key(format!("rail-{label}"))
        .w(68.0)
        .h(58.0)
        .center()
        .gap(2.0)
        .child(icon(glyph, 22.0).color(ink))
        .child(text(label).font(ts(11.0, 400.0)).color(ink))
        .children(active.then(|| el().absolute(0.0, 15.0).size(3.0, 28.0).radius(2.0).fill(c(PURPLE))))
        .children(badge.map(|n| {
            el()
                .absolute(38.0, 6.0)
                .size(16.0, 16.0)
                .radius(8.0)
                .fill(c(RED))
                .center()
                .child(text(n).font(ts(10.0, 600.0)).color(c(WHITE)))
        }))
}

fn chat_list(s: &State) -> El {
    let section = |idx: usize, label: &'static str, which: Section| -> Vec<El> {
        let collapsed = s.collapsed[idx];
        let header = row()
            .key(format!("section-{label}"))
            .w_full()
            .h(28.0)
            .px(8.0)
            .gap(4.0)
            .center_y()
            .radius(4.0)
            .hover(c("#EBEBEB"))
            .on_press(move |s: &mut State| s.collapsed[idx] = !s.collapsed[idx])
            .child(icon(if collapsed { &CHEVRON_RIGHT } else { &CHEVRON_DOWN }, 12.0).color(c(MUTED)))
            .child(text(label).font(ts(12.0, 600.0)).color(c(MUTED)));
        let mut out = vec![header];
        if !collapsed {
            out.extend(
                s.chats
                    .iter()
                    .enumerate()
                    .filter(|(_, ch)| ch.section == which)
                    .map(|(i, ch)| chat_row(i, ch, i == s.selected)),
            );
        }
        out
    };
    let mut rows = section(0, "Pinned", Section::Pinned);
    rows.extend(section(1, "Recent", Section::Recent));
    col()
        .key("chats")
        .w(320.0)
        .h_full()
        .min_h(0.0)
        .fill(c(LIST))
        .child(
            row()
                .h(56.0)
                .px(16.0)
                .gap(12.0)
                .center_y()
                .child(text("Chat").font(ts(20.0, 700.0)).color(c(INK)))
                .child(el().grow(1.0))
                .child(icon(&FILTER, 18.0).color(c(INK)))
                .child(icon(&COMPOSE, 18.0).color(c(INK))),
        )
        .child(
            col()
                .key("chat-list")
                .grow(1.0)
                .min_h(0.0)
                .w_full()
                .px(8.0)
                .pb(8.0)
                .gap(2.0)
                .scroll(ScrollSpec { stick_bottom: false })
                .children(rows),
        )
}

fn chat_row(i: usize, ch: &Chat, selected: bool) -> El {
    let dot = el().size(6.0, 6.0).radius(3.0).fill(if ch.unread { c(PURPLE) } else { Color::BLACK.alpha(0.0) });
    let name_weight = if ch.unread { 700.0 } else { 400.0 };
    row()
        .key(format!("chat-{i}"))
        .w_full()
        .h(56.0)
        .px(10.0)
        .gap(10.0)
        .center_y()
        .radius(8.0)
        .fill(if selected { c(WHITE) } else { c(LIST) })
        .hover(c("#EBEBEB"))
        .on_press(move |s: &mut State| s.select(i))
        .child(avatar(ch.initials, ch.tint, 36.0, ch.online))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(2.0)
                .child(
                    row()
                        .w_full()
                        .gap(6.0)
                        .center_y()
                        .child(dot)
                        .child(text(ch.name).font(ts(14.0, name_weight)).color(c(INK)).truncate().grow(1.0).min_w(0.0))
                        .child(text(ch.time).font(ts(12.0, 400.0)).color(c(MUTED))),
                )
                .child(text(ch.preview.clone()).font(ts(13.0, 400.0)).color(c(MUTED)).truncate()),
        )
}

fn avatar(initials: &str, tint: &str, size: f32, online: bool) -> El {
    let circle = col()
        .size(size, size)
        .radius(size / 2.0)
        .fill(c(tint))
        .center()
        .child(text(initials).font(ts(size * 0.36, 600.0)).color(c(INK)));
    el()
        .size(size, size)
        .child(circle)
        .children(online.then(|| {
            el()
                .absolute(size - 11.0, size - 11.0)
                .size(12.0, 12.0)
                .radius(6.0)
                .fill(c(PRESENCE))
                .border(2.0, c(WHITE))
        }))
}

fn conversation(s: &State) -> El {
    let chat = &s.chats[s.selected];
    col()
        .key("conversation")
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .min_h(0.0)
        .fill(c(WHITE))
        .child(conversation_header(chat))
        .children(s.calling.then(|| call_banner(chat.name)))
        .child(el().h(1.0).w_full().fill(c(LINE)))
        .child(message_list(chat))
        .child(composer(s))
}

fn conversation_header(chat: &Chat) -> El {
    let tab = |label: &'static str, active: bool| {
        col()
            .gap(2.0)
            .child(text(label).font(ts(13.0, if active { 600.0 } else { 400.0 })).color(c(if active { INK } else { MUTED })))
            .child(el().h(2.0).w_full().fill(if active { c(PURPLE) } else { Color::BLACK.alpha(0.0) }))
    };
    let tabs = row()
        .gap(20.0)
        .center_y()
        .child(tab("Chat", true))
        .child(tab("Shared", false))
        .child(icon(&PLUS, 16.0).color(c(MUTED)));
    let call_button = row()
        .key("call")
        .h(32.0)
        .px(8.0)
        .gap(6.0)
        .center_y()
        .radius(6.0)
        .border(1.0, c(LINE))
        .hover(c("#EBEBEB"))
        .on_press(|s: &mut State| s.calling = !s.calling)
        .child(icon(&PHONE, 16.0).color(c(PURPLE)))
        .child(icon(&CHEVRON_DOWN, 12.0).color(c(INK)));
    row()
        .h(64.0)
        .px(16.0)
        .gap(12.0)
        .center_y()
        .child(avatar(chat.initials, chat.tint, 36.0, chat.online))
        .child(
            col()
                .gap(4.0)
                .child(text(chat.name).font(ts(16.0, 700.0)).color(c(INK)))
                .child(tabs),
        )
        .child(el().grow(1.0))
        .child(call_button)
        .child(icon_btn(&ADD_PEOPLE, 18.0))
        .child(icon_btn(&POP_OUT, 18.0))
        .child(icon_btn(&DOTS, 18.0))
}

fn call_banner(name: &str) -> El {
    row()
        .key("banner")
        .h(44.0)
        .px(16.0)
        .gap(10.0)
        .center_y()
        .fill(c("#EEF0FB"))
        .child(icon(&PHONE, 16.0).color(c(PURPLE)))
        .child(text(format!("Calling {name}…")).font(ts(14.0, 600.0)).color(c(INK)))
        .child(el().grow(1.0))
        .child(
            row()
                .key("end-call")
                .h(28.0)
                .px(12.0)
                .radius(6.0)
                .fill(c(RED))
                .center()
                .on_press(|s: &mut State| s.calling = false)
                .child(text("End call").font(ts(13.0, 600.0)).color(c(WHITE))),
        )
}

fn message_list(chat: &Chat) -> El {
    let rows: Vec<El> = chat
        .msgs
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let header = i
                .checked_sub(1)
                .map_or(true, |p| {
                    let prev = &chat.msgs[p];
                    prev.mine != m.mine || prev.who != m.who || prev.time != m.time
                });
            message(chat, m, header)
        })
        .collect();
    col()
        .key("messages")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .px(20.0)
        .py(12.0)
        .gap(12.0)
        .justify_end()
        .scroll(ScrollSpec { stick_bottom: true })
        .children(rows)
}

fn message(chat: &Chat, m: &Msg, header: bool) -> El {
    let bubble = if m.file {
        row()
            .w(320.0)
            .padding(10.0)
            .gap(10.0)
            .center_y()
            .radius(8.0)
            .fill(c(WHITE))
            .border(1.0, c(LINE))
            .child(
                col()
                    .size(40.0, 40.0)
                    .radius(4.0)
                    .fill(c(WORD))
                    .center()
                    .child(text("W").font(ts(18.0, 700.0)).color(c(WHITE))),
            )
            .child(
                col()
                    .gap(2.0)
                    .child(text(m.text.clone()).font(ts(14.0, 600.0)).color(c(INK)))
                    .child(text("Daichi Fukuda's Files").font(ts(12.0, 400.0)).color(c(MUTED))),
            )
    } else {
        col()
            .max_w(560.0)
            .radius(12.0)
            .px(12.0)
            .py(8.0)
            .fill(c(if m.mine { LAVENDER } else { BUBBLE }))
            .child(text(m.text.clone()).font(ts(14.0, 400.0)).color(c(INK)).wrap())
    };
    let reaction = (m.heart > 0).then(|| {
        row()
            .h(20.0)
            .px(6.0)
            .gap(4.0)
            .center_y()
            .radius(10.0)
            .fill(c(WHITE))
            .border(1.0, c(LINE))
            .child(icon(&HEART, 12.0).color(c(RED)))
            .child(text(m.heart.to_string()).font(ts(12.0, 400.0)).color(c(INK)))
    });
    if m.mine {
        row().w_full().justify_end().child(
            col()
                .items_end()
                .gap(4.0)
                .children(header.then(|| text(m.time).font(ts(12.0, 400.0)).color(c(MUTED))))
                .child(bubble),
        )
    } else {
        let head = row()
            .gap(8.0)
            .center_y()
            .child(text(m.who).font(ts(12.0, 700.0)).color(c(INK)))
            .child(text(m.time).font(ts(12.0, 400.0)).color(c(MUTED)));
        row()
            .w_full()
            .gap(8.0)
            .child(if header {
                avatar(chat.initials, chat.tint, 28.0, false)
            } else {
                el().size(28.0, 28.0)
            })
            .child(
                col()
                    .gap(4.0)
                    .items_start()
                    .children(header.then_some(head))
                    .child(bubble)
                    .children(reaction),
            )
    }
}

fn composer(s: &State) -> El {
    let field = text_input(&s.composer, ts(14.0, 400.0))
        .key("composer")
        .label("Message")
        .single_line()
        .placeholder("Type a message", c(MUTED))
        .color(c(INK))
        .grow(1.0)
        .min_w(0.0)
        .on_edit(|s: &mut State| &mut s.composer)
        .on_submit(|s: &mut State, tx: &mut Tx| send(s, tx));
    let send_button = row()
        .key("send")
        .size(30.0, 30.0)
        .radius(15.0)
        .fill(c(PURPLE))
        .center()
        .on_press_tx(|s: &mut State, tx: &mut Tx| send(s, tx))
        .child(icon(&SEND, 16.0).color(c(WHITE)));
    col().w_full().px(16.0).pt(8.0).pb(12.0).child(
        row()
            .w_full()
            .h(48.0)
            .px(12.0)
            .gap(4.0)
            .center_y()
            .radius(8.0)
            .fill(c(WHITE))
            .border(1.0, c(LINE))
            .child(field)
            .child(icon_btn(&FORMAT, 18.0))
            .child(icon_btn(&EMOJI, 18.0))
            .child(icon_btn(&STICKER, 18.0))
            .child(icon_btn(&PLUS, 18.0))
            .child(send_button),
    )
}

fn main() {
    App::new(AppSpec::new("Microsoft Teams", (1600.0, 900.0), State::new, view))
        .titlebar(Titlebar::new(48.0))
        .run();
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Env, Runtime};

    fn click(rt: &mut Runtime<State>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
    }

    #[test]
    fn a_click_selects_a_row_and_send_appends_to_it() {
        let env = Env { system_dark: false, scale: 1.0 };
        let mut rt = Runtime::new(State::new(), view, (1600.0, 900.0), env);
        rt.frame(0.0);
        click(&mut rt, "chat-4");
        assert_eq!(rt.state.selected, 4);
        assert!(!rt.state.chats[4].unread);
        rt.state.composer = TextEdit::new("Hello there");
        click(&mut rt, "send");
        assert_eq!(rt.state.chats[4].msgs.last().unwrap().text, "Hello there");
        assert_eq!(rt.state.chats[4].preview, "You: Hello there");
        assert!(rt.state.composer.text.is_empty());
    }
}
