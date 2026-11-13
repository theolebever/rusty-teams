//! Meeting prejoin and active call controls.
use crate::avatar::rgb;
use crate::ui::text;
use calling::{
    CallCommand, CallState, CallUpdate, EngineEvent, IncomingRing, MuteCommand, RosterEntry,
    VideoKey, VideoPicture,
};
use rune::native_surface::{ByteFrame, ColorInfo, Format};
use rune::tok::Family;
use rune::*;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use app_call_model::end_screen::{end_tone, join_tone};
use app_call_model::model::{CallKind, CallModel};
use app_backend::notify::sounds::CallTone;
use app_call_model::record_rights::{RecordBlock, RecordFeature};
use app_call_model::recording_menu::{RecordAction, record_actions};
use app_call_model::stage_layout::{self as gallery, VisibleRotation as Rotation};
use crate::types::Actions;
use crate::ui::UiState;

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Layout {
    #[default]
    Gallery,
    Speaker,
    Content,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shortcut {
    Accept(bool),
    Decline,
    Mute,
    Camera,
    Hangup,
    Hand,
    Share,
}

pub fn shortcut<A: Actions>(s: &mut UiState<A>, action: Shortcut) -> bool {
    if s.read_only {
        return false;
    }
    let active = s.calls.state.as_ref().is_some_and(CallState::is_active);
    let available = match action {
        Shortcut::Accept(_) => s.calls.ring.is_some() && !active && !s.calls.pending,
        Shortcut::Decline => s.calls.ring.is_some(),
        _ => active,
    };
    if !available {
        return false;
    }
    if let Some(window) = &s.call_window {
        window.shortcut(action);
        return true;
    }
    match action {
        Shortcut::Accept(video) => {
            if let Some(ring) = &s.calls.ring {
                s.calls.pending = true;
                s.calls.video = video;
                s.actions.accept_call(ring.ring_id, video);
            }
        }
        Shortcut::Decline => {
            if let Some(ring) = s.calls.ring.take() {
                s.actions.decline_call(ring.ring_id);
            }
        }
        Shortcut::Mute => s
            .actions
            .call_command(CallCommand::Mute(MuteCommand::Toggle)),
        Shortcut::Camera => s
            .actions
            .call_command(CallCommand::SetCamera(!s.calls.camera)),
        Shortcut::Hangup => s.actions.call_command(CallCommand::Hangup),
        Shortcut::Hand => s.actions.call_command(CallCommand::SetHand(!s.calls.hand)),
        Shortcut::Share => {
            if s.calls.local_share.is_some() {
                s.actions.call_command(CallCommand::StopShare);
            } else {
                s.calls.show_shares = !s.calls.show_shares;
                s.calls.show_people = false;
                s.calls.show_devices = false;
                s.calls.show_captions = false;
                if s.calls.show_shares {
                    s.actions.call_command(CallCommand::RefreshShareSources);
                }
            }
        }
    }
    true
}

struct Preview {
    hub: Arc<calling::VideoHub>,
    sender: tokio::sync::mpsc::UnboundedSender<CallUpdate>,
    updates: tokio::sync::mpsc::UnboundedReceiver<CallUpdate>,
    settings: Arc<calling::blur::BlurSettings>,
    capture: Option<calling::capture::Capture>,
    camera: Option<calling::DeviceChoice>,
}

impl Preview {
    fn new() -> Self {
        let (sender, updates) = tokio::sync::mpsc::unbounded_channel();
        Self {
            hub: calling::VideoHub::new(sender.clone()),
            sender,
            updates,
            settings: Arc::new(calling::blur::BlurSettings::default()),
            capture: None,
            camera: None,
        }
    }

    fn start(&mut self, choice: &calling::DeviceChoice) {
        release(self.capture.take());
        self.camera = Some(choice.clone());
        let hub = self.hub.clone();
        let settings = self.settings.clone();
        let sender = self.sender.clone();
        match calling::camera::start_camera(
            choice,
            settings.clone(),
            move |buffer| {
                if let Some(mut picture) = calling::video_frame::picture_from_i420(buffer, 640) {
                    if settings.effects().mirror {
                        picture.flip_horizontally();
                    }
                    hub.publish(VideoKey::LocalCamera, picture);
                }
            },
            move |error| {
                let _ = sender.send(CallUpdate::Notice(error));
            },
        ) {
            Ok(capture) => self.capture = Some(capture),
            Err(error) => {
                let _ = self.sender.send(CallUpdate::Notice(error.to_string()));
            }
        }
    }
}

impl Drop for Preview {
    fn drop(&mut self) {
        release(self.capture.take());
    }
}

/// Lists audio devices and cameras off the UI thread.
pub(crate) fn list_devices(send: impl Fn(CallUpdate) + Send + 'static) {
    app_backend::runtime::handle().spawn_blocking(move || {
        send(CallUpdate::Devices(calling::devices::list_devices(&calling::devices::shared_factory())));
        send(CallUpdate::Cameras(calling::camera::list_cameras()));
    });
}

/// Dropping a capture joins its thread, so it happens off the UI thread.
fn release(capture: Option<calling::capture::Capture>) {
    if let Some(capture) = capture {
        app_backend::runtime::handle().spawn_blocking(move || drop(capture));
    }
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum HistoryFilter {
    #[default]
    All,
    Missed,
    Incoming,
    Outgoing,
}

impl HistoryFilter {
    fn includes(self, entry: &chatsvc::CallLogEntry) -> bool {
        match self {
            Self::All => true,
            Self::Missed => entry.missed(),
            Self::Incoming => entry.direction == chatsvc::CallDirection::Incoming,
            Self::Outgoing => entry.direction == chatsvc::CallDirection::Outgoing,
        }
    }
}

fn history_view<A: Actions + 'static>(s: &UiState<A>) -> El {
    let call = &s.calls;
    let mut contacts = col()
        .key("call-contacts")
        .w(260.0)
        .h_full()
        .padding(20.0)
        .gap(12.0)
        .fill(color("#FFFFFF"))
        .child(
            text("Make a call")
                .font(style(18.0, 600.0))
                .color(color("#242424")),
        )
        .child(
            text_input(&call.contact_search, style(14.0, 400.0))
                .key("call-contact-search")
                .label("Find a contact")
                .placeholder("Type a name", color("#616161"))
                .min_h(36.0)
                .w_full()
                .on_edit(|s: &mut UiState<A>| &mut s.calls.contact_search),
        )
        .child(button(
            "Join a meeting",
            "open-prejoin",
            |s: &mut UiState<A>, _| {
                s.calls.prejoin_open = true;
                if matches!(s.calls.state, Some(CallState::Ended { .. })) {
                    s.calls.state = None;
                }
            },
        ))
        .child(
            text("Contacts")
                .font(style(14.0, 600.0))
                .color(color("#242424")),
        );
    let query = call.contact_search.text.trim().to_lowercase();
    let mut people = col().w_full().gap(4.0);
    for chat in s
        .chats
        .iter()
        .filter(|chat| {
            s.details
                .get(&chat.id)
                .is_some_and(|d| d.members.len() == 2)
                && chat.title.to_lowercase().contains(&query)
        })
        .take(30)
    {
        let id = chat.id.clone();
        let mut person = row()
            .key(format!("call-contact-{}", chat.id))
            .w_full()
            .min_h(44.0)
            .gap(8.0)
            .center_y()
            .child(crate::avatar::chat(s, &chat.title, &chat.id, 32.0))
            .child(
                text(chat.title.clone())
                    .font(style(13.0, 400.0))
                    .color(color("#242424"))
                    .truncate()
                    .grow(1.0)
                    .min_w(0.0),
            );
        if !s.read_only {
            person = person.child(crate::view_messages::action_button(
                format!("contact-call-{}", chat.id),
                "call",
                move |s: &mut UiState<A>, _| s.actions.meet_now(&id),
            ));
        }
        people = people.child(person);
    }
    contacts = contacts.child(
        col()
            .grow(1.0)
            .min_h(0.0)
            .w_full()
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .child(people),
    );
    let mut filters = row().w_full().gap(8.0).center_y();
    for (label, filter) in [
        ("All", HistoryFilter::All),
        ("Missed", HistoryFilter::Missed),
        ("Incoming", HistoryFilter::Incoming),
        ("Outgoing", HistoryFilter::Outgoing),
    ] {
        filters = filters.child(
            button(
                label,
                &format!("history-{label}"),
                move |s: &mut UiState<A>, _| s.calls.history_filter = filter,
            )
            .fill(color(if call.history_filter == filter {
                "#E8EBFA"
            } else {
                "#FFFFFF"
            })),
        );
    }
    let mut list = col().w_full().gap(2.0);
    if call.history_loading {
        list = list.child(
            text("Loading call history…")
                .key("history-loading")
                .font(style(13.0, 400.0))
                .color(color("#616161")),
        );
    } else if let Some(error) = &call.history_error {
        list = list
            .child(
                text(error.clone())
                    .font(style(13.0, 400.0))
                    .color(color("#C4314B"))
                    .wrap(),
            )
            .child(button("Retry", "history-retry", |s: &mut UiState<A>, _| {
                s.calls.history_loading = true;
                s.actions.load_call_history();
            }));
    } else if !call
        .history
        .iter()
        .any(|entry| call.history_filter.includes(entry))
    {
        list = list.child(
            text("No calls to show")
                .key("history-empty")
                .font(style(14.0, 400.0))
                .color(color("#616161")),
        );
    }
    for entry in call
        .history
        .iter()
        .filter(|entry| call.history_filter.includes(entry))
    {
        let name = entry
            .peer_name
            .as_deref()
            .or(entry.peer_id.as_deref())
            .unwrap_or("Meeting");
        let label = if entry.missed() {
            "Missed call"
        } else if entry.direction == chatsvc::CallDirection::Outgoing {
            "Outgoing call"
        } else {
            "Incoming call"
        };
        let mut item = row()
            .key(format!("history-call-{}", entry.call_id))
            .w_full()
            .min_h(64.0)
            .px(12.0)
            .gap(12.0)
            .center_y()
            .hover(color("#EBEBEB"))
            .child(crate::avatar::person(
                s,
                name,
                entry.peer_id.as_deref(),
                36.0,
                None,
            ))
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .child(
                        text(name.to_owned())
                            .font(style(14.0, 600.0))
                            .color(color(if entry.missed() { "#C4314B" } else { "#242424" }))
                            .truncate(),
                    )
                    .child(text(label).font(style(12.0, 400.0)).color(color("#616161"))),
            )
            .child(
                text(
                    entry
                        .started_at
                        .with_timezone(&chrono::Local)
                        .format("%d/%m %H:%M")
                        .to_string(),
                )
                .font(style(12.0, 400.0))
                .color(color("#616161")),
            );
        if !s.read_only && !entry.is_meeting && entry.peer_id.is_some() {
            let entry = entry.clone();
            item = item.child(crate::view_messages::action_button(
                format!("history-call-back-{}", entry.call_id),
                "call",
                move |s: &mut UiState<A>, _| s.actions.call_back(&entry),
            ));
        }
        list = list.child(item);
    }
    let history = col()
        .key("call-history")
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .padding(24.0)
        .gap(16.0)
        .fill(color("#FFFFFF"))
        .child(
            text("Calls")
                .font(style(20.0, 600.0))
                .color(color("#242424")),
        )
        .child(
            text("History")
                .font(style(16.0, 600.0))
                .color(color("#242424")),
        )
        .child(filters)
        .child(
            col()
                .grow(1.0)
                .min_h(0.0)
                .w_full()
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(list),
        );
    row()
        .key("calls")
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .child(contacts)
        .child(el().w(1.0).h_full().fill(color("#E0E0E0")))
        .child(history)
}

/// How a call started, as GPUI's `CallModel` constructors (`people`, `meeting`, `incoming`)
/// record it: the kind, the chat it belongs to and the name to show when that chat is unknown.
#[derive(Debug, Clone, PartialEq)]
pub struct CallIntro {
    pub kind: CallKind,
    pub conversation: Option<String>,
    pub name: String,
}

impl CallIntro {
    pub fn of(spec: &calling::CallSpec) -> Self {
        match spec {
            calling::CallSpec::Echo => Self { kind: CallKind::Test, conversation: None, name: "Test call".into() },
            calling::CallSpec::People { callees, thread_id } => Self {
                kind: if callees.len() == 1 { CallKind::Direct } else { CallKind::Group },
                conversation: Some(thread_id.clone()),
                name: match callees.as_slice() {
                    [callee] => callee.display_name.clone(),
                    _ => "Call".into(),
                },
            },
            calling::CallSpec::Meeting(target) => Self {
                kind: CallKind::Meeting,
                conversation: Some(target.thread_id.clone()),
                name: "Meeting".into(),
            },
        }
    }

    /// An answered ring is a direct call in GPUI (`CallModel::incoming`), even from a group.
    fn ring(ring: &IncomingRing) -> Self {
        Self { kind: CallKind::Direct, conversation: ring.thread_id.clone(), name: ring.caller.display_name.clone() }
    }
}

/// GPUI's `LOBBY_TONE_THROTTLE`: one lobby chime per half minute.
const LOBBY_TONE_THROTTLE: Duration = Duration::from_secs(30);

