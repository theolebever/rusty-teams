//! Meeting prejoin and active call controls.
use calling::{
    CallCommand, CallState, CallUpdate, EngineEvent, IncomingRing, MuteCommand, RosterEntry,
    VideoKey, VideoPicture,
};
use rune::native_surface::{ByteFrame, ColorInfo, Format};
use rune::tok::Family;
use rune::*;
use std::collections::HashMap;
use std::sync::Arc;

use crate::types::Actions;
use crate::ui::UiState;

pub struct CallView {
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
            let surface = self.surfaces.entry(key).or_insert_with(NativeSurface::new);
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
        fn open(&self, _: &str) {}
        fn send(&self, _: &str, _: String) {}
        fn post(&self, _: &str, _: String) {}
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
        let mut view = CallView::default();
        view.pending = true;
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
        runtime.frame(0.0);
        let (x, y) = runtime.find("join-call").expect("join button");
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.frame(0.0);
        assert_eq!(
            runtime.state.actions.actions.borrow().as_slice(),
            ["join https://teams.microsoft.com/meet/1234 false"]
        );
        runtime.update(|state, _| state.calls.apply(CallUpdate::State(CallState::Connecting)));
        runtime.frame(0.0);
        let (x, y) = runtime.find("call-leave").expect("leave button");
        runtime.pointer_down(x, y);
        runtime.pointer_up(x, y);
        runtime.frame(0.0);
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
