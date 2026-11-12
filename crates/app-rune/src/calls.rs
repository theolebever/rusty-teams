//! Meeting prejoin and active call controls.
use calling::{
    CallCommand, CallState, CallUpdate, EngineEvent, IncomingRing, MuteCommand, RosterEntry,
    VideoKey, VideoPicture,
};
use rune::native_surface::{ByteFrame, ColorInfo, Format};
use rune::tok::Family;
use rune::*;
use crate::ui::text;
use std::collections::HashMap;
use std::sync::Arc;

use crate::types::Actions;
use crate::ui::UiState;

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
            |s: &mut UiState<A>, _| s.calls.prejoin_open = true,
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
            .child(crate::avatar::person(s, name, entry.peer_id.as_deref(), 36.0, None))
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
    surfaces: HashMap<VideoKey, NativeSurface>,
    frame_id: u64,
    devices: calling::DeviceLists,
    cameras: Vec<calling::CameraDevice>,
    shares: Vec<calling::ShareSource>,
    local_share: Option<String>,
    show_devices: bool,
    show_shares: bool,
    hand: bool,
    captions: calling::CaptionState,
    caption: Option<calling::CaptionEntry>,
    reaction: Option<(String, calling::Reaction)>,
    own_mri: String,
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
            surfaces: HashMap::new(),
            frame_id: 0,
            devices: calling::DeviceLists::default(),
            cameras: Vec::new(),
            shares: Vec::new(),
            local_share: None,
            show_devices: false,
            show_shares: false,
            hand: false,
            captions: calling::CaptionState::Off,
            caption: None,
            reaction: None,
            own_mri: String::new(),
        }
    }
}

impl CallView {
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
        match update {
            CallUpdate::State(state) => {
                self.pending = false;
                if matches!(state, CallState::Connecting) {
                    self.roster.clear();
                    self.notice = None;
                    self.muted = true;
                    self.camera = self.video;
                    self.ring = None;
                    self.surfaces.clear();
                    self.hand = false;
                    self.captions = calling::CaptionState::Off;
                    self.caption = None;
                    self.reaction = None;
                    self.local_share = None;
                    self.own_mri.clear();
                }
                self.state = Some(state);
            }
            CallUpdate::Muted(muted) => self.muted = muted,
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
                self.roster = roster;
            }
            CallUpdate::VideoStopped(key) => {
                self.surfaces.remove(&key);
            }
            CallUpdate::ScreenShare(None) => {
                self.surfaces.remove(&VideoKey::Screen);
            }
            CallUpdate::Devices(devices) => self.devices = devices,
            CallUpdate::Cameras(cameras) => self.cameras = cameras,
            CallUpdate::ShareSources(shares) => self.shares = shares,
            CallUpdate::OwnHand(hand) => self.hand = hand.is_some(),
            CallUpdate::Captions(state) => self.captions = state,
            CallUpdate::Caption(caption) => self.caption = Some(caption),
            CallUpdate::Reaction { mri, reaction } => self.reaction = Some((mri, reaction)),
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
            _ => {}
        }
    }
}

fn color(hex: &str) -> Color {
    crate::settings::color(hex)
}
fn style(size: f32, weight: f32) -> TextStyle {
    TextStyle::new(size, weight).family(Family::Named("Segoe UI"))
}

fn button<A: Actions + 'static>(
    label: &str,
    key: &str,
    action: impl Fn(&mut UiState<A>, &mut Tx) + 'static,
) -> El {
    row()
        .key(key.to_owned())
        .h(36.0)
        .px(16.0)
        .radius(6.0)
        .center()
        .fill(color("#E8EBFA"))
        .hover(color("#DADDF5"))
        .on_press_tx(action)
        .child(
            text(label.to_owned())
                .font(style(13.0, 600.0))
                .color(color("#242424")),
        )
}