pub struct CallView {
    pub prejoin_open: bool,
    pub history: Vec<chatsvc::CallLogEntry>,
    pub history_loading: bool,
    pub history_error: Option<String>,
    pub history_filter: HistoryFilter,
    pub contact_search: TextEdit,
    pub link: TextEdit,
    pub video: bool,
    pub pending: bool,
    pub state: Option<CallState>,
    pub muted: bool,
    pub camera: bool,
    pub roster: Vec<RosterEntry>,
    pub notice: Option<String>,
    pub ring: Option<IncomingRing>,
    pub retry: Option<Arc<dyn Fn() + Send + Sync>>,
    pub detached: bool,
    lobby: bool,
    progress: Option<calling::Progress>,
    speaking: Vec<String>,
    local_level: f32,
    health: Option<calling::CallHealth>,
    show_health: bool,
    pub(crate) selected_input: calling::DeviceChoice,
    pub(crate) selected_output: calling::DeviceChoice,
    pub(crate) selected_camera: calling::DeviceChoice,
    preview: Option<Preview>,
    prejoin_mic: bool,
    pub(crate) background_blur: bool,
    pub(crate) effects: calling::CameraEffects,
    rotation: Rotation,
    page: usize,
    layout: Layout,
    pinned: Option<String>,
    tile_menu: Option<String>,
    stage_size: (f32, f32),
    video_wants: Option<Vec<calling::VideoWant>>,
    screen_share: Option<String>,
    surfaces: HashMap<VideoKey, NativeSurface>,
    frame_id: u64,
    pub(crate) devices: calling::DeviceLists,
    pub(crate) cameras: Vec<calling::CameraDevice>,
    shares: Vec<calling::ShareSource>,
    local_share: Option<String>,
    show_people: bool,
    invite_open: bool,
    confirm: Option<CallCommand>,
    show_reactions: bool,
    show_devices: bool,
    show_shares: bool,
    hand: bool,
    captions: calling::CaptionState,
    caption: Option<calling::CaptionEntry>,
    caption_history: std::collections::VecDeque<calling::CaptionEntry>,
    show_captions: bool,
    caption_language: Option<String>,
    recording: bool,
    recording_pending: bool,
    recording_audio_only: bool,
    transcribing: bool,
    consent_required: bool,
    capability: calling::MeetingCapability,
    /// The meeting's chat thread (`CallUpdate::MeetingChat`), shown beside the stage.
    meeting_chat: Option<String>,
    show_chat: bool,
    reactions: Vec<(u64, String, calling::Reaction, Instant)>,
    own_mri: String,
    /// How the current (or last) call started; `None` before the first one.
    pub(crate) intro: Option<CallIntro>,
    /// `start` ran for the call now connecting; without it, the call is the answered ring.
    intro_fresh: bool,
    /// One-shot tones (joined, ended, lobby) for `notifications::ring` to play.
    pub(crate) tones: Vec<CallTone>,
    lobby_tone_at: Option<Instant>,
    recording_policy: chatsvc::RecordingPolicy,
    own_tenant: Option<String>,
    meeting_tenant: Option<String>,
}

impl Default for CallView {
    fn default() -> Self {
        Self {
            prejoin_open: false,
            history: Vec::new(),
            history_loading: false,
            history_error: None,
            history_filter: HistoryFilter::All,
            contact_search: TextEdit::default(),
            link: TextEdit::new(""),
            video: false,
            pending: false,
            state: None,
            muted: true,
            camera: false,
            roster: Vec::new(),
            notice: None,
            ring: None,
            retry: None,
            detached: false,
            lobby: false,
            progress: None,
            speaking: Vec::new(),
            local_level: 0.0,
            health: None,
            show_health: false,
            selected_input: calling::DeviceChoice::default(),
            selected_output: calling::DeviceChoice::default(),
            selected_camera: calling::DeviceChoice::default(),
            preview: None,
            prejoin_mic: false,
            background_blur: false,
            effects: calling::CameraEffects::default(),
            rotation: Rotation::default(),
            page: 0,
            layout: Layout::default(),
            pinned: None,
            tile_menu: None,
            stage_size: (900.0, 560.0),
            video_wants: None,
            screen_share: None,
            surfaces: HashMap::new(),
            frame_id: 0,
            devices: calling::DeviceLists::default(),
            cameras: Vec::new(),
            shares: Vec::new(),
            local_share: None,
            show_people: false,
            invite_open: false,
            confirm: None,
            show_reactions: false,
            show_devices: false,
            show_shares: false,
            hand: false,
            captions: calling::CaptionState::Off,
            caption: None,
            caption_history: std::collections::VecDeque::new(),
            show_captions: false,
            caption_language: None,
            recording: false,
            recording_pending: false,
            recording_audio_only: false,
            transcribing: false,
            consent_required: false,
            capability: calling::MeetingCapability::default(),
            meeting_chat: None,
            show_chat: false,
            reactions: Vec::new(),
            own_mri: String::new(),
            intro: None,
            intro_fresh: false,
            tones: Vec::new(),
            lobby_tone_at: None,
            recording_policy: chatsvc::RecordingPolicy::default(),
            own_tenant: None,
            meeting_tenant: None,
        }
    }
}

impl CallView {
    fn stop_preview(&mut self) {
        self.preview = None;
        self.surfaces.remove(&VideoKey::LocalCamera);
    }

    fn prejoin_commands(&self) -> Vec<CallCommand> {
        vec![
            CallCommand::SelectInput(self.selected_input.clone()),
            CallCommand::SelectOutput(self.selected_output.clone()),
            CallCommand::SelectCamera(self.selected_camera.clone()),
            CallCommand::SetBackground(if self.background_blur {
                calling::BackgroundChoice::Blur
            } else {
                calling::BackgroundChoice::None
            }),
            CallCommand::SetCameraEffects(self.effects),
            CallCommand::Mute(if self.prejoin_mic {
                MuteCommand::Unmute
            } else {
                MuteCommand::Mute
            }),
        ]
    }

    /// Records how a call starts; joining a meeting chimes as in GPUI (`join_tone`).
    pub fn start(&mut self, intro: CallIntro) {
        self.tones.extend(join_tone(intro.kind));
        self.intro = Some(intro);
        self.intro_fresh = true;
    }

    pub fn recording_rights(&mut self, policy: Option<chatsvc::RecordingPolicy>, own_tenant: Option<String>) {
        if let Some(policy) = policy { self.recording_policy = policy; }
        if own_tenant.is_some() { self.own_tenant = own_tenant; }
    }

    /// GPUI's `chat_thread`: a meeting's chat, else the chat of the people called.
    fn chat_thread(&self) -> Option<&str> {
        let conversation = self.intro.as_ref().and_then(|intro| intro.conversation.as_deref());
        app_call_model::model::chat_thread(self.kind(), self.meeting_chat.as_deref(), conversation)
    }

    fn kind(&self) -> CallKind {
        self.intro.as_ref().map_or(CallKind::Meeting, |intro| intro.kind)
    }

    pub fn apply_ring(&mut self, event: EngineEvent) {
        match event {
            EngineEvent::Incoming(ring) => self.ring = Some(ring),
            EngineEvent::RingEnded { ring_id, .. } => {
                if self
                    .ring
                    .as_ref()
                    .is_some_and(|ring| ring.ring_id == ring_id)
                {
                    self.ring = None;
                    self.pending = false;
                }
            }
        }
    }

    pub fn apply_video(&mut self, frames: Vec<(VideoKey, Arc<VideoPicture>)>) {
        for (key, picture) in frames {
            if let VideoKey::Person(mri) = &key
                && self
                    .video_wants
                    .as_ref()
                    .is_some_and(|wants| !wants.iter().any(|want| &want.mri == mri))
            {
                continue;
            }
            let surface = self.surfaces.entry(key).or_default();
            self.frame_id = self.frame_id.wrapping_add(1);
            let _ = surface.push_bytes(
                ByteFrame {
                    width: picture.width,
                    height: picture.height,
                    format: Format::Bgra,
                    color: ColorInfo::default(),
                    planes: [picture.bgra.clone(), Vec::new()],
                },
                self.frame_id,
            );
        }
    }

    pub fn apply(&mut self, update: CallUpdate) {
        let layout_changed = matches!(
            &update,
            CallUpdate::State(_)
                | CallUpdate::Roster(_)
                | CallUpdate::Speakers(_)
                | CallUpdate::OwnIdentity { .. }
        );
        match update {
            CallUpdate::State(state) => {
                self.pending = false;
                if matches!(state, CallState::Connecting) {
                    if !std::mem::take(&mut self.intro_fresh) && let Some(ring) = &self.ring {
                        self.intro = Some(CallIntro::ring(ring));
                    }
                    self.stop_preview();
                    self.lobby = false;
                    self.progress = None;
                    self.speaking.clear();
                    self.local_level = 0.0;
                    self.health = None;
                    self.show_health = false;
                    self.rotation = Rotation::default();
                    self.page = 0;
                    self.layout = Layout::Gallery;
                    self.pinned = None;
                    self.tile_menu = None;
                    self.video_wants = None;
                    self.screen_share = None;
                    self.retry = None;
                    self.roster.clear();
                    self.show_people = false;
                    self.invite_open = false;
                    self.confirm = None;
                    self.show_devices = false;
                    self.show_shares = false;
                    self.show_reactions = false;
                    self.notice = None;
                    self.muted = true;
                    self.camera = self.video;
                    self.ring = None;
                    self.surfaces.clear();
                    self.hand = false;
                    self.captions = calling::CaptionState::Off;
                    self.caption = None;
                    self.caption_history.clear();
                    self.show_captions = false;
                    self.caption_language = None;
                    self.recording = false;
                    self.recording_pending = false;
                    self.recording_audio_only = false;
                    self.transcribing = false;
                    self.consent_required = false;
                    self.capability = calling::MeetingCapability::default();
                    self.meeting_chat = None;
                    self.show_chat = false;
                    self.reactions.clear();
                    self.local_share = None;
                    self.own_mri.clear();
                    self.meeting_tenant = None;
                }
                if matches!(state, CallState::Ended { .. }) {
                    self.surfaces.clear();
                    if self.state.as_ref().is_some_and(|old| !matches!(old, CallState::Ended { .. })) {
                        self.tones.extend(end_tone(self.kind(), false));
                    }
                }
                self.state = Some(state);
            }
            CallUpdate::Muted(muted) => self.muted = muted,
            CallUpdate::Lobby(lobby) => self.lobby = lobby,
            CallUpdate::Progress(progress) => self.progress = Some(progress),
            CallUpdate::Speakers(speaking) => self.speaking = speaking,
            CallUpdate::Levels { local, .. } => self.local_level = local,
            CallUpdate::Health(health) => self.health = Some(health),
            CallUpdate::Selected { input, output } => {
                self.selected_input = input;
                self.selected_output = output;
            }
            CallUpdate::Camera(camera) => {
                self.camera = camera;
                if !camera {
                    self.surfaces.remove(&VideoKey::LocalCamera);
                }
            }
            CallUpdate::Roster(roster) => {
                self.surfaces.retain(|key, _| match key {
                    VideoKey::Person(id) => roster
                        .iter()
                        .any(|person| person.mri == *id && person.has_video),
                    _ => true,
                });
                let lobby_before: Vec<String> = self.lobby_guests().map(|p| p.mri.clone()).collect();
                self.roster = roster;
                // GPUI's lobby chime: someone new waits and we may admit them.
                let admit = self.can_manage() && !self.lobby && matches!(self.state, Some(CallState::Connected { .. } | CallState::Reconnecting { .. }));
                if admit
                    && self.lobby_guests().any(|p| !lobby_before.contains(&p.mri))
                    && self.lobby_tone_at.is_none_or(|at| at.elapsed() >= LOBBY_TONE_THROTTLE)
                {
                    self.lobby_tone_at = Some(Instant::now());
                    self.tones.push(CallTone::MeetingReminder);
                }
                if self
                    .pinned
                    .as_ref()
                    .is_some_and(|mri| !self.roster.iter().any(|p| &p.mri == mri && !p.in_lobby))
                {
                    self.pinned = None;
                }
            }
            CallUpdate::VideoStopped(key) => {
                self.surfaces.remove(&key);
            }
            CallUpdate::ScreenShare(sharer) => {
                if sharer.is_none() {
                    self.surfaces.remove(&VideoKey::Screen);
                }
                self.screen_share = sharer;
            }
            CallUpdate::Devices(devices) => self.devices = devices,
            CallUpdate::Cameras(cameras) => self.cameras = cameras,
            CallUpdate::ShareSources(shares) => self.shares = shares,
            CallUpdate::OwnHand(hand) => self.hand = hand.is_some(),
            CallUpdate::Captions(state) => {
                if state != calling::CaptionState::On {
                    self.caption = None;
                }
                self.captions = state;
            }
            CallUpdate::Caption(caption) if self.captions != calling::CaptionState::Off => {
                if let Some(line) = self
                    .caption_history
                    .iter_mut()
                    .rev()
                    .take(16)
                    .find(|line| line.id == caption.id && line.user_id == caption.user_id)
                {
                    *line = caption.clone();
                } else {
                    // shortcut: keep the last 200 utterances in memory; add persistent history only if requested.
                    if self.caption_history.len() == 200 {
                        self.caption_history.pop_front();
                    }
                    self.caption_history.push_back(caption.clone());
                }
                self.caption = Some(caption);
            }
            CallUpdate::Recording(recording) => self.recording = recording,
            CallUpdate::RecordingPending(pending) => self.recording_pending = pending,
            CallUpdate::RecordingAudioOnly(audio) => self.recording_audio_only = audio,
            CallUpdate::Transcribing(transcribing) => self.transcribing = transcribing,
            CallUpdate::ConsentRequired(required) => self.consent_required = required,
            CallUpdate::Capability(capability) => self.capability = capability,
            CallUpdate::MeetingChat(thread) => self.meeting_chat = Some(thread),
            CallUpdate::MeetingTenant(tenant) => self.meeting_tenant = Some(tenant),
            CallUpdate::Reaction { mri, reaction } => {
                self.frame_id = self.frame_id.wrapping_add(1);
                if self.reactions.len() == 12 {
                    self.reactions.remove(0);
                }
                self.reactions
                    .push((self.frame_id, mri, reaction, Instant::now()));
            }
            CallUpdate::OwnIdentity { mri } => self.own_mri = mri,
            CallUpdate::LocalShare(share) => {
                if share.is_none() {
                    self.surfaces.remove(&VideoKey::LocalScreen);
                }
                self.local_share = share;
            }
            CallUpdate::Notice(notice) => {
                self.pending = false;
                self.notice = Some(notice);
            }
            CallUpdate::Organizer { action, outcome } => {
                self.notice = match outcome {
                    Ok(status) if (200..300).contains(&status) => None,
                    Ok(_) => Some(format!("Could not {action}. Try again.")),
                    Err(error) => Some(format!("Could not {action}: {error}")),
                };
            }
            _ => {}
        }
        if layout_changed {
            self.refresh_gallery(Instant::now());
        }
    }

