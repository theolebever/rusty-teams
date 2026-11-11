//! Rusty Teams on Rune: chat list, conversation and composer on the real backend.
//! Run with `--help` for account, browser and demo options.

mod agenda;
mod agent_panel;
mod arguments;
mod avatar;
mod bridge;
mod cloud_picker;
mod composed;
mod emoji_picker;
mod gif_picker;
mod mentions;
mod calls;
mod calls_backend;
mod calendar_grid;
mod icons;
mod cards;
mod image_viewer;
mod link_preview;
mod media;
mod rich_text;
mod message_menu;
mod find;
mod shortcuts;
mod switcher;
mod chat_menu;
mod members;
mod new_chat;
mod voice;
mod notifications;
mod profile;
mod reactions;
mod screens;
mod search;
mod settings;
mod status_menu;
mod teams;
mod theme;
mod toast;
mod types;
mod view_chatlist;
mod ui;
mod view_chrome;
mod view_header;
mod view_messages;
mod view_composer;

use std::sync::OnceLock;

use arguments::Arguments;
use bridge::Bridge;
use rune::*;
use types::{Actions, ChannelRow, ChatRow, MessageRow, SendTarget, TeamRow, Update};
use ui::UiState;

const TITLE: &str = "Rusty Teams";
const SIZE: (f32, f32) = (1100.0, 700.0);

/// `init` takes no arguments and runs before `setup` hands out the waker, so both go through statics.
static WAKER: OnceLock<Waker> = OnceLock::new();
static ARGUMENTS: OnceLock<Arguments> = OnceLock::new();

fn init() -> UiState<Bridge> {
    let arguments = ARGUMENTS.get().expect("arguments are parsed before creating the window");
    let bridge = Bridge::start(arguments.database.clone(), arguments.no_sso, arguments.read_only, arguments.endpoint.clone(), || {
        if let Some(wake) = WAKER.get() {
            wake();
        }
    });
    let mut state = UiState::new(bridge);
    state.apply(Update::Status("Connecting…".into()));
    state
}

fn setup(_: &mut UiState<Bridge>, wake: Waker) {
    let _ = WAKER.set(wake.clone());
    // Updates may have been queued before the waker existed.
    wake();
    app_backend::runtime::handle().spawn(async {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            interval.tick().await;
            if let Some(wake) = WAKER.get() { wake(); }
        }
    });
}

fn poll(s: &mut UiState<Bridge>, _now: f64, tx: &mut Tx) -> bool {
    if s.media.voice.is_playing() { tx.wake_after(std::time::Duration::from_millis(100)); }
    let now = std::time::Instant::now();
    let input = app_backend::user_activity::latest_input(rune::last_input(), app_backend::user_activity::system_idle(), now);
    s.actions.set_activity(app_backend::user_activity::is_active(input, now, app_backend::user_activity::IDLE_AFTER));
    let chat = (rune_notify::app_focused() && matches!(s.screens.screen, screens::Screen::Chat | screens::Screen::Teams))
        .then_some(s.selected.as_deref()).flatten();
    s.actions.set_foreground(chat, s.calls.state.as_ref().is_some_and(calling::CallState::is_active));
    view_composer::persist_draft(s);
    let updates = s.actions.poll();
    // Arrivals animate in the one commit that adds them (`view_messages::snappy`).
    s.appearing.clear();
    let before = s.typing.len();
    s.typing.retain(|_, event| (chrono::Utc::now() - event.received_at).num_seconds() < 8);
    let changed = !updates.is_empty() || s.typing.len() != before || s.media.voice.is_playing();
    for update in updates {
        s.apply(update);
    }
    s.media.trim(now);
    chat_menu::drain(s, tx);
    // Direct writes to `status` (media errors) do not pass through `apply`.
    let status = s.status.clone();
    let noticed = s.toast.observe(&status);
    let ticked = s.toast.tick(now);
    let opened = s.profile.chat_request.take().map(|id| {
        screens::navigate(s, screens::Screen::Chat);
        ui::open_chat(s, &id, tx);
    });
    changed || noticed || ticked || opened.is_some()
}