pub fn view<A: Actions + 'static>(s: &UiState<A>) -> El {
    let call = &s.calls;
    let active = call.state.as_ref().is_some_and(CallState::is_active);
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
        let label = match call.state.as_ref() {
            Some(CallState::Connecting) => "Connecting…",
            Some(CallState::Connected { .. }) => "Connected",
            Some(CallState::Reconnecting { .. }) => "Reconnecting…",
            _ => "Call",
        };
        body = body.child(text(label).font(style(16.0, 600.0)).color(color("#242424")));
        let mut controls = row().gap(8.0);
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
            if camera {
                "Turn camera off"
            } else {
                "Turn camera on"
            },
            "call-camera",
            move |s, _| {
                s.actions.call_command(CallCommand::SetCamera(!camera));
            },
        ));
        controls = controls.child(button::<A>("Leave", "call-leave", |s, _| {
            s.actions.call_command(CallCommand::Hangup)
        }));
        controls = controls.child(button::<A>("Devices", "call-devices", |s, _| {
            s.calls.show_devices = !s.calls.show_devices;
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
                if s.calls.show_shares {
                    s.actions.call_command(CallCommand::RefreshShareSources);
                }
            }));
        }
        body = body.child(controls).child(
            text("People")
                .font(style(15.0, 700.0))
                .color(color("#242424")),
        );
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
        body = body.child(features);
        if let calling::CaptionState::Failed(error) = &call.captions {
            body = body.child(
                text(error.clone())
                    .font(style(13.0, 400.0))
                    .color(color("#C4314B")),
            );
        }
        if let Some(caption) = &call.caption {
            body = body.child(
                text(format!("{}: {}", caption.display_name, caption.text))
                    .font(style(14.0, 400.0))
                    .color(color("#242424"))
                    .wrap(),
            );
        }
        if let Some((mri, reaction)) = &call.reaction {
            let name = call
                .roster
                .iter()
                .find(|person| person.mri == *mri)
                .map(|person| person.display_name.as_str())
                .unwrap_or(mri);
            body = body.child(
                text(format!("Latest reaction: {name}: {}", reaction.name()))
                    .font(style(13.0, 400.0))
                    .color(color("#242424")),
            );
        }
        if call.show_devices {
            let mut devices = col().gap(4.0).child(
                text("Microphone")
                    .font(style(13.0, 700.0))
                    .color(color("#242424")),
            );
            for entry in call.devices.input_entries() {
                let choice = entry.choice;
                devices = devices.child(button::<A>(
                    &entry.label,
                    &format!("input-{}", entry.label),
                    move |s, _| {
                        s.actions
                            .call_command(CallCommand::SelectInput(choice.clone()))
                    },
                ));
            }
            devices = devices.child(
                text("Speaker")
                    .font(style(13.0, 700.0))
                    .color(color("#242424")),
            );
            for entry in call.devices.output_entries() {
                let choice = entry.choice;
                devices = devices.child(button::<A>(
                    &entry.label,
                    &format!("output-{}", entry.label),
                    move |s, _| {
                        s.actions
                            .call_command(CallCommand::SelectOutput(choice.clone()))
                    },
                ));
            }
            devices = devices.child(
                text("Camera")
                    .font(style(13.0, 700.0))
                    .color(color("#242424")),
            );
            for entry in calling::camera_entries(&call.cameras) {
                let choice = entry.choice;
                devices = devices.child(button::<A>(
                    &entry.label,
                    &format!("camera-{}", entry.label),
                    move |s, _| {
                        s.actions
                            .call_command(CallCommand::SelectCamera(choice.clone()))
                    },
                ));
            }
            body = body.child(devices);
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
            body = body.child(shares);
        }
        let mut surfaces: Vec<_> = call.surfaces.iter().collect();
        surfaces.sort_by_key(|(key, _)| format!("{key:?}"));
        for pair in surfaces.chunks(2) {
            let mut videos = row().w_full().gap(8.0);
            for &(key, surface) in pair {
                let label = match key {
                    VideoKey::Person(id) => call
                        .roster
                        .iter()
                        .find(|person| person.mri == *id)
                        .map(|person| person.display_name.as_str())
                        .unwrap_or(id),
                    VideoKey::Screen => "Shared screen",
                    VideoKey::LocalCamera => "You",
                    VideoKey::LocalScreen => "Your screen",
                };
                videos = videos.child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(4.0)
                        .child(
                            el().w_full()
                                .h(180.0)
                                .native_surface(surface, SurfaceFit::Fill),
                        )
                        .child(
                            text(label.to_owned())
                                .font(style(12.0, 400.0))
                                .color(color("#242424")),
                        ),
                );
            }
            body = body.child(videos);
        }
        let can_manage = call.roster.iter().any(|person| {
            person.mri == call.own_mri
                && (person.organizer || person.role == Some(calling::MeetingRole::Organizer))
        });
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
            let mut person_row = row().w_full().gap(8.0).center_y().child(
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
                } else if !person.muted {
                    let mri = person.mri.clone();
                    person_row = person_row.child(button::<A>(
                        "Mute",
                        &format!("mute-{mri}"),
                        move |s, _| {
                            s.actions
                                .call_command(CallCommand::MuteParticipant { mri: mri.clone() })
                        },
                    ));
                }
            }
            body = body.child(person_row);
        }
    } else if call.ring.is_none() {
        if let Some(CallState::Ended { reason }) = &call.state {
            body = body.child(
                text(format!("Call ended: {reason:?}"))
                    .font(style(13.0, 400.0))
                    .color(color("#616161")),
            );
        }
        body = body
            .child(
                text("Join a meeting")
                    .font(style(16.0, 600.0))
                    .color(color("#242424")),
            )
            .child(
                text("Your microphone starts muted")
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
            body = body.child(button::<A>(
                if call.video {
                    "Camera on"
                } else {
                    "Camera off"
                },
                "prejoin-camera",
                |s, _| s.calls.video = !s.calls.video,
            ));
            if !call.pending {
                body = body.child(button::<A>("Join now", "join-call", |s, _| {
                    if !s.calls.link.text.trim().is_empty() {
                        s.calls.pending = true;
                        s.calls.notice = None;
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
        fn send(&self, _: crate::types::SendTarget, _: String, _: Vec<teams_core::FileReference>) {}
        fn join_call(&self, url: &str, video: bool) {
            self.actions
                .borrow_mut()
                .push(format!("join {url} {video}"));
        }
        fn call_command(&self, command: CallCommand) {
            self.actions.borrow_mut().push(format!("{command:?}"));
        }
    }

    #[test]
    fn updates_track_controls_and_reset_for_next_call() {
        let mut view = CallView { pending: true, ..Default::default() };
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
        view.surfaces.insert(VideoKey::LocalCamera, NativeSurface::default());
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
            runtime.state.actions.actions.borrow().as_slice(),
            ["join https://teams.microsoft.com/meet/1234 false"]
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