    /// What GPUI's record rights (`app_call_model::record_rights`) see of this call.
    fn record_model(&self) -> CallModel {
        let mut model = CallModel::meeting("", false);
        model.kind = self.kind();
        model.recording_policy = self.recording_policy;
        model.own_tenant.clone_from(&self.own_tenant);
        model.meeting_tenant.clone_from(&self.meeting_tenant);
        if let Some(state) = &self.state { model.state = state.clone(); }
        let own = self.roster.iter().find(|person| person.mri == self.own_mri);
        model.organizer = own.is_some_and(|person| person.organizer);
        model.own_role = own.and_then(|person| person.role);
        model.lobby = self.lobby;
        model.recording = self.recording;
        model.transcribing = self.transcribing;
        model.record_pending = self.recording_pending;
        model.capability = self.capability;
        model
    }

    fn lobby_guests(&self) -> impl Iterator<Item = &RosterEntry> {
        self.roster.iter().filter(|p| p.in_lobby)
    }

    fn can_manage(&self) -> bool {
        self.roster.iter().any(|person| {
            person.mri == self.own_mri
                && (person.organizer || person.role == Some(calling::MeetingRole::Organizer))
        })
    }

    fn has_content(&self) -> bool {
        self.screen_share.is_some()
            || self.local_share.is_some()
            || self.surfaces.contains_key(&VideoKey::Screen)
            || self.surfaces.contains_key(&VideoKey::LocalScreen)
    }

    fn refresh_gallery(&mut self, now: Instant) {
        let members: Vec<_> = self
            .roster
            .iter()
            .filter(|p| !p.in_lobby && p.mri != self.own_mri && !p.mri.starts_with("28:"))
            .map(|p| p.mri.clone())
            .collect();
        self.rotation
            .update(now, &members, &[], &self.speaking, gallery::GALLERY_MAX);
        self.page = self.page.min(gallery::gallery_pages(members.len(), 0) - 1);
    }

    fn ordered_people(&self) -> Vec<&RosterEntry> {
        let mut people: Vec<_> = self
            .rotation
            .order()
            .iter()
            .filter_map(|mri| self.roster.iter().find(|p| &p.mri == mri))
            .collect();
        people.sort_by_key(|p| {
            (
                self.pinned.as_deref() != Some(p.mri.as_str()),
                p.spotlight.as_ref().map(|s| s.rank).unwrap_or(u64::MAX),
            )
        });
        people
    }

    fn focus<'a>(&self, people: &[&'a RosterEntry]) -> Option<&'a RosterEntry> {
        people
            .iter()
            .copied()
            .find(|p| self.pinned.as_deref() == Some(p.mri.as_str()) || p.spotlight.is_some())
            .or_else(|| {
                people
                    .iter()
                    .copied()
                    .find(|p| self.speaking.contains(&p.mri))
            })
            .or_else(|| people.first().copied())
    }

    fn visible_people(&self) -> Vec<(&RosterEntry, calling::TileDetail)> {
        use calling::TileDetail::{Small, Stage};
        let people = self.ordered_people();
        if self.has_content() {
            return if self.layout == Layout::Content {
                Vec::new()
            } else {
                people
                    .into_iter()
                    .take(gallery::GALLERY_MAX)
                    .map(|p| (p, Small))
                    .collect()
            };
        }
        if self.layout == Layout::Speaker
            || self.pinned.is_some()
            || people.iter().any(|p| p.spotlight.is_some())
        {
            let focus = self.focus(&people);
            return focus
                .into_iter()
                .map(|p| (p, Stage))
                .chain(
                    people
                        .into_iter()
                        .filter(|p| focus.is_none_or(|f| f.mri != p.mri))
                        .take(gallery::GALLERY_MAX - 1)
                        .map(|p| (p, Small)),
                )
                .collect();
        }
        let range = gallery::gallery_page_range(people.len(), 0, self.page);
        let detail = if range.len() <= 4 { Stage } else { Small };
        people[range].iter().map(|p| (*p, detail)).collect()
    }

    fn wants(&self) -> Vec<calling::VideoWant> {
        self.visible_people()
            .into_iter()
            .filter(|(p, _)| p.has_video)
            .take(calling::video_layout::RECEIVE_SLOTS)
            .map(|(p, detail)| calling::VideoWant {
                mri: p.mri.clone(),
                detail,
            })
            .collect()
    }

    /// The looping call tone, as GPUI's `sync_call_sounds`: ringback while an outgoing call
    /// rings (only people calls report progress), reconnecting outside the lobby.
    pub(crate) fn tone(&self) -> Option<app_backend::notify::sounds::CallTone> {
        use app_backend::notify::sounds::CallTone;
        match self.state {
            Some(CallState::Connecting) if self.progress.is_some() => Some(CallTone::OutgoingRinging),
            Some(CallState::Reconnecting { .. }) if !self.lobby => Some(CallTone::Reconnecting),
            _ => None,
        }
    }

    fn status(&self, now: Instant) -> String {
        if self.lobby {
            return "Waiting in the lobby...".into();
        }
        match &self.state {
            Some(CallState::Connecting) => match self.progress {
                Some(calling::Progress::Ringing) => "Ringing",
                Some(calling::Progress::Calling) => "Calling...",
                None => "Connecting...",
            }
            .into(),
            Some(CallState::Reconnecting { .. }) => "Reconnecting...".into(),
            Some(CallState::Connected { since }) => {
                app_call_model::model::format_elapsed(now.saturating_duration_since(*since))
            }
            _ => "Call".into(),
        }
    }
}

pub fn poll<A: Actions>(state: &mut UiState<A>, _: f64, tx: &mut Tx) -> bool {
    let active = state.calls.state.as_ref().is_some_and(CallState::is_active);
    let prejoin = !active
        && matches!(state.calls.state, None | Some(CallState::Idle))
        && state.calls.ring.is_none()
        && !state.calls.pending
        && !state.read_only
        && state.screens.screen == crate::screens::Screen::Calls
        && (state.calls.prejoin_open || !state.calls.link.text.is_empty());
    let mut changed = false;
    if prejoin {
        if state.calls.preview.is_none() {
            let preview = Preview::new();
            let sender = preview.sender.clone();
            list_devices(move |update| { let _ = sender.send(update); });
            state.calls.preview = Some(preview);
        }
        let call = &mut state.calls;
        let preview = call.preview.as_mut().expect("prejoin preview");
        preview.settings.set_enabled(call.background_blur);
        preview.settings.set_effects(call.effects);
        if call.video && preview.camera.as_ref() != Some(&call.selected_camera) {
            preview.start(&call.selected_camera);
        } else if !call.video {
            release(preview.capture.take());
            preview.camera = None;
            call.surfaces.remove(&VideoKey::LocalCamera);
        }
        let mut updates = Vec::new();
        while let Ok(update) = preview.updates.try_recv() {
            updates.push(update);
        }
        let frames = preview.hub.drain();
        changed = !updates.is_empty() || !frames.is_empty();
        for update in updates {
            call.apply(update);
        }
        call.apply_video(frames);
        tx.wake_after(Duration::from_millis(33));
    } else if state.calls.preview.is_some() {
        state.calls.stop_preview();
        changed = true;
    }
    let running = active && state.call_window.is_none();
    if running {
        let now = Instant::now();
        state
            .calls
            .reactions
            .retain(|(_, _, _, at)| now.saturating_duration_since(*at) < Duration::from_secs(4));
        state.calls.refresh_gallery(now);
        sync_video(state);
        tx.wake_after(if state.calls.reactions.is_empty() {
            Duration::from_secs(1)
        } else {
            Duration::from_millis(33)
        });
    }
    running || changed
}

fn sync_video<A: Actions>(state: &mut UiState<A>) {
    let wants = state.calls.wants();
    if state.calls.video_wants.as_ref() != Some(&wants) {
        state.calls.surfaces.retain(|key, _| match key {
            VideoKey::Person(mri) => wants.iter().any(|want| &want.mri == mri),
            _ => true,
        });
        state
            .actions
            .call_command(CallCommand::SetVideoWants(wants.clone()));
        state.calls.video_wants = Some(wants);
    }
}

fn end_reason(reason: &calling::EndReason) -> String {
    use calling::{EndKind, EndReason};
    match reason {
        EndReason::LocalHangup => "You left the call".into(),
        EndReason::Cancelled | EndReason::Remote(EndKind::Cancelled) => "Call cancelled".into(),
        EndReason::Dropped | EndReason::Remote(EndKind::MediaError) => {
            "Call dropped. Check your internet connection and try again.".into()
        }
        EndReason::Failed(message) => format!("Could not connect: {message}"),
        EndReason::Remote(EndKind::Declined) => "Call declined".into(),
        EndReason::Remote(EndKind::NoAnswer) => "No answer".into(),
        EndReason::Remote(EndKind::Unavailable) => "This person is not available".into(),
        EndReason::Remote(EndKind::AnsweredElsewhere) => "Call answered on another device".into(),
        EndReason::Remote(EndKind::Forwarded) => "Call forwarded".into(),
        EndReason::Remote(EndKind::RoomClosed) => "The meeting has ended".into(),
        EndReason::Remote(EndKind::Removed) => {
            "Someone just removed you from the meeting. You can try to rejoin.".into()
        }
        EndReason::Remote(EndKind::Normal | EndKind::Other) => "Call ended".into(),
    }
}

fn color(hex: &str) -> Color {
    crate::settings::color(hex)
}
fn style(size: f32, weight: f32) -> TextStyle {
    TextStyle::new(size, weight).family(Family::Named("Segoe UI"))
}

fn record_key(label: &str) -> String {
    format!("call-record-{}", label.to_lowercase().replace(' ', "-"))
}

/// A record action the call does not allow, greyed with GPUI's reason (`RecordBlock::reason`).
fn blocked_record(label: &str, (block, feature): (RecordBlock, RecordFeature)) -> El {
    // ponytail: no lock glyph (not in `icons::SOURCES`); the grey and the reason say it.
    col()
        .key(format!("{}-blocked", record_key(label)))
        .label(format!("{label}: {}", block.reason(feature)))
        .max_w(236.0)
        .px(12.0)
        .py(4.0)
        .radius(6.0)
        .fill(color("#3D3D3D"))
        .child(text(label.to_owned()).font(style(13.0, 600.0)).color(color("#A0A0A0")))
        .child(text(block.reason(feature)).font(style(11.0, 400.0)).color(color("#A0A0A0")).wrap())
}

/// The recording's file name, GPUI's call title: the call's chat, else whom we called.
fn recording_title<A: Actions>(s: &UiState<A>) -> String {
    let intro = s.calls.intro.as_ref();
    intro
        .and_then(|intro| intro.conversation.as_deref())
        .and_then(|id| s.chats.iter().find(|chat| chat.id == id))
        .map(|chat| chat.title.clone())
        .filter(|title| !title.trim().is_empty())
        .or_else(|| intro.map(|intro| intro.name.clone()))
        .unwrap_or_else(|| "Meeting".into())
}

fn button<A: Actions + 'static>(
    label: &str,
    key: &str,
    action: impl Fn(&mut UiState<A>, &mut Tx) + 'static,
) -> El {
    let symbol = match key {
        "call-mute" => Some(if label == "Unmute" { "mic_off" } else { "mic" }),
        "call-camera" => Some("videocam"),
        "call-devices" => Some("settings"),
        "call-share" | "stop-share" => Some("share"),
        "call-people" => Some("group"),
        "call-more" => Some("more_horiz"),
        "call-chat" => Some("chat_bubble"),
        _ => None,
    };
    if let Some(symbol) = symbol {
        return col()
            .key(key.to_owned())
            .label(label.to_owned())
            .w(64.0)
            .h(60.0)
            .gap(4.0)
            .radius(4.0)
            .center()
            .hover(color("#424242"))
            .on_press_tx(action)
            .child(crate::icons::sym(symbol, 22.0).color(rgb(0xFFFFFF)))
            .child(
                text(label.to_owned())
                    .font(style(11.0, 400.0))
                    .color(rgb(0xFFFFFF)),
            );
    }
    row()
        .key(key.to_owned())
        .h(36.0)
        .px(16.0)
        .radius(6.0)
        .center()
        .fill(color(if key == "call-leave" {
            "#C4314B"
        } else {
            "#E8EBFA"
        }))
        .hover(color(if key == "call-leave" {
            "#A4262C"
        } else {
            "#DADDF5"
        }))
        .on_press_tx(action)
        .child(
            text(label.to_owned())
                .font(style(13.0, 600.0))
                .color(if key == "call-leave" {
                    rgb(0xFFFFFF)
                } else {
                    color("#242424")
                }),
        )
}