fn main() {
    session::ensure_crypto_provider();
    if std::env::args().any(|argument| matches!(argument.as_str(), "--help" | "-h")) {
        print!("{}", arguments::HELP);
        return;
    }
    // Rune consumes its snapshot/script options and leaves account options for us.
    let arguments = match Arguments::parse(rune::extra_args().iter().cloned()) {
        Ok(arguments) => arguments,
        Err(error) => {
            eprintln!("{error}\nUse --help for available options.");
            std::process::exit(2);
        }
    };
    if arguments.help {
        print!("{}", arguments::HELP);
        return;
    }
    if !arguments.demo && arguments.database.is_none() && !rune::single_instance("rusty-teams-rune") {
        return;
    }
    let demo = arguments.demo;
    if !demo {
        let directory = arguments.database.as_deref()
            .and_then(std::path::Path::parent)
            .filter(|path| !path.as_os_str().is_empty())
            .map(std::path::Path::to_path_buf)
            .or_else(|| directories::ProjectDirs::from("", "", store::DATA_DIR_NAME)
                .map(|directories| directories.data_local_dir().to_path_buf()))
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        if let Err(error) = std::fs::create_dir_all(&directory) {
            eprintln!("Could not create {}: {error}", directory.display());
            std::process::exit(1);
        }
        app_backend::crash_log::install(directory.join(app_backend::crash_log::CRASH_LOG_FILE));
        app_backend::diagnostics::install(directory.join(app_backend::diagnostics::DIAGNOSTICS_FILE));
        if let Err(error) = rune_notify::register(&rune_notify::Identity::new("rusty-teams-rune", TITLE)) {
            app_backend::diagnostics::record(format!("Notifications unavailable: {error}"));
        }
    }
    ARGUMENTS.set(arguments).expect("arguments are initialized once");
    if demo {
        App::new(AppSpec {
            scenes: &[
                ("calendar", |s: &mut UiState<DemoActions>| s.screens.screen = screens::Screen::Calendar),
                ("activity", |s: &mut UiState<DemoActions>| s.screens.screen = screens::Screen::Activity),
                ("calls", |s: &mut UiState<DemoActions>| s.screens.screen = screens::Screen::Calls),
                ("settings", |s: &mut UiState<DemoActions>| s.screens.screen = screens::Screen::Settings),
            ],
            on_key: Some(shortcuts::on_key::<DemoActions>),
            ..AppSpec::new(TITLE, SIZE, demo_init, ui::view::<DemoActions>)
        })
        .titlebar(Titlebar::new(view_chrome::TITLE_BAR_HEIGHT))
        .run();
        return;
    }
    let app = App::new(AppSpec {
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(shortcuts::on_key::<Bridge>),
        ..AppSpec::new(TITLE, SIZE, init, ui::view::<Bridge>)
    })
    .titlebar(Titlebar::new(view_chrome::TITLE_BAR_HEIGHT))
    .on_close(|state, _| view_composer::persist_draft(state))
    .on_quit(|state, _| {
        state.actions.shutdown();
        app_backend::presence_link::unregister_on_exit(std::time::Duration::from_millis(800));
    });
    let handle = app.handle();
    let icon = ::image::load_from_memory(app_backend::appearance::AppIcon::MicrosoftTeams.png())
        .expect("embedded application icon").into_rgba8();
    let tray = Tray::new(
        TraySpec::new("rusty-teams-rune", TITLE, TrayIcon::rgba(icon.width(), icon.height(), icon.into_raw()))
            .tooltip(TITLE).menu(vec![TrayItem::action("open", "Ouvrir"), TrayItem::Separator, TrayItem::action("quit", "Quitter")]),
        move |event| match event {
            TrayEvent::Activate { .. } => handle.open(),
            TrayEvent::Menu(id) if id == "open" => handle.open(),
            TrayEvent::Menu(id) if id == "quit" => handle.quit(),
            _ => {}
        },
    );
    match tray {
        Ok(_tray) => app.keep_running().run(),
        Err(error) => {
            app_backend::diagnostics::record(format!("System tray unavailable: {error}"));
            app.run();
        }
    }
}

/// Demo only: fake actions and sample data, so the view can be looked at without a backend.
struct DemoActions;

impl Actions for DemoActions {
    fn open(&self, _conversation_id: &str) {}
    fn send(&self, _target: SendTarget, _text: String, _files: Vec<teams_core::FileReference>) {}
}

fn demo_init() -> UiState<DemoActions> {
    let mut s = UiState::new(DemoActions);
    s.apply(Update::Status("Online (demo data)".into()));
    s.apply(Update::Calendar {
        week: s.screens.week,
        rows: vec![screens::CalendarRow {
            id: "demo-meeting".into(),
            subject: "Project review".into(),
            start: chrono::Utc::now(),
            end: chrono::Utc::now() + chrono::Duration::minutes(30),
            cancelled: false,
            all_day: false,
            organizer: Some("Ada Example".into()),
            ical_uid: None,
            join_url: None,
            thread_id: Some("c0".into()),
            response: chatsvc::ResponseType::NotResponded,
        }],
    });
    s.apply(Update::Activity(teams_core::ActivitySnapshot {
        items: vec![teams_core::ActivityItem {
            message_id: "demo-mention".into(),
            source_thread_id: "c0".into(),
            source_user_name: "Ada Example".into(),
            preview: "Mentioned you in Project review".into(),
            activity_type: "mentionInChat".into(),
            version_ms: chrono::Utc::now().timestamp_millis(),
            ..Default::default()
        }],
        ..Default::default()
    }));
    let chats = (0..40)
        .map(|i| ChatRow {
            id: format!("c{i}"),
            title: format!("Chat {i} with a fairly long display name"),
            preview: "Ada: the preview is one line and gets truncated when it is too long".into(),
            unread: i % 5 == 0,
            when: format!("{:02}:{:02}", 9 + i % 12, i * 7 % 60),
        })
        .collect();
    s.apply(Update::Chats(chats));
    s.apply(Update::Teams(vec![TeamRow {
        id: "demo-team".into(),
        name: "Example team".into(),
        channels: vec![ChannelRow {
            hidden: false,
            id: "demo-channel".into(),
            name: "General".into(),
            unread: false,
        }],
    }]));
    s.selected = Some("c0".into());
    s.member_counts.insert("c0".into(), 5);
    s.pinned = Some(types::PinnedRow {
        conversation_id: "c0".into(),
        sender: "Ada Example".into(),
        text: "Rollout for Friday:".into(),
    });
    let rows = (0..60)
        .map(|i| MessageRow {
            sender_id: None,
            system: false,
            id: format!("m{i}"),
            reply_to_id: None,
            sender: if i % 3 == 0 { "Me".into() } else { "Ada Example".into() },
            when: format!("09:{:02}", i % 60),
            text: format!("Message {i}. Lorem ipsum dolor sit amet, consectetur adipiscing elit.\nSecond line."),
            mine: i % 3 == 0,
            media: Default::default(),
            reactions: vec![],
            deleted: false,
            can_delete: false,
        })
        .collect();
    s.apply(Update::Messages {
        conversation_id: "c0".into(),
        rows,
    });
    s
}
