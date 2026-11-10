//! Rusty Teams on Rune: chat list, conversation and composer on the real backend.
//! Flags: `--database <path>`, `--no-sso`, `--demo` (sample data, no backend).

mod bridge;
mod types;
mod ui;

use std::path::PathBuf;
use std::sync::OnceLock;

use bridge::Bridge;
use rune::*;
use types::{Actions, ChatRow, MessageRow, Update};
use ui::UiState;

const TITLE: &str = "Rusty Teams";
const SIZE: (f32, f32) = (1100.0, 700.0);

/// `init` takes no arguments and runs before `setup` hands out the waker, so both go through statics.
static WAKER: OnceLock<Waker> = OnceLock::new();

struct Arguments {
    database: Option<PathBuf>,
    no_sso: bool,
    demo: bool,
}

fn arguments() -> Arguments {
    let mut parsed = Arguments { database: None, no_sso: false, demo: false };
    let mut input = std::env::args().skip(1);
    while let Some(argument) = input.next() {
        match argument.as_str() {
            "--database" => parsed.database = input.next().map(PathBuf::from),
            "--no-sso" => parsed.no_sso = true,
            "--demo" => parsed.demo = true,
            _ => {}
        }
    }
    parsed
}

fn init() -> UiState<Bridge> {
    let arguments = arguments();
    let bridge = Bridge::start(arguments.database, arguments.no_sso, || {
        if let Some(wake) = WAKER.get() {
            wake();
        }
    });
    UiState::new(bridge)
}

fn setup(_: &mut UiState<Bridge>, wake: Waker) {
    let _ = WAKER.set(wake.clone());
    // Updates may have been queued before the waker existed.
    wake();
}

fn poll(s: &mut UiState<Bridge>, _now: f64, _: &mut Tx) -> bool {
    let updates = s.actions.poll();
    let changed = !updates.is_empty();
    for update in updates {
        s.apply(update);
    }
    changed
}

fn main() {
    if arguments().demo {
        App::new(AppSpec::new(TITLE, SIZE, demo_init, ui::view::<DemoActions>)).run();
        return;
    }
    App::new(AppSpec {
        setup: Some(setup),
        poll: Some(poll),
        ..AppSpec::new(TITLE, SIZE, init, ui::view::<Bridge>)
    })
    .run();
}

/// Demo only: fake actions and sample data, so the view can be looked at without a backend.
struct DemoActions;

impl Actions for DemoActions {
    fn open(&self, _conversation_id: &str) {}
    fn send(&self, _conversation_id: &str, _text: String) {}
}

fn demo_init() -> UiState<DemoActions> {
    let mut s = UiState::new(DemoActions);
    s.apply(Update::Status("Online (demo data)".into()));
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
    s.selected = Some("c0".into());
    let rows = (0..60)
        .map(|i| MessageRow {
            id: format!("m{i}"),
            sender: if i % 3 == 0 { "Me".into() } else { "Ada Example".into() },
            when: format!("09:{:02}", i % 60),
            text: format!("Message {i}. Lorem ipsum dolor sit amet, consectetur adipiscing elit.\nSecond line."),
            mine: i % 3 == 0,
        })
        .collect();
    s.apply(Update::Messages { conversation_id: "c0".into(), rows });
    s
}