fn health_indicator<A: Actions + 'static>(call: &CallView) -> El {
    let Some(health) = &call.health else {
        return el();
    };
    let (lit, tint) = match health.level {
        calling::NetworkLevel::Good => (3, "#FFFFFF"),
        calling::NetworkLevel::Poor => (2, "#FFAA44"),
        calling::NetworkLevel::Bad => (1, "#FFB3BE"),
    };
    let label = format!(
        "Network: {}",
        app_call_model::health::level_text(health.level)
    );
    let mut bars = row()
        .key("call-health")
        .label(label)
        .gap(2.0)
        .h(18.0)
        .center_y()
        .on_press(|s: &mut UiState<A>| s.calls.show_health = !s.calls.show_health);
    for (index, height) in [6.0, 10.0, 14.0].into_iter().enumerate() {
        bars = bars.child(
            el().w(3.0)
                .h(height)
                .radius(1.0)
                .fill(color(if index < lit { tint } else { "#616161" })),
        );
    }
    bars
}

fn device_panel<A: Actions + 'static>(call: &CallView, prejoin: bool) -> El {
    let mut panel = col().key("call-device-list").gap(4.0);
    use crate::settings::Device;
    for (device, kind, title, entries, selected) in [
        (Device::Microphone, "input", "Microphone", call.devices.input_entries(), &call.selected_input),
        (Device::Speaker, "output", "Speaker", call.devices.output_entries(), &call.selected_output),
        (Device::Camera, "camera", "Camera", calling::camera_entries(&call.cameras), &call.selected_camera),
    ] {
        panel = panel.child(text(title).font(style(13.0, 700.0)).color(color("#242424")));
        for entry in entries {
            let choice = entry.choice;
            let label = format!(
                "{}{}",
                if &choice == selected { "✓ " } else { "" },
                entry.label
            );
            panel = panel.child(button::<A>(
                &label,
                &format!("{kind}-{}", entry.label),
                move |s, _| {
                    let command = match device {
                        Device::Microphone => CallCommand::SelectInput(choice.clone()),
                        Device::Speaker => CallCommand::SelectOutput(choice.clone()),
                        Device::Camera => CallCommand::SelectCamera(choice.clone()),
                    };
                    if prejoin {
                        match device {
                            Device::Microphone => s.calls.selected_input = choice.clone(),
                            Device::Speaker => s.calls.selected_output = choice.clone(),
                            Device::Camera => s.calls.selected_camera = choice.clone(),
                        }
                    } else {
                        s.actions.call_command(command);
                        // shortcut: the engine has no selected-camera update; confirm it when that event exists.
                        if device == Device::Camera {
                            s.calls.selected_camera = choice.clone();
                        }
                    }
                },
            ));
        }
    }
    panel
}

fn effects_panel<A: Actions + 'static>(call: &CallView, prejoin: bool) -> El {
    let mut panel = col().key("call-effects").gap(4.0).child(button::<A>(
        if call.background_blur {
            "Background blur on"
        } else {
            "Background blur off"
        },
        "call-blur",
        move |s, _| {
            s.calls.background_blur = !s.calls.background_blur;
            if !prejoin {
                s.actions
                    .call_command(CallCommand::SetBackground(if s.calls.background_blur {
                        calling::BackgroundChoice::Blur
                    } else {
                        calling::BackgroundChoice::None
                    }));
            }
        },
    ));
    type Field = fn(&mut calling::CameraEffects) -> &mut bool;
    let effects: [(&str, &str, bool, Field); 4] = [
        ("mirror", "Mirror my video", call.effects.mirror, |e| &mut e.mirror),
        ("framing", "Auto framing", call.effects.auto_framing, |e| &mut e.auto_framing),
        ("brightness", "Adjust brightness", call.effects.adjust_brightness, |e| &mut e.adjust_brightness),
        ("soft-focus", "Soft focus", call.effects.soft_focus, |e| &mut e.soft_focus),
    ];
    for (key, label, enabled, field) in effects {
        panel = panel.child(button::<A>(
            &format!("{} {label}", if enabled { "✓" } else { "" }),
            &format!("effect-{key}"),
            move |s, _| {
                *field(&mut s.calls.effects) = !enabled;
                if !prejoin {
                    s.actions
                        .call_command(CallCommand::SetCameraEffects(s.calls.effects));
                }
            },
        ));
    }
    panel
}

fn invite_draft<A: Actions>(s: &mut UiState<A>) -> &mut crate::new_chat::Draft {
    &mut s.chatlist.manage.add
}

/// The chat emoji of a call reaction; Teams has no chat emotion named "applause".
fn reaction_glyph(reaction: &calling::Reaction) -> String {
    match reaction {
        calling::Reaction::Applause => "👏".to_owned(),
        other => chatsvc::reactions::reaction_glyph_for_key(other.name()).unwrap_or_else(|| other.name().to_owned()),
    }
}

fn participant_tile<A: Actions + 'static>(
    call: &CallView,
    person: &RosterEntry,
    (width, height): (f32, f32),
) -> El {
    let own = person.mri == call.own_mri;
    let name = if own {
        "You"
    } else if person.display_name.is_empty() {
        &person.mri
    } else {
        &person.display_name
    };
    let key = if own {
        "call-self".into()
    } else {
        format!("call-person-{}", person.mri)
    };
    let video = if own {
        VideoKey::LocalCamera
    } else {
        VideoKey::Person(person.mri.clone())
    };
    let surface = call.surfaces.get(&video);
    let speaking = !person.muted
        && if own {
            call.local_level > 0.02
        } else {
            call.speaking.contains(&person.mri)
        };
    let compact = height < 200.0;
    let inset = if compact { 4.0 } else { 8.0 };
    let mut tile = col()
        .key(key.clone())
        .size(width, height)
        .min_w(0.0)
        .min_h(0.0)
        .radius(10.0)
        .fill(color("#333333"))
        .border(2.0, color(if speaking { "#9299F7" } else { "#333333" }));
    let picture = if let Some(surface) = surface {
        el().w_full()
            .h_full()
            .min_h(0.0)
            .native_surface(surface, SurfaceFit::Fit)
    } else {
        col().w_full().grow(1.0).center().child(
            row()
                .w(72.0)
                .h(72.0)
                .radius(36.0)
                .center()
                .fill(color("#5B5FC7"))
                .child(
                    text(
                        name.split_whitespace()
                            .take(2)
                            .filter_map(|word| word.chars().next())
                            .collect::<String>(),
                    )
                    .font(style(26.0, 600.0))
                    .color(rgb(0xFFFFFF)),
                ),
        )
    };
    let mut plate = row()
        .key(format!("{key}-name"))
        .pin(Some(inset), None, None, Some(inset))
        .max_w(if compact { width * 0.6 } else { width - 16.0 })
        .px(6.0)
        .py(2.0)
        .gap(4.0)
        .center_y()
        .radius(4.0)
        .fill(color("#1F1F1F"))
        .child(
            text(name.to_owned())
                .font(style(if compact { 11.0 } else { 13.0 }, 400.0))
                .color(rgb(0xFFFFFF))
                .truncate()
                .min_w(0.0),
        );
    for (visible, suffix, icon, label) in [
        (person.muted, "muted", "mic_off", "Muted"),
        (speaking, "speaking", "mic", "Speaking"),
        (person.hand.is_some(), "hand", "back_hand", "Hand raised"),
        (person.sharing, "sharing", "share", "Sharing"),
        (
            call.pinned.as_deref() == Some(person.mri.as_str()),
            "pinned",
            "keep",
            "Pinned for me",
        ),
        (
            person.spotlight.is_some(),
            "spotlight",
            "star",
            "Spotlighted",
        ),
    ] {
        if visible {
            plate = plate.child(
                crate::icons::sym(icon, 14.0)
                    .key(format!("{key}-{suffix}"))
                    .label(label)
                    .color(rgb(0xFFFFFF)),
            );
        }
    }
    tile = tile.child(picture).child(plate);
    if !own {
        let mri = person.mri.clone();
        tile = tile.child(
            row()
                .key(format!("{key}-menu"))
                .label("More options")
                .pin(None, Some(4.0), Some(4.0), None)
                .size(24.0, 24.0)
                .center()
                .radius(4.0)
                .fill(color("#292929"))
                .on_press(move |s: &mut UiState<A>| {
                    s.calls.tile_menu = if s.calls.tile_menu.as_ref() == Some(&mri) {
                        None
                    } else {
                        Some(mri.clone())
                    };
                })
                .child(crate::icons::sym("more_horiz", 18.0).color(rgb(0xFFFFFF))),
        );
    }
    tile
}

pub fn view<A: Actions + 'static>(s: &UiState<A>) -> El {
    view_with_motion(s, false)
}

pub fn view_with_motion<A: Actions + 'static>(s: &UiState<A>, reduced_motion: bool) -> El {
    let call = &s.calls;
    let active = call.state.as_ref().is_some_and(CallState::is_active);
    if s.call_window.is_some() && (active || call.ring.is_some()) {
        return col()
            .grow(1.0)
            .h_full()
            .child(button::<A>("Return to call", "return-to-call", |s, _| {
                if let Some(window) = &s.call_window {
                    window.open();
                }
            }))
            .child(history_view(s));
    }
    if s.call_window.is_some() && matches!(call.state, Some(CallState::Ended { .. })) {
        return history_view(s);
    }
    if call.ring.is_none()
        && let Some(CallState::Ended { reason }) = &call.state
    {
        let mut actions =
            row()
                .gap(8.0)
                .center()
                .child(button::<A>("Close", "call-end-close", |s, tx| {
                    s.calls.state = None;
                    s.calls.pending = false;
                    s.calls.prejoin_open = false;
                    s.calls.link.clear();
                    s.calls.stop_preview();
                    if s.calls.detached {
                        tx.close_window();
                    }
                }));
        if !s.read_only
            && !matches!(
                reason,
                calling::EndReason::Remote(calling::EndKind::RoomClosed)
            )
            && let Some(retry) = &call.retry
        {
            let retry = retry.clone();
            actions = actions.child(button::<A>("Rejoin", "call-end-rejoin", move |s, _| {
                if !s.calls.pending {
                    s.calls.pending = true;
                    retry();
                }
            }));
        }
        return col()
            .key("call-ended")
            .w_full()
            .h_full()
            .center()
            .gap(20.0)
            .fill(color("#1F1F1F"))
            .child(crate::icons::sym("call", 48.0).color(rgb(0xFFFFFF)))
            .child(
                text(end_reason(reason))
                    .key("call-end-reason")
                    .font(style(20.0, 600.0))
                    .color(rgb(0xFFFFFF))
                    .wrap(),
            )
            .children(call.notice.as_ref().map(|notice| {
                text(notice.clone())
                    .font(style(13.0, 400.0))
                    .color(color("#FFB3BE"))
                    .wrap()
            }))
            .child(actions);
    }
    if !active && call.ring.is_none() && call.link.text.is_empty() && !call.prejoin_open {
        return history_view(s);
    }
    let mut body = col()
        .key("calls")
        .w_full()
        .padding(24.0)
        .gap(16.0)
        .fill(color("#FFFFFF"))
        .child(
            text("Calls")
                .font(style(20.0, 700.0))
                .color(color("#242424")),
        );
    if let Some(notice) = &call.notice {
        body = body.child(
            text(notice.clone())
                .key("call-notice")
                .font(style(13.0, 400.0))
                .color(color("#C4314B"))
                .wrap(),
        );
    }
    if let Some(ring) = &call.ring {
        let id = ring.ring_id;
        let name = if ring.caller.display_name.is_empty() {
            &ring.caller.mri
        } else {
            &ring.caller.display_name
        };
        body = body.child(
            text(format!("Incoming call from {name}"))
                .font(style(16.0, 600.0))
                .color(color("#242424")),
        );
        if !s.read_only && !call.pending {
            let mut controls = row().gap(8.0);
            if !active {
                controls = controls
                    .child(button::<A>("Answer", "call-answer", move |s, _| {
                        s.calls.pending = true;
                        s.calls.video = false;
                        s.actions.accept_call(id, false);
                    }))
                    .child(button::<A>(
                        "Answer with video",
                        "call-answer-video",
                        move |s, _| {
                            s.calls.pending = true;
                            s.calls.video = true;
                            s.actions.accept_call(id, true);
                        },
                    ));
            }
            body =
                body.child(
                    controls.child(button::<A>("Decline", "call-decline", move |s, _| {
                        s.actions.decline_call(id);
                        s.calls.ring = None;
                    })),
                );
        }
    }
    if active {
        let label = call.status(Instant::now());
        body = col()
            .key("call-session")
            .w_full()
            .h_full()
            .min_h(0.0)
            .fill(color("#1F1F1F"));
        let mut panels = col()
            .key("call-panel")
            .w(280.0)
            .h_full()
            .min_h(0.0)
            .padding(16.0)
            .gap(12.0)
            .fill(color("#FFFFFF"))
            .scroll(ScrollSpec {
                stick_bottom: false,
            });
        let mut controls = row()
            .key("call-toolbar")
            .w_full()
            .min_h(72.0)
            .px(16.0)
            .gap(8.0)
            .center_y()
            .fill(color("#292929"))
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .child(
                        text(label)
                            .key("call-status")
                            .font(style(13.0, 600.0))
                            .color(rgb(0xFFFFFF))
                            .truncate(),
                    )
                    .child(health_indicator::<A>(call)),
            );
        controls = controls.child(button::<A>(
            if call.muted { "Unmute" } else { "Mute" },
            "call-mute",
            |s, _| {
                s.actions
                    .call_command(CallCommand::Mute(MuteCommand::Toggle));
            },
        ));
        let camera = call.camera;
        controls = controls.child(button::<A>(
            if camera { "Camera on" } else { "Camera off" },
            "call-camera",
            move |s, _| {
                s.actions.call_command(CallCommand::SetCamera(!camera));
            },
        ));
        controls = controls.child(button::<A>("Devices", "call-devices", |s, _| {
            s.calls.show_devices = !s.calls.show_devices;
            s.calls.show_people = false;
            s.calls.show_shares = false;
            s.calls.show_captions = false;
            if s.calls.show_devices {
                s.actions.call_command(CallCommand::RefreshDevices);
            }
        }));
        if call.local_share.is_some() {
            controls = controls.child(button::<A>("Stop sharing", "stop-share", |s, _| {
                s.actions.call_command(CallCommand::StopShare)
            }));
        } else {
            controls = controls.child(button::<A>("Share", "call-share", |s, _| {
                s.calls.show_shares = !s.calls.show_shares;
                s.calls.show_people = false;
                s.calls.show_devices = false;
                s.calls.show_captions = false;
                if s.calls.show_shares {
                    s.actions.call_command(CallCommand::RefreshShareSources);
                }
            }));
        }
        controls = controls
            .child(button::<A>("People", "call-people", |s, _| {
                s.calls.show_people = !s.calls.show_people;
                s.calls.show_devices = false;
                s.calls.show_shares = false;
                s.calls.show_captions = false;
            }))
            .child(button::<A>("More", "call-more", |s, _| {
                s.calls.show_reactions = !s.calls.show_reactions;
            }));
        if let Some(thread) = call.chat_thread() {
            let thread = thread.to_owned();
            // GPUI's `call_chat_unread`: Rune knows whether the chat is unread, not how much.
            let unread = !call.show_chat && s.chats.iter().any(|chat| chat.id == thread && chat.unread);
            let chat = button::<A>("Chat", "call-chat", move |s, tx| {
                s.calls.show_chat = !s.calls.show_chat;
                // The panel reuses the selected-chat views. The call window has its own selection
                // and its bridge follows the chat apart (`Bridge::open`); inline (demo) calls
                // select the chat in the main window.
                if s.calls.show_chat && s.selected.as_deref() != Some(thread.as_str()) {
                    crate::ui::open_chat(s, &thread, tx);
                }
            });
            controls = controls.child(if unread {
                chat.child(el().key("call-chat-unread").w(8.0).h(8.0).radius(4.0).fill(color("#C4314B")))
            } else {
                chat
            });
        }
        controls = controls.child(button::<A>("Leave", "call-leave", |s, _| {
            s.actions.call_command(CallCommand::Hangup)
        }));
        body = body.child(controls);
        if call.recording || call.transcribing || call.recording_pending {
            let label = if call.recording_pending {
                "Starting recording…"
            } else if call.recording && call.recording_audio_only {
                "● Recording audio"
            } else if call.recording {
                "● Recording"
            } else {
                "Transcribing"
            };
            body = body.child(
                text(label)
                    .key("call-recording-indicator")
                    .font(style(13.0, 600.0))
                    .color(color("#FFB3BE")),
            );
        }
        if call.consent_required {
            body = body.child(col().key("call-recording-consent").w_full().padding(12.0).gap(8.0)
                .child(text("This meeting is being recorded. Agree to being recorded to turn on your microphone or camera.")
                    .font(style(14.0, 400.0)).color(rgb(0xFFFFFF)).wrap())
                .child(button::<A>("Agree", "call-consent-agree", |s, _| s.actions.call_command(CallCommand::ConsentToRecording))));
        }
        if let Some(command) = &call.confirm {
            let prompt = match command {
                CallCommand::EndMeeting => "End the meeting for everyone?",
                CallCommand::RemoveParticipant { .. } => "Remove this person from the meeting?",
                CallCommand::MuteAll => "Mute everyone?",
                CallCommand::StopRecording { scope } => RecordAction::Stop(*scope).confirm_title().unwrap_or("Stop recording?"),
                _ => "Continue?",
            };
            body = body.child(
                row()
                    .key("call-confirm")
                    .w_full()
                    .padding(12.0)
                    .gap(8.0)
                    .wrap()
                    .child(text(prompt).font(style(14.0, 600.0)).color(rgb(0xFFFFFF)))
                    .child(button::<A>("Confirm", "call-confirm-yes", |s, _| {
                        let Some(command) = s.calls.confirm.take() else { return };
                        let allowed = match &command {
                            CallCommand::StopRecording { scope } => s.calls.record_model().action_allowed(RecordAction::Stop(*scope)),
                            _ => s.calls.can_manage() && !s.calls.lobby,
                        };
                        if allowed {
                            s.actions.call_command(command);
                        }
                    }))
                    .child(button::<A>("Cancel", "call-confirm-no", |s, _| {
                        s.calls.confirm = None;
                    })),
            );
        }
        let waiting = call.roster.iter().filter(|p| p.in_lobby).count();
        if waiting > 0 && call.can_manage() && !call.lobby {
            body = body.child(
                row()
                    .key("call-lobby-banner")
                    .w_full()
                    .padding(8.0)
                    .gap(8.0)
                    .wrap()
                    .child(
                        text(if waiting == 1 {
                            "1 person is waiting in the lobby".into()
                        } else {
                            format!("{waiting} people are waiting in the lobby")
                        })
                        .color(rgb(0xFFFFFF)),
                    )
                    .child(button::<A>("View lobby", "call-open-lobby", |s, _| {
                        s.calls.show_people = true;
                        s.calls.show_devices = false;
                        s.calls.show_shares = false;
                    }))
                    .child(button::<A>("Admit all", "lobby-admit-all", |s, _| {
                        s.actions.call_command(CallCommand::AdmitAll);
                    })),
            );
        }
        if call.show_health
            && let Some(health) = &call.health
        {
            body = body.child(
                text(format!(
                    "Call health · Packet loss {:.1}% · Round trip {:.0} ms · Jitter {:.0} ms",
                    health.sample.loss_percent,
                    health.sample.round_trip_ms,
                    health.sample.jitter_ms
                ))
                .key("call-health-details")
                .font(style(13.0, 400.0))
                .color(rgb(0xFFFFFF))
                .wrap(),
            );
        }
        if let Some(ring) = &call.ring {
            let mut incoming = row().w_full().padding(12.0).gap(12.0).center_y().child(
                text(format!("Incoming call from {}", ring.caller.display_name))
                    .font(style(14.0, 600.0))
                    .color(rgb(0xFFFFFF))
                    .grow(1.0),
            );
            if !s.read_only && !call.pending {
                let id = ring.ring_id;
                incoming = incoming.child(button::<A>("Decline", "call-decline", move |s, _| {
                    s.actions.decline_call(id);
                    s.calls.ring = None;
                }));
            }
            body = body.child(incoming);
        }
        let hand = call.hand;
        let mut features = row().gap(8.0).child(button::<A>(
            if hand { "Lower hand" } else { "Raise hand" },
            "call-hand",
            move |s, _| s.actions.call_command(CallCommand::SetHand(!hand)),
        ));
        if !matches!(call.captions, calling::CaptionState::Starting) {
            let enabled = matches!(call.captions, calling::CaptionState::On);
            features = features.child(button::<A>(
                if enabled {
                    "Turn captions off"
                } else {
                    "Turn captions on"
                },
                "call-captions",
                move |s, _| s.actions.call_command(CallCommand::SetCaptions(!enabled)),
            ));
        }
        // GPUI's "Record and transcribe" menu: blocked actions are greyed with the reason.
        let record = call.record_model();
        if !s.read_only && !record.record_hidden() {
            if let Some(blocked) = record.record_menu_block() {
                features = features.child(blocked_record("Record and transcribe", blocked));
            } else {
                for (action, label) in record_actions(record.recording, record.transcribing) {
                    if let Some(blocked) = record.action_block(action) {
                        features = features.child(blocked_record(label, blocked));
                        continue;
                    }
                    let key = record_key(label);
                    features = features.child(button::<A>(label, &key, move |s, _| match action {
                        RecordAction::Start(kind) => {
                            let title = recording_title(s);
                            s.actions.call_command(CallCommand::StartRecording { kind, title });
                        }
                        RecordAction::Stop(scope) => s.calls.confirm = Some(CallCommand::StopRecording { scope }),
                    }));
                }
            }
        }
        for reaction in calling::Reaction::ALL {
            let label = reaction.name().to_owned();
            features = features.child(button::<A>(
                &label,
                &format!("reaction-{label}"),
                move |s, _| {
                    s.actions
                        .call_command(CallCommand::SendReaction(reaction.clone()))
                },
            ));
        }
        if call.show_reactions {
            body = body.child(
                row()
                    .w_full()
                    .padding(8.0)
                    .scroll(ScrollSpec {
                        stick_bottom: false,
                    })
                    .child(features),
            );
            body = body.child(button::<A>(
                "Captions history",
                "call-caption-history",
                |s, _| {
                    s.calls.show_captions = !s.calls.show_captions;
                    s.calls.show_people = false;
                    s.calls.show_devices = false;
                    s.calls.show_shares = false;
                },
            ));
        }
        if let Some(notice) = &call.notice {
            body = body.child(
                text(notice.clone())
                    .font(style(13.0, 400.0))
                    .color(rgb(0xFFFFFF))
                    .wrap(),
            );
        }
        if let calling::CaptionState::Failed(error) = &call.captions {
            body = body.child(
                text(error.clone())
                    .font(style(13.0, 400.0))
                    .color(color("#FFB3BE")),
            );
        }
        if let Some(caption) = &call.caption {
            body = body.child(
                text(format!("{}: {}", caption.display_name, caption.text))
                    .font(style(14.0, 400.0))
                    .color(rgb(0xFFFFFF))
                    .wrap(),
            );
        }
        if call.show_devices {
            panels = panels
                .child(device_panel::<A>(call, false))
                .child(effects_panel::<A>(call, false));
        }
        if call.show_captions {
            panels = panels.child(
                text("Captions")
                    .font(style(18.0, 600.0))
                    .color(color("#242424")),
            );
            if matches!(call.captions, calling::CaptionState::On) {
                for (tag, name) in calling::SPOKEN_LANGUAGES {
                    let label = format!(
                        "{}{}",
                        if call.caption_language.as_deref() == Some(tag) {
                            "✓ "
                        } else {
                            ""
                        },
                        name
                    );
                    panels = panels.child(button::<A>(
                        &label,
                        &format!("caption-language-{tag}"),
                        move |s, _| {
                            s.calls.caption_language = Some(tag.to_owned());
                            s.actions
                                .call_command(CallCommand::SetCaptionLanguage(tag.to_owned()));
                        },
                    ));
                }
            }
            for (index, caption) in call.caption_history.iter().enumerate() {
                panels = panels.child(
                    text(format!("{}: {}", caption.display_name, caption.text))
                        .key(format!("caption-{index}-{}-{}", caption.user_id, caption.id))
                        .font(style(13.0, 400.0))
                        .color(color("#242424"))
                        .wrap(),
                );
            }
        }
        if call.show_shares && call.local_share.is_none() {
            let mut shares = col().gap(4.0).child(
                text("Choose what to share")
                    .font(style(13.0, 700.0))
                    .color(color("#242424")),
            );
            for source in &call.shares {
                let source = source.clone();
                shares = shares.child(button::<A>(
                    &source.label(),
                    &format!("share-{:?}-{}", source.kind, source.id),
                    move |s, _| {
                        s.actions
                            .call_command(CallCommand::StartShare(source.clone()));
                        s.calls.show_shares = false;
                    },
                ));
            }
            panels = panels.child(shares);
        }
        let mut stage = col()
            .key("call-stage")
            .grow(1.0)
            .min_w(0.0)
            .h_full()
            .min_h(0.0)
            .padding(16.0)
            .gap(12.0);
        for (index, (id, mri, reaction, at)) in call.reactions.iter().enumerate() {
            let progress =
                (Instant::now().saturating_duration_since(*at).as_secs_f32() / 4.0).min(1.0);
            let name = call
                .roster
                .iter()
                .find(|p| &p.mri == mri)
                .map_or(mri.as_str(), |p| p.display_name.as_str());
            stage = stage.child(
                text(reaction_glyph(reaction))
                    .key(format!("call-reaction-{id}"))
                    .label(format!("{name}: {}", reaction.name()))
                    .font(style(40.0, 400.0))
                    .color(rgb(0xFFFFFF))
                    .opacity(if reduced_motion { 1.0 } else { 1.0 - progress })
                    .pin(
                        None,
                        None,
                        Some(16.0 + (index % 4) as f32 * 52.0),
                        Some(
                            16.0 + if reduced_motion {
                                0.0
                            } else {
                                progress * 160.0
                            },
                        ),
                    ),
            );
        }
        if call.show_reactions {
            let mut layouts = row().gap(8.0);
            for (label, layout) in [
                ("Gallery", Layout::Gallery),
                ("Speaker", Layout::Speaker),
                ("Content", Layout::Content),
            ] {
                if layout == Layout::Content && !call.has_content() {
                    continue;
                }
                layouts = layouts.child(button::<A>(
                    label,
                    &format!("layout-{label}"),
                    move |s, _| {
                        s.calls.layout = layout;
                        s.calls.page = 0;
                        sync_video(s);
                    },
                ));
            }
            stage = stage.child(layouts);
        }
        if let Some(person) = call
            .tile_menu
            .as_ref()
            .and_then(|mri| call.roster.iter().find(|p| &p.mri == mri && !p.in_lobby))
        {
            let mri = person.mri.clone();
            let mut menu = col().key("call-tile-menu").gap(4.0).child(button::<A>(
                if call.pinned.as_ref() == Some(&mri) {
                    "Unpin"
                } else {
                    "Pin for me"
                },
                "call-pin",
                move |s, _| {
                    s.calls.pinned = if s.calls.pinned.as_ref() == Some(&mri) {
                        None
                    } else {
                        Some(mri.clone())
                    };
                    s.calls.tile_menu = None;
                    sync_video(s);
                },
            ));
            if call.can_manage() {
                let mri = person.mri.clone();
                let spotlighted = person.spotlight.is_some();
                menu = menu.child(button::<A>(
                    if spotlighted {
                        "Stop spotlight"
                    } else {
                        "Spotlight for everyone"
                    },
                    "call-spotlight",
                    move |s, _| {
                        s.actions.call_command(if spotlighted {
                            CallCommand::StopSpotlight { mri: mri.clone() }
                        } else {
                            CallCommand::Spotlight { mri: mri.clone() }
                        });
                        s.calls.tile_menu = None;
                    },
                ));
            }
            stage = stage.child(menu);
        }
        let shared = call
            .surfaces
            .get(&VideoKey::Screen)
            .or_else(|| call.surfaces.get(&VideoKey::LocalScreen));
        if call.has_content() {
            let presentation = el().key("call-presentation").w_full().grow(3.0).min_h(0.0);
            stage = stage.child(if let Some(surface) = shared {
                presentation.native_surface(surface, SurfaceFit::Fit)
            } else {
                presentation.child(text("Loading shared content…").color(rgb(0xFFFFFF)))
            });
        }
        let people = call.visible_people();
        let own = RosterEntry {
            mri: call.own_mri.clone(),
            muted: call.muted,
            hand: call.hand.then_some(calling::RaisedHand {
                state_id: String::new(),
                rank: 0,
            }),
            sharing: call.local_share.is_some(),
            ..Default::default()
        };
        let mut gallery = col()
            .key("call-gallery")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .gap(12.0)
            .on_layout(|s: &mut UiState<A>, [_, _, width, height]| {
                s.calls.stage_size = (width, height)
            });
        let focused = !call.has_content()
            && (call.layout == Layout::Speaker
                || call.pinned.is_some()
                || people.iter().any(|(p, _)| p.spotlight.is_some()));
        if call.has_content() || focused {
            let mut remaining = people.as_slice();
            if focused && let Some(((person, _), others)) = remaining.split_first() {
                gallery = gallery.child(participant_tile::<A>(
                    call,
                    person,
                    (call.stage_size.0, (call.stage_size.1 - 132.0).max(0.0)),
                ));
                remaining = others;
            }
            let mut strip = row()
                .w_full()
                .h(120.0)
                .min_h(120.0)
                .gap(12.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                });
            for (person, _) in remaining {
                strip = strip.child(participant_tile::<A>(call, person, (180.0, 120.0)));
            }
            if call.camera && call.layout != Layout::Content {
                strip = strip.child(participant_tile::<A>(call, &own, (180.0, 120.0)));
            }
            if !remaining.is_empty() || call.camera && call.layout != Layout::Content {
                gallery = gallery.child(strip);
            }
            if call.has_content() {
                gallery = gallery.grow(0.0).h(120.0).min_h(120.0).max_h(120.0);
            }
        } else if !people.is_empty() {
            let gallery::GalleryGrid {
                columns,
                tile_width: width,
                tile_height: height,
            } = gallery::gallery_grid(people.len(), call.stage_size.0, call.stage_size.1, 12.0);
            for group in people.chunks(columns) {
                let mut tiles = row().w_full().h(height).gap(12.0).center();
                for (person, _) in group {
                    tiles = tiles.child(participant_tile::<A>(call, person, (width, height)));
                }
                gallery = gallery.child(tiles);
            }
        }
        if people.is_empty() && !call.has_content() {
            gallery = gallery.child(
                col()
                    .w_full()
                    .grow(1.0)
                    .center()
                    .gap(16.0)
                    .child(crate::icons::sym("call", 40.0).color(color("#B8B8E6")))
                    .child(
                        text(
                            if call.lobby
                                || matches!(
                                    call.state,
                                    Some(CallState::Connecting | CallState::Reconnecting { .. })
                                )
                            {
                                call.status(Instant::now())
                            } else {
                                "Waiting for others to join".into()
                            },
                        )
                        .key("call-waiting")
                        .font(style(20.0, 600.0))
                        .color(rgb(0xFFFFFF)),
                    ),
            );
        }
        if !people.is_empty()
            || !call.has_content()
            || call.camera && call.layout != Layout::Content
        {
            stage = stage.child(gallery);
        }
        if !call.has_content() && !focused {
            let pages = gallery::gallery_pages(call.rotation.order().len(), 0);
            if pages > 1 {
                let mut pager = row().key("call-gallery-pages").gap(8.0).center();
                if call.page > 0 {
                    pager = pager.child(button::<A>("Previous", "gallery-previous", |s, _| {
                        s.calls.page -= 1;
                        sync_video(s);
                    }));
                }
                pager =
                    pager.child(text(format!("{} / {pages}", call.page + 1)).color(rgb(0xFFFFFF)));
                if call.page + 1 < pages {
                    pager = pager.child(button::<A>("Next", "gallery-next", |s, _| {
                        s.calls.page += 1;
                        sync_video(s);
                    }));
                }
                stage = stage.child(pager);
            }
        }
        if call.camera && !call.has_content() && !focused {
            stage = stage.child(
                row()
                    .w_full()
                    .h(132.0)
                    .child(el().grow(1.0))
                    .child(participant_tile::<A>(call, &own, (208.0, 132.0))),
            );
        }
        if call.show_people {
            panels = panels.child(
                text(format!("People ({})", call.roster.len()))
                    .font(style(18.0, 600.0))
                    .color(color("#242424")),
            );
            let can_manage = call.can_manage() && !call.lobby;
            if can_manage {
                if waiting > 0 {
                    panels = panels.child(button::<A>("Admit all", "people-admit-all", |s, _| {
                        s.actions.call_command(CallCommand::AdmitAll)
                    }));
                }
                panels = panels
                    .child(button::<A>("Mute all", "people-mute-all", |s, _| {
                        s.calls.confirm = Some(CallCommand::MuteAll);
                    }))
                    .child(button::<A>(
                        "Lower all hands",
                        "people-lower-all",
                        |s, _| s.actions.call_command(CallCommand::LowerAllHands),
                    ))
                    .child(button::<A>("Invite people", "people-invite", |s, _| {
                        s.calls.invite_open = !s.calls.invite_open;
                        if s.calls.invite_open {
                            s.chatlist.manage.add = crate::new_chat::Draft::default();
                        }
                    }))
                    .child(button::<A>(
                        "End meeting for all",
                        "people-end-meeting",
                        |s, _| {
                            s.calls.confirm = Some(CallCommand::EndMeeting);
                        },
                    ));
                if call.invite_open {
                    let skip = call
                        .roster
                        .iter()
                        .map(|p| p.mri.strip_prefix("8:orgid:").unwrap_or(&p.mri).to_owned())
                        .collect::<Vec<_>>();
                    panels = panels.child(crate::new_chat::picker(
                        s,
                        &s.chatlist.manage.add,
                        "call-invite",
                        invite_draft::<A>,
                        &skip,
                    ));
                    if !s.chatlist.manage.add.chips.is_empty() {
                        panels = panels.child(button::<A>("Add", "call-invite-send", |s, _| {
                            let callees = std::mem::take(&mut s.chatlist.manage.add.chips)
                                .into_iter()
                                .map(|p| calling::Callee {
                                    mri: format!("8:orgid:{}", p.user_id),
                                    display_name: p.name,
                                })
                                .collect();
                            s.actions.call_command(CallCommand::Invite {
                                callees,
                                group_thread: None,
                            });
                            s.calls.invite_open = false;
                        }));
                    }
                }
            }
            for person in &call.roster {
                let name = if person.display_name.is_empty() {
                    &person.mri
                } else {
                    &person.display_name
                };
                let detail = if person.in_lobby {
                    " · Waiting in lobby"
                } else if person.muted {
                    " · Muted"
                } else {
                    ""
                };
                let mut person_row = row().w_full().gap(8.0).wrap().center_y().child(
                    text(format!("{name}{detail}"))
                        .font(style(13.0, 400.0))
                        .color(color("#242424")),
                );
                if can_manage && person.mri != call.own_mri {
                    if person.in_lobby {
                        let mri = person.mri.clone();
                        person_row = person_row.child(button::<A>(
                            "Admit",
                            &format!("admit-{mri}"),
                            move |s, _| {
                                s.actions
                                    .call_command(CallCommand::Admit { mri: mri.clone() })
                            },
                        ));
                        let mri = person.mri.clone();
                        person_row = person_row.child(button::<A>(
                            "Deny",
                            &format!("deny-{mri}"),
                            move |s, _| {
                                s.actions
                                    .call_command(CallCommand::Deny { mri: mri.clone() })
                            },
                        ));
                    } else {
                        if !person.muted {
                            let mri = person.mri.clone();
                            person_row = person_row.child(button::<A>(
                                "Mute",
                                &format!("mute-{mri}"),
                                move |s, _| {
                                    s.actions.call_command(CallCommand::MuteParticipant {
                                        mri: mri.clone(),
                                    })
                                },
                            ));
                        }
                        if person.hand.is_some() {
                            let mri = person.mri.clone();
                            person_row = person_row.child(button::<A>(
                                "Lower hand",
                                &format!("lower-{mri}"),
                                move |s, _| {
                                    s.actions
                                        .call_command(CallCommand::LowerHand { mri: mri.clone() });
                                },
                            ));
                        }
                        let mri = person.mri.clone();
                        person_row = person_row.child(button::<A>(
                            "Remove",
                            &format!("remove-{mri}"),
                            move |s, _| {
                                s.calls.confirm =
                                    Some(CallCommand::RemoveParticipant { mri: mri.clone() });
                            },
                        ));
                    }
                }
                panels = panels.child(person_row);
            }
        }
        let mut content = row().w_full().grow(1.0).min_h(0.0).child(stage);
        if call.show_people
            || call.show_devices
            || call.show_captions
            || (call.show_shares && call.local_share.is_none())
        {
            content = content.child(panels);
        }
        if call.show_chat && s.selected.is_some() && s.selected.as_deref() == call.chat_thread() {
            let composer = if s.read_only {
                text("Read-only").font(style(13.0, 400.0)).color(color("#616161"))
            } else {
                crate::view_composer::composer(s, true)
            };
            content = content.child(col().key("call-chat-panel").w(360.0).h_full().min_h(0.0).fill(color("#FFFFFF"))
                .child(crate::view_messages::message_list(s, false))
                .child(composer));
        }
        return body.child(content);
    } else if call.ring.is_none() {
        body = body
            .child(
                text("Join a meeting")
                    .font(style(16.0, 600.0))
                    .color(color("#242424")),
            )
            .child(
                text(if call.prejoin_mic {
                    "Your microphone is on"
                } else {
                    "Your microphone starts muted"
                })
                .font(style(13.0, 400.0))
                .color(color("#616161")),
            )
            .child(
                text_input(&call.link, style(14.0, 400.0))
                    .key("meeting-link")
                    .label("Meeting link")
                    .placeholder("Paste a meeting link", color("#616161"))
                    .color(color("#242424"))
                    .w_full()
                    .on_edit(|s: &mut UiState<A>| &mut s.calls.link),
            );
        if !s.read_only {
            if call.video {
                let preview = el()
                    .key("prejoin-preview")
                    .w_full()
                    .h(220.0)
                    .fill(color("#292929"));
                body = body.child(
                    if let Some(surface) = call.surfaces.get(&VideoKey::LocalCamera) {
                        preview.native_surface(surface, SurfaceFit::Fit)
                    } else {
                        preview.child(text("Starting camera…").color(rgb(0xFFFFFF)))
                    },
                );
            }
            body = body
                .child(button::<A>(
                    if call.video {
                        "Camera on"
                    } else {
                        "Camera off"
                    },
                    "prejoin-camera",
                    |s, _| {
                        s.calls.video = !s.calls.video;
                        if !s.calls.video {
                            s.calls.stop_preview();
                        }
                    },
                ))
                .child(button::<A>(
                    if call.prejoin_mic {
                        "Microphone on"
                    } else {
                        "Microphone off"
                    },
                    "prejoin-mic",
                    |s, _| {
                        s.calls.prejoin_mic = !s.calls.prejoin_mic;
                    },
                ))
                .child(button::<A>(
                    "Device settings and effects",
                    "prejoin-devices",
                    |s, _| {
                        s.calls.show_devices = !s.calls.show_devices;
                    },
                ));
            if !call.pending {
                body = body.child(button::<A>("Join now", "join-call", |s, _| {
                    if !s.calls.link.text.trim().is_empty() {
                        s.calls.pending = true;
                        s.calls.notice = None;
                        s.calls.stop_preview();
                        for command in s.calls.prejoin_commands() {
                            s.actions.call_command(command);
                        }
                        s.actions.join_call(&s.calls.link.text, s.calls.video);
                    }
                }));
            } else {
                body = body.child(
                    text("Joining…")
                        .font(style(13.0, 400.0))
                        .color(color("#616161")),
                );
            }
            if call.show_devices {
                body = body
                    .child(device_panel::<A>(call, true))
                    .child(effects_panel::<A>(call, true));
            }
        }
    }
    col()
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .min_h(0.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake {
        actions: RefCell<Vec<String>>,
    }

    impl Actions for Fake {
        fn load_call_history(&self) {
            self.actions.borrow_mut().push("history".into());
        }
        fn call_back(&self, entry: &chatsvc::CallLogEntry) {
            self.actions
                .borrow_mut()
                .push(format!("call back {}", entry.call_id));
        }
        fn open(&self, _: &str) {}
        fn send(&self, target: crate::types::SendTarget, text: String, _: Vec<teams_core::FileReference>) {
            self.actions.borrow_mut().push(format!("send {} {text}", target.conversation_id));
        }
        fn join_call(&self, url: &str, video: bool) {
            self.actions
                .borrow_mut()
                .push(format!("join {url} {video}"));
        }
        fn call_command(&self, command: CallCommand) {
            self.actions.borrow_mut().push(format!("{command:?}"));
        }
        fn accept_call(&self, ring_id: u64, video: bool) {
            self.actions
                .borrow_mut()
                .push(format!("accept {ring_id} {video}"));
        }
        fn decline_call(&self, ring_id: u64) {
            self.actions.borrow_mut().push(format!("decline {ring_id}"));
        }
    }

    fn crowd(count: usize) -> Vec<RosterEntry> {
        (0..count)
            .map(|index| RosterEntry {
                mri: format!("p{index}"),
                display_name: format!("Person {index}"),
                has_video: true,
                ..Default::default()
            })
            .collect()
    }

    fn connected(count: usize) -> UiState<Fake> {
        let mut state = UiState::new(Fake::default());
        state.calls.apply(CallUpdate::State(CallState::Connecting));
        state.calls.apply(CallUpdate::State(CallState::Connected {
            since: Instant::now(),
        }));
        state
            .calls
            .apply(CallUpdate::OwnIdentity { mri: "me".into() });
        state.calls.apply(CallUpdate::Roster(crowd(count)));
        state
    }

    fn runtime(state: UiState<Fake>) -> Runtime<UiState<Fake>> {
        let mut rt = Runtime::new(
            state,
            crate::call_window::view::<Fake>,
            (1180.0, 760.0),
            Env::default(),
        );
        rt.frame(1.0);
        rt.frame(2.0);
        rt.frame(3.0);
        rt
    }

    fn click(rt: &mut Runtime<UiState<Fake>>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("missing {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(4.0);
        rt.frame(5.0);
    }

    #[test]
    fn speakers_hands_sharing_devices_and_health_reach_the_view() {
        let mut state = connected(1);
        state.calls.roster[0].hand = Some(calling::RaisedHand {
            state_id: "hand".into(),
            rank: 1,
        });
        state.calls.roster[0].sharing = true;
        state.calls.apply(CallUpdate::Speakers(vec!["p0".into()]));
        state.calls.apply(CallUpdate::Health(calling::CallHealth {
            level: calling::NetworkLevel::Poor,
            cpu_limited: false,
            sample: calling::HealthSample::default(),
            transport: calling::TransportDetails::default(),
            path: calling::MediaPath::Direct,
        }));
        let mut rt = runtime(state);
        for key in [
            "call-person-p0-speaking",
            "call-person-p0-hand",
            "call-person-p0-sharing",
            "call-health",
        ] {
            assert!(rt.find(key).is_some(), "{key}");
        }
        click(&mut rt, "call-health");
        assert!(rt.find("call-health-details").is_some());
        click(&mut rt, "call-devices");
        rt.state.calls.apply(CallUpdate::Selected {
            input: calling::DeviceChoice::Device("mic".into()),
            output: calling::DeviceChoice::SystemDefault,
        });
        assert_eq!(
            rt.state.calls.selected_input,
            calling::DeviceChoice::Device("mic".into())
        );
        click(&mut rt, "input-Default device");
        assert_eq!(
            rt.state.actions.actions.borrow().last().map(String::as_str),
            Some("SelectInput(SystemDefault)")
        );
        assert_eq!(
            rt.state.calls.selected_input,
            calling::DeviceChoice::Device("mic".into()),
            "wait for the engine's Selected acknowledgement"
        );
        rt.state
            .calls
            .apply(CallUpdate::State(CallState::Connecting));
        assert!(rt.state.calls.speaking.is_empty() && rt.state.calls.health.is_none());
    }

    #[test]
    fn video_wants_follow_visible_page_focus_and_content() {
        let mut state = connected(12);
        let call = &mut state.calls;
        let wants = call.wants();
        assert_eq!(wants.len(), 9);
        assert!(
            wants
                .iter()
                .all(|want| want.detail == calling::TileDetail::Small)
        );
        call.page = 1;
        let wants = call.wants();
        assert_eq!(
            wants
                .iter()
                .map(|want| want.mri.as_str())
                .collect::<Vec<_>>(),
            ["p9", "p10", "p11"]
        );
        assert!(
            wants
                .iter()
                .all(|want| want.detail == calling::TileDetail::Stage)
        );
        call.pinned = Some("p11".into());
        assert_eq!(
            call.wants()[0],
            calling::VideoWant {
                mri: "p11".into(),
                detail: calling::TileDetail::Stage
            }
        );
        assert!(
            call.wants()[1..]
                .iter()
                .all(|want| want.detail == calling::TileDetail::Small)
        );
        call.screen_share = Some("p0".into());
        assert!(
            call.wants()
                .iter()
                .all(|want| want.detail == calling::TileDetail::Small)
        );
        call.layout = Layout::Content;
        assert!(call.wants().is_empty());
        call.screen_share = None;
        call.pinned = None;
        call.page = 0;
        call.apply(CallUpdate::Roster(crowd(2)));
        assert!(
            call.wants()
                .iter()
                .all(|want| want.detail == calling::TileDetail::Stage)
        );
    }

    #[test]
    fn subscriptions_are_sent_once_until_the_visible_layout_changes() {
        let mut state = connected(12);
        sync_video(&mut state);
        sync_video(&mut state);
        assert_eq!(state.actions.actions.borrow().len(), 1);
        state.calls.page = 1;
        sync_video(&mut state);
        assert_eq!(state.actions.actions.borrow().len(), 2);
    }

    #[test]
    fn gallery_pages_pin_menu_and_compact_plates_are_interactive() {
        let mut rt = runtime(connected(12));
        assert!(rt.find("call-person-p0").is_some());
        assert!(rt.find("call-person-p9").is_none());
        let (tile, _) = rt.presented("call-person-p0").unwrap();
        let (plate, _) = rt.presented("call-person-p0-name").unwrap();
        assert!(tile[3] < 200.0);
        assert!(plate[2] <= tile[2] * 0.6 + 0.5);
        click(&mut rt, "gallery-next");
        assert!(rt.find("call-person-p0").is_none());
        assert!(rt.find("call-person-p9").is_some());
        click(&mut rt, "call-person-p11-menu");
        click(&mut rt, "call-pin");
        assert_eq!(rt.state.calls.pinned.as_deref(), Some("p11"));
        assert!(rt.find("call-person-p11-pinned").is_some());
        rt.state.calls.apply(CallUpdate::Roster(crowd(2)));
        assert!(rt.state.calls.pinned.is_none());
        assert_eq!(rt.state.calls.page, 0);
    }

    #[test]
    fn prejoin_choices_prepare_the_call_without_turning_on_capture_in_headless_tests() {
        let mut state = UiState::new(Fake::default());
        state.calls.prejoin_open = true;
        state.calls.link = TextEdit::new("https://teams.microsoft.com/meet/1234");
        let mut rt = runtime(state);
        click(&mut rt, "prejoin-mic");
        click(&mut rt, "prejoin-devices");
        click(&mut rt, "input-Default device");
        assert!(rt.state.actions.actions.borrow().is_empty());
        assert!(rt.state.calls.preview.is_none());
        // Inspect effects without scrolling the long settings list in this command test.
        rt.state.calls.background_blur = true;
        rt.state.calls.effects.adjust_brightness = true;
        click(&mut rt, "join-call");
        let actions = rt.state.actions.actions.borrow();
        assert!(actions.iter().any(|action| action == "SetBackground(Blur)"));
        assert!(actions.iter().any(|action| action == "Mute(Unmute)"));
        assert_eq!(
            actions.last().map(String::as_str),
            Some("join https://teams.microsoft.com/meet/1234 false")
        );
    }

    #[test]
    fn organizer_controls_require_the_own_role_and_confirm_destructive_actions() {
        let mut state = connected(1);
        state.calls.roster.push(RosterEntry {
            mri: "me".into(),
            organizer: true,
            ..Default::default()
        });
        state.calls.roster.push(RosterEntry {
            mri: "guest".into(),
            in_lobby: true,
            ..Default::default()
        });
        let mut rt = runtime(state);
        assert!(rt.find("call-lobby-banner").is_some());
        click(&mut rt, "lobby-admit-all");
        assert_eq!(
            rt.state.actions.actions.borrow().last().map(String::as_str),
            Some("AdmitAll")
        );
        click(&mut rt, "call-people");
        click(&mut rt, "people-end-meeting");
        assert!(
            !rt.state
                .actions
                .actions
                .borrow()
                .iter()
                .any(|action| action == "EndMeeting")
        );
        click(&mut rt, "call-confirm-no");
        click(&mut rt, "people-end-meeting");
        click(&mut rt, "call-confirm-yes");
        assert_eq!(
            rt.state.actions.actions.borrow().last().map(String::as_str),
            Some("EndMeeting")
        );
        let mut attendee = runtime(connected(1));
        click(&mut attendee, "call-people");
        assert!(attendee.find("people-end-meeting").is_none());
    }

    #[test]
    fn captions_merge_revisions_and_recording_consent_is_explicit() {
        let mut state = connected(1);
        state
            .calls
            .apply(CallUpdate::Captions(calling::CaptionState::On));
        let caption = |text: &str, is_final| calling::CaptionEntry {
            id: "phrase".into(),
            user_id: "p0".into(),
            display_name: "Ada".into(),
            text: text.into(),
            is_final,
        };
        state
            .calls
            .apply(CallUpdate::Caption(caption("Hel", false)));
        state
            .calls
            .apply(CallUpdate::Caption(caption("Hello", true)));
        assert_eq!(state.calls.caption_history.len(), 1);
        assert_eq!(state.calls.caption_history[0].text, "Hello");
        state.calls.apply(CallUpdate::Recording(true));
        state.calls.apply(CallUpdate::ConsentRequired(true));
        let mut rt = runtime(state);
        assert!(rt.find("call-recording-indicator").is_some());
        assert!(rt.state.actions.actions.borrow().is_empty());
        click(&mut rt, "call-consent-agree");
        assert_eq!(
            rt.state.actions.actions.borrow().last().map(String::as_str),
            Some("ConsentToRecording")
        );
        assert!(
            rt.state.calls.consent_required,
            "wait for the engine acknowledgement"
        );
        rt.update(|s, _| s.calls.show_captions = true);
        rt.frame(6.0);
        click(&mut rt, "caption-language-en-US");
        assert_eq!(
            rt.state.actions.actions.borrow().last().map(String::as_str),
            Some("SetCaptionLanguage(\"en-US\")")
        );
        rt.state.calls.apply(CallUpdate::ConsentRequired(false));
        rt.state
            .calls
            .apply(CallUpdate::Captions(calling::CaptionState::Off));
        assert!(rt.state.calls.caption.is_none());
        rt.state
            .calls
            .apply(CallUpdate::State(CallState::Connecting));
        assert!(rt.state.calls.caption_history.is_empty() && !rt.state.calls.recording);
    }

    #[test]
    fn recording_starts_from_the_more_menu_unless_the_role_or_meeting_forbids_it() {
        let mut state = connected(1);
        state.calls.apply(CallUpdate::Roster(vec![RosterEntry { mri: "me".into(), role: Some(calling::MeetingRole::Attendee), ..Default::default() }]));
        state.calls.start(CallIntro { kind: CallKind::Meeting, conversation: Some("19:meeting".into()), name: "Meeting".into() });
        state.chats.push(crate::types::ChatRow { id: "19:meeting".into(), title: "Weekly sync".into(), preview: String::new(), unread: false, when: String::new() });
        let mut rt = runtime(state);
        click(&mut rt, "call-more");
        assert!(rt.find("call-record-start-recording").is_none(), "attendees cannot record");
        assert!(rt.find("call-record-record-and-transcribe-blocked").is_some(), "the menu stays, greyed");
        rt.update(|s, _| s.calls.apply(CallUpdate::Roster(vec![RosterEntry { mri: "me".into(), role: Some(calling::MeetingRole::Presenter), ..Default::default() }])));
        rt.frame(1.0);
        click(&mut rt, "call-record-record-audio-only");
        assert_eq!(rt.state.actions.actions.borrow().last().map(String::as_str), Some("StartRecording { kind: AudioOnly, title: \"Weekly sync\" }"));
        rt.update(|s, _| s.calls.apply(CallUpdate::Capability(calling::MeetingCapability { allow_cloud_recording: Some(false), ..Default::default() })));
        rt.frame(1.0);
        assert!(rt.find("call-record-start-recording").is_none(), "the meeting turned recording off");
        assert!(rt.find("call-record-start-recording-blocked").is_some());
        assert!(rt.find("call-record-start-transcription-only").is_some());
        rt.update(|s, _| {
            s.calls.apply(CallUpdate::Capability(calling::MeetingCapability::default()));
            s.calls.apply(CallUpdate::MeetingTenant("tenant-a".into()));
            s.calls.recording_rights(None, Some("tenant-b".into()));
        });
        rt.frame(1.0);
        assert!(rt.find("call-record-record-and-transcribe-blocked").is_some(), "guests from another org cannot record");
    }

    #[test]
    fn the_org_call_policy_blocks_recording_direct_calls_but_not_meetings() {
        let mut call = CallView::default();
        call.start(CallIntro::of(&calling::CallSpec::People {
            callees: vec![calling::Callee { mri: "8:orgid:bea".into(), display_name: "Bea".into() }],
            thread_id: "19:chat".into(),
        }));
        call.apply(CallUpdate::State(CallState::Connected { since: Instant::now() }));
        call.recording_rights(Some(chatsvc::RecordingPolicy { call_recording: false, ..Default::default() }), None);
        assert_eq!(call.record_model().record_block(), Some(RecordBlock::CallPolicy));
        call.intro.as_mut().expect("intro").kind = CallKind::Meeting;
        assert_eq!(call.record_model().record_block(), None);
    }

    #[test]
    fn the_meeting_chat_opens_beside_the_stage_and_sends_to_the_meeting_thread() {
        let mut state = connected(1);
        state.calls.apply(CallUpdate::MeetingChat("19:meeting".into()));
        let mut rt = runtime(state);
        click(&mut rt, "call-chat");
        assert_eq!(rt.state.selected.as_deref(), Some("19:meeting"));
        assert!(rt.find("call-chat-panel").is_some());
        rt.update(|s, _| s.composer = TextEdit::new("hello"));
        rt.frame(1.0);
        click(&mut rt, "send");
        assert_eq!(rt.state.actions.actions.borrow().last().map(String::as_str), Some("send 19:meeting hello"));
        click(&mut rt, "call-chat");
        assert!(rt.find("call-chat-panel").is_none());
    }

    #[test]
    fn people_calls_chat_in_their_chat_and_flag_unread_messages_on_the_chat_button() {
        let mut state = connected(1);
        state.calls.start(CallIntro::of(&calling::CallSpec::People {
            callees: vec![calling::Callee { mri: "8:orgid:bea".into(), display_name: "Bea".into() }],
            thread_id: "19:chat".into(),
        }));
        state.chats.push(crate::types::ChatRow { id: "19:chat".into(), title: "Bea".into(), preview: String::new(), unread: true, when: String::new() });
        let mut rt = runtime(state);
        assert!(rt.find("call-chat-unread").is_some());
        click(&mut rt, "call-chat");
        assert_eq!(rt.state.selected.as_deref(), Some("19:chat"));
        assert!(rt.find("call-chat-panel").is_some());
        assert!(rt.find("call-chat-unread").is_none(), "an open panel shows no badge");
    }

    #[test]
    fn stopping_recording_requires_organizer_confirmation() {
        let mut state = connected(1);
        state.calls.roster.push(RosterEntry {
            mri: "me".into(),
            organizer: true,
            ..Default::default()
        });
        state.calls.apply(CallUpdate::Recording(true));
        state.calls.apply(CallUpdate::Transcribing(true));
        let mut rt = runtime(state);
        click(&mut rt, "call-more");
        click(&mut rt, "call-record-stop-recording");
        assert!(rt.state.actions.actions.borrow().is_empty());
        click(&mut rt, "call-confirm-yes");
        assert_eq!(
            rt.state.actions.actions.borrow().last().map(String::as_str),
            Some("StopRecording { scope: RecordingOnly }")
        );
    }

    #[test]
    fn shortcuts_dispatch_only_when_available_and_reactions_expire() {
        let mut state = connected(1);
        for action in [
            Shortcut::Camera,
            Shortcut::Hand,
            Shortcut::Share,
            Shortcut::Hangup,
        ] {
            assert!(shortcut(&mut state, action));
        }
        assert_eq!(
            state.actions.actions.borrow().as_slice(),
            [
                "SetCamera(true)",
                "SetHand(true)",
                "RefreshShareSources",
                "Hangup"
            ]
        );
        state.read_only = true;
        assert!(!shortcut(&mut state, Shortcut::Camera));
        state.read_only = false;
        state.calls.apply(CallUpdate::Reaction {
            mri: "p0".into(),
            reaction: calling::Reaction::Heart,
        });
        assert_eq!(state.calls.reactions.len(), 1);
        state.calls.reactions[0].3 = Instant::now() - Duration::from_secs(5);
        poll(&mut state, 0.0, &mut Tx::default());
        assert!(state.calls.reactions.is_empty());
        state.calls.state = None;
        assert!(!shortcut(&mut state, Shortcut::Camera));
        state.calls.apply_ring(EngineEvent::Incoming(IncomingRing {
            ring_id: 7,
            caller: calling::Caller {
                mri: "peer".into(),
                display_name: "Peer".into(),
            },
            thread_id: None,
            video: true,
            is_group: false,
            subject: None,
        }));
        assert!(shortcut(&mut state, Shortcut::Accept(false)));
        assert!(!shortcut(&mut state, Shortcut::Accept(true)));
        assert_eq!(
            state.actions.actions.borrow().last().map(String::as_str),
            Some("accept 7 false")
        );
    }

    #[test]
    fn lobby_progress_and_reconnecting_keep_the_original_call_timer() {
        let since = Instant::now();
        let mut call = CallView::default();
        call.apply(CallUpdate::State(CallState::Connecting));
        assert_eq!(call.tone(), None);
        call.apply(CallUpdate::Progress(calling::Progress::Ringing));
        assert_eq!(call.status(since), "Ringing");
        assert_eq!(call.tone(), Some(app_backend::notify::sounds::CallTone::OutgoingRinging));
        call.apply(CallUpdate::Lobby(true));
        assert_eq!(call.status(since), "Waiting in the lobby...");
        call.apply(CallUpdate::Lobby(false));
        call.apply(CallUpdate::State(CallState::Connected { since }));
        assert_eq!(call.status(since + Duration::from_secs(61)), "01:01");
        assert_eq!(call.tone(), None);
        call.apply(CallUpdate::State(CallState::Reconnecting { since }));
        assert_eq!(call.status(since), "Reconnecting...");
        assert_eq!(call.tone(), Some(app_backend::notify::sounds::CallTone::Reconnecting));
        call.apply(CallUpdate::State(CallState::Connected { since }));
        assert_eq!(call.tone(), None);
        assert_eq!(call.status(since + Duration::from_secs(3725)), "1:02:05");
    }

    #[test]
    fn meetings_chime_on_join_lobby_and_leave_while_people_calls_chime_on_end() {
        let meeting = calling::CallSpec::Meeting(calling::MeetingTarget {
            thread_id: "19:meeting".into(),
            tenant_id: "t".into(),
            organizer_id: "me".into(),
            meeting_data: None,
        });
        let mut call = CallView::default();
        call.start(CallIntro::of(&meeting));
        assert_eq!(call.tones, [CallTone::MeetingJoined]);
        call.tones.clear();
        call.apply(CallUpdate::State(CallState::Connecting));
        call.apply(CallUpdate::State(CallState::Connected { since: Instant::now() }));
        call.apply(CallUpdate::OwnIdentity { mri: "me".into() });
        let me = RosterEntry { mri: "me".into(), organizer: true, ..Default::default() };
        let guest = RosterEntry { mri: "guest".into(), in_lobby: true, ..Default::default() };
        call.apply(CallUpdate::Roster(vec![me.clone(), guest.clone()]));
        call.apply(CallUpdate::Roster(vec![me.clone(), guest, RosterEntry { mri: "late".into(), in_lobby: true, ..Default::default() }]));
        assert_eq!(call.tones, [CallTone::MeetingReminder], "one chime per half minute");
        call.tones.clear();
        call.apply(CallUpdate::State(CallState::Ended { reason: calling::EndReason::LocalHangup }));
        call.apply(CallUpdate::State(CallState::Ended { reason: calling::EndReason::LocalHangup }));
        assert_eq!(call.tones, [CallTone::MeetingLeft]);

        let people = calling::CallSpec::People {
            callees: vec![calling::Callee { mri: "8:orgid:bea".into(), display_name: "Bea".into() }],
            thread_id: "19:chat".into(),
        };
        let mut call = CallView::default();
        call.start(CallIntro::of(&people));
        assert!(call.tones.is_empty(), "people calls start silently");
        assert_eq!(call.kind(), CallKind::Direct);
        call.apply(CallUpdate::State(CallState::Connecting));
        call.apply(CallUpdate::State(CallState::Ended { reason: calling::EndReason::Dropped }));
        assert_eq!(call.tones, [CallTone::CallEnded]);

        call.apply_ring(EngineEvent::Incoming(IncomingRing {
            ring_id: 7,
            caller: calling::Caller { mri: "8:orgid:cy".into(), display_name: "Cy".into() },
            thread_id: Some("19:group".into()),
            video: false,
            is_group: true,
            subject: None,
        }));
        call.apply(CallUpdate::State(CallState::Connecting));
        assert_eq!(call.intro, Some(CallIntro { kind: CallKind::Direct, conversation: Some("19:group".into()), name: "Cy".into() }));
    }

    #[test]
    fn end_screen_rejoins_once_and_closes_without_hanging_up() {
        let mut state = UiState::new(Fake::default());
        state.calls.apply(CallUpdate::State(CallState::Ended {
            reason: calling::EndReason::Dropped,
        }));
        let retries = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = retries.clone();
        state.calls.retry = Some(Arc::new(move || {
            count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }));
        let mut rt = Runtime::new(
            state,
            crate::call_window::view::<Fake>,
            (1180.0, 760.0),
            Env::default(),
        );
        rt.frame(1.0);
        assert!(rt.find("call-end-reason").is_some());
        assert!(rt.find("meeting-link").is_none());
        for key in ["call-end-rejoin", "call-end-rejoin", "call-end-close"] {
            let (x, y) = rt.find(key).expect("end screen action");
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
            rt.frame(2.0);
        }
        assert_eq!(retries.load(std::sync::atomic::Ordering::Relaxed), 1);
        assert!(rt.state.calls.state.is_none());
        assert!(rt.state.actions.actions.borrow().is_empty());
    }

    #[test]
    fn updates_track_controls_and_reset_for_next_call() {
        let mut view = CallView {
            pending: true,
            ..Default::default()
        };
        view.apply(CallUpdate::State(CallState::Connecting));
        assert!(!view.pending);
        view.apply(CallUpdate::Muted(false));
        view.apply(CallUpdate::Camera(true));
        assert!(!view.muted && view.camera);
        view.apply(CallUpdate::Notice("offline".into()));
        assert_eq!(view.notice.as_deref(), Some("offline"));
        view.apply(CallUpdate::State(CallState::Connecting));
        assert!(view.muted && view.notice.is_none());
    }

    #[test]
    fn stopped_video_removes_only_its_surface() {
        let mut view = CallView::default();
        let peer = VideoKey::Person("8:orgid:peer".into());
        view.surfaces.insert(peer.clone(), NativeSurface::default());
        view.surfaces
            .insert(VideoKey::LocalCamera, NativeSurface::default());
        view.apply(CallUpdate::VideoStopped(peer.clone()));
        assert!(!view.surfaces.contains_key(&peer));
        assert!(view.surfaces.contains_key(&VideoKey::LocalCamera));
    }

    #[test]
    fn history_loads_and_filters_missed_calls_without_hiding_outgoing_calls_from_all() {
        let mut state = UiState::new(Fake::default());
        crate::screens::navigate(&mut state, crate::screens::Screen::Calls);
        assert_eq!(*state.actions.actions.borrow(), ["history"]);
        assert!(state.calls.history_loading);
        let entry = |id: &str, direction, outcome| chatsvc::CallLogEntry {
            call_id: id.into(),
            started_at: chrono::Utc::now(),
            duration_seconds: None,
            direction,
            outcome,
            is_meeting: false,
            peer_id: Some("ada".into()),
            peer_name: Some("Ada".into()),
            thread_id: Some("chat".into()),
        };
        state.apply(crate::types::Update::CallHistory(Ok(vec![
            entry(
                "missed",
                chatsvc::CallDirection::Incoming,
                chatsvc::CallOutcome::Missed,
            ),
            entry(
                "outgoing",
                chatsvc::CallDirection::Outgoing,
                chatsvc::CallOutcome::Accepted,
            ),
        ])));
        let mut rt = Runtime::new(
            state,
            crate::ui::view::<Fake>,
            (1280.0, 800.0),
            Env {
                system_dark: false,
                scale: 1.0,
            },
        );
        rt.frame(1.0);
        assert!(rt.find("history-call-outgoing").is_some());
        let (x, y) = rt.find("history-Missed").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(2.0);
        assert!(rt.find("history-call-missed").is_some());
        assert!(rt.find("history-call-outgoing").is_none());
        let (x, y) = rt.find("history-call-back-missed").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(3.0);
        assert_eq!(
            rt.state.actions.actions.borrow().last().unwrap(),
            "call back missed"
        );
    }

    #[test]
    fn prejoin_and_leave_dispatch_user_actions() {
        let mut state = UiState::new(Fake::default());
        state.screens.screen = crate::screens::Screen::Calls;
        state.calls.link = TextEdit::new("https://teams.microsoft.com/meet/1234");
        let mut runtime = Runtime::new(
            state,
            crate::ui::view::<Fake>,
            (1000.0, 700.0),
            Env {
                system_dark: false,
                scale: 1.0,
            },
        );
        runtime.frame(1.0);
        let (x, y) = runtime.find("join-call").expect("join button");
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.frame(1.0);
        assert_eq!(
            runtime
                .state
                .actions
                .actions
                .borrow()
                .last()
                .map(String::as_str),
            Some("join https://teams.microsoft.com/meet/1234 false")
        );
        runtime.update(|state, _| state.calls.apply(CallUpdate::State(CallState::Connecting)));
        runtime.frame(1.0);
        let (x, y) = runtime.find("call-leave").expect("leave button");
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.frame(1.0);
        assert_eq!(
            runtime
                .state
                .actions
                .actions
                .borrow()
                .last()
                .map(String::as_str),
            Some("Hangup")
        );
    }
}
