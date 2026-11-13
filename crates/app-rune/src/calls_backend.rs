//! Owns the call handle and forwards its updates to the Rune window.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use app_backend::launcher::CallLauncher;
use calling::{
    AcceptMode, CallCommand, CallHandle, CallSpec, CallState, CallUpdate, MeetingTarget, VideoKey,
    VideoPicture,
};
use teams_core::{MeetingLink, parse_meeting_link};
use tokio::sync::mpsc::UnboundedSender;

struct State {
    launcher: Option<Arc<CallLauncher>>,
    commands: Option<UnboundedSender<CallCommand>>,
    joining: bool,
    generation: u64,
    frames: HashMap<VideoKey, Arc<VideoPicture>>,
    video_pending: bool,
    retry: Option<(CallSpec, bool)>,
    prejoin: Vec<CallCommand>,
}

pub struct Calls {
    state: Arc<Mutex<State>>,
    publish: Arc<dyn Fn(CallUpdate) + Send + Sync>,
    video: Arc<dyn Fn() + Send + Sync>,
    read_only: bool,
}

impl Calls {
    pub fn new(
        read_only: bool,
        publish: impl Fn(CallUpdate) + Send + Sync + 'static,
        video: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                launcher: None,
                commands: None,
                joining: false,
                generation: 0,
                frames: HashMap::new(),
                video_pending: false,
                retry: None,
                prejoin: Vec::new(),
            })),
            publish: Arc::new(publish),
            video: Arc::new(video),
            read_only,
        }
    }

    pub fn set_launcher(&self, launcher: Arc<CallLauncher>) {
        self.state.lock().expect("call state lock").launcher = Some(launcher);
    }

    pub fn start(&self, spec: CallSpec) {
        self.start_with_video(spec, false);
    }

    pub fn retry(&self) -> Option<(CallSpec, bool)> {
        self.state.lock().expect("call state lock").retry.clone()
    }

    pub fn start_with_video(&self, spec: CallSpec, video: bool) {
        if self.read_only {
            return;
        }
        let handle = {
            let mut state = self.state.lock().expect("call state lock");
            if state.joining || state.commands.is_some() {
                return;
            }
            let Some(launcher) = state.launcher.clone() else {
                (self.publish)(CallUpdate::Notice("Calling is not connected yet".into()));
                return;
            };
            state.retry = Some((spec.clone(), video));
            let handle = if video {
                launcher.start_video_call(spec)
            } else {
                launcher.start_call(spec)
            };
            state.prejoin.clear();
            state.commands = Some(handle.commands.clone());
            handle
        };
        app_backend::runtime::handle().spawn(forward(
            handle,
            self.state.clone(),
            self.publish.clone(),
            self.video.clone(),
        ));
    }

    pub fn join(&self, url: &str, video: bool) {
        if self.read_only {
            return;
        }
        let Some(link) = parse_meeting_link(url) else {
            self.state.lock().expect("call state lock").prejoin.clear();
            (self.publish)(CallUpdate::Notice("Could not find that meeting".into()));
            return;
        };
        let (launcher, generation) = {
            let mut state = self.state.lock().expect("call state lock");
            if state.joining || state.commands.is_some() {
                return;
            }
            let Some(launcher) = state.launcher.clone() else {
                state.prejoin.clear();
                (self.publish)(CallUpdate::Notice("Calling is not connected yet".into()));
                return;
            };
            state.joining = true;
            (launcher, state.generation)
        };
        let state = self.state.clone();
        let publish = self.publish.clone();
        let video_frames = self.video.clone();
        app_backend::runtime::handle().spawn(async move {
            let target = match link {
                MeetingLink::Thread(meeting) => Ok(MeetingTarget {
                    thread_id: meeting.thread_id,
                    tenant_id: meeting.tenant_id,
                    organizer_id: meeting.organizer_id,
                    meeting_data: None,
                }),
                MeetingLink::Code(code) => launcher.resolve_meeting(&code.meeting_data()).await,
            };
            let target = match target {
                Ok(target) => target,
                Err(error) => {
                    let mut state = state.lock().expect("call state lock");
                    if state.generation != generation {
                        return;
                    }
                    state.joining = false;
                    state.prejoin.clear();
                    publish(CallUpdate::Notice(error));
                    return;
                }
            };
            let spec = CallSpec::Meeting(target);
            let handle = {
                let mut state = state.lock().expect("call state lock");
                if state.generation != generation {
                    return;
                }
                state.retry = Some((spec.clone(), video));
                let handle = if video {
                    launcher.start_video_call(spec)
                } else {
                    launcher.start_call(spec)
                };
                state.commands = Some(handle.commands.clone());
                for command in state.prejoin.drain(..) {
                    let _ = handle.commands.send(command);
                }
                state.joining = false;
                handle
            };
            forward(handle, state, publish, video_frames).await;
        });
    }

    pub fn accept(&self, ring_id: u64, video: bool) {
        if self.read_only {
            return;
        }
        let (launcher, generation) = {
            let mut state = self.state.lock().expect("call state lock");
            if state.joining || state.commands.is_some() {
                return;
            }
            let Some(launcher) = state.launcher.clone() else {
                (self.publish)(CallUpdate::Notice("Calling is not connected yet".into()));
                return;
            };
            state.joining = true;
            state.retry = None;
            state.prejoin.clear();
            (launcher, state.generation)
        };
        let state = self.state.clone();
        let publish = self.publish.clone();
        let frames = self.video.clone();
        app_backend::runtime::handle().spawn(async move {
            let mode = if video {
                AcceptMode::Video
            } else {
                AcceptMode::Audio
            };
            match launcher.accept_ring(ring_id, mode).await {
                Some(handle) => {
                    {
                        let mut state = state.lock().expect("call state lock");
                        if state.generation != generation {
                            let _ = handle.commands.send(CallCommand::Hangup);
                            return;
                        }
                        state.commands = Some(handle.commands.clone());
                        state.joining = false;
                        // Prejoin choices queued while answering belong to no meeting; drop them.
                        state.prejoin.clear();
                    }
                    forward(handle, state, publish, frames).await
                }
                None => {
                    let mut state = state.lock().expect("call state lock");
                    if state.generation != generation {
                        return;
                    }
                    state.joining = false;
                    publish(CallUpdate::Notice("Could not answer the call".into()));
                }
            }
        });
    }

    pub fn decline(&self, ring_id: u64) {
        if self.read_only {
            return;
        }
        if let Some(launcher) = &self.state.lock().expect("call state lock").launcher {
            launcher.decline_ring(ring_id);
        }
    }

    pub fn command(&self, command: CallCommand) {
        if self.read_only {
            return;
        }
        if command == CallCommand::Hangup {
            self.hangup();
            return;
        }
        let mut state = self.state.lock().expect("call state lock");
        if let Some(commands) = &state.commands {
            if commands.send(command).is_err() {
                (self.publish)(CallUpdate::Notice("Call controls are unavailable".into()));
            }
        } else if matches!(
            command,
            CallCommand::SelectInput(_)
                | CallCommand::SelectOutput(_)
                | CallCommand::SelectCamera(_)
                | CallCommand::SetBackground(_)
                | CallCommand::SetCameraEffects(_)
                | CallCommand::Mute(_)
        ) {
            state
                .prejoin
                .retain(|old| std::mem::discriminant(old) != std::mem::discriminant(&command));
            state.prejoin.push(command);
        }
    }

    pub fn hangup(&self) {
        let mut state = self.state.lock().expect("call state lock");
        state.generation = state.generation.wrapping_add(1);
        state.joining = false;
        state.prejoin.clear();
        if let Some(commands) = &state.commands {
            let _ = commands.send(CallCommand::Hangup);
        }
    }

    pub fn take_video(&self) -> Vec<(VideoKey, Arc<VideoPicture>)> {
        let mut state = self.state.lock().expect("call state lock");
        state.video_pending = false;
        state.frames.drain().collect()
    }
}

async fn forward(
    mut handle: CallHandle,
    state: Arc<Mutex<State>>,
    publish: Arc<dyn Fn(CallUpdate) + Send + Sync>,
    frames: Arc<dyn Fn() + Send + Sync>,
) {
    publish(CallUpdate::State(CallState::Connecting));
    while let Some(update) = handle.updates.recv().await {
        if matches!(update, CallUpdate::VideoReady) {
            let notify = {
                let mut state = state.lock().expect("call state lock");
                for (key, picture) in handle.video.drain() {
                    if state.frames.len() < 16 || state.frames.contains_key(&key) {
                        state.frames.insert(key, picture);
                    }
                }
                let notify = !state.video_pending && !state.frames.is_empty();
                state.video_pending |= notify;
                notify
            };
            if notify {
                frames();
            }
            continue;
        }
        if let CallUpdate::VideoStopped(key) = &update {
            state.lock().expect("call state lock").frames.remove(key);
        }
        let ended = matches!(update, CallUpdate::State(CallState::Ended { .. }));
        if ended {
            let mut state = state.lock().expect("call state lock");
            state.commands = None;
            state.frames.clear();
            state.video_pending = false;
        }
        publish(update);
        if ended {
            return;
        }
    }
    {
        let mut state = state.lock().expect("call state lock");
        state.commands = None;
        state.frames.clear();
        state.video_pending = false;
    }
    publish(CallUpdate::State(CallState::Ended {
        reason: calling::EndReason::Dropped,
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prejoin_settings_coalesce_and_invalid_joins_clear_them() {
        let calls = Calls::new(false, |_| {}, || {});
        calls.command(CallCommand::SelectInput(
            calling::DeviceChoice::SystemDefault,
        ));
        calls.command(CallCommand::SelectInput(calling::DeviceChoice::Device(
            "mic".into(),
        )));
        calls.command(CallCommand::Mute(calling::MuteCommand::Unmute));
        calls.command(CallCommand::SendReaction(calling::Reaction::Heart));
        assert_eq!(
            calls.state.lock().unwrap().prejoin,
            vec![
                CallCommand::SelectInput(calling::DeviceChoice::Device("mic".into())),
                CallCommand::Mute(calling::MuteCommand::Unmute),
            ]
        );
        calls.join("not a meeting link", false);
        assert!(calls.state.lock().unwrap().prejoin.is_empty());
        calls.command(CallCommand::Mute(calling::MuteCommand::Unmute));
        calls.command(CallCommand::Hangup);
        assert!(calls.state.lock().unwrap().prejoin.is_empty());
        assert_eq!(calls.state.lock().unwrap().generation, 1);
    }

    #[tokio::test]
    async fn forwards_call_updates_and_releases_handle_after_end() {
        let (handle, control) = calling::call_channel();
        let state = Arc::new(Mutex::new(State {
            launcher: None,
            commands: Some(handle.commands.clone()),
            joining: false,
            generation: 0,
            frames: HashMap::new(),
            video_pending: false,
            retry: None,
            prejoin: Vec::new(),
        }));
        let (updates, mut received) = tokio::sync::mpsc::unbounded_channel();
        let publish: Arc<dyn Fn(CallUpdate) + Send + Sync> = Arc::new(move |update| {
            let _ = updates.send(update);
        });
        let frames = Arc::new(|| {});
        let task = tokio::spawn(forward(handle, state.clone(), publish, frames));
        assert!(matches!(
            received.recv().await,
            Some(CallUpdate::State(CallState::Connecting))
        ));
        assert!(state.lock().expect("call state lock").commands.is_some());
        control.send(CallUpdate::Muted(false));
        assert!(matches!(
            received.recv().await,
            Some(CallUpdate::Muted(false))
        ));
        let peer = VideoKey::Person("8:orgid:peer".into());
        state
            .lock()
            .expect("call state lock")
            .frames
            .insert(peer.clone(), Arc::new(VideoPicture::solid(2, 2, 1, 0, 0)));
        control.send(CallUpdate::VideoStopped(peer.clone()));
        assert!(matches!(
            received.recv().await,
            Some(CallUpdate::VideoStopped(key)) if key == peer
        ));
        assert!(state.lock().expect("call state lock").frames.is_empty());
        control.send(CallUpdate::State(CallState::Ended {
            reason: calling::EndReason::LocalHangup,
        }));
        assert!(matches!(
            received.recv().await,
            Some(CallUpdate::State(CallState::Ended { .. }))
        ));
        task.await.expect("forward task completes");
        assert!(state.lock().expect("call state lock").commands.is_none());
    }

    #[tokio::test]
    async fn video_notifications_coalesce_until_the_ui_drains_frames() {
        let (handle, control) = calling::call_channel();
        let video = handle.video.clone();
        let state = Arc::new(Mutex::new(State {
            launcher: None,
            commands: Some(handle.commands.clone()),
            joining: false,
            generation: 0,
            frames: HashMap::new(),
            video_pending: false,
            retry: None,
            prejoin: Vec::new(),
        }));
        let (wakes, mut received) = tokio::sync::mpsc::unbounded_channel();
        let frames = Arc::new(move || {
            let _ = wakes.send(());
        });
        let (updates, mut forwarded) = tokio::sync::mpsc::unbounded_channel();
        let publish = Arc::new(move |update: CallUpdate| {
            let _ = updates.send(update);
        });
        let task = tokio::spawn(forward(handle, state.clone(), publish, frames));
        forwarded.recv().await.expect("call started");
        let key = VideoKey::LocalCamera;
        video.publish(key.clone(), VideoPicture::solid(2, 2, 1, 0, 0));
        received.recv().await.expect("first frame wakes the UI");
        video.publish(key.clone(), VideoPicture::solid(2, 2, 2, 0, 0));
        control.send(CallUpdate::Notice("barrier".into()));
        forwarded.recv().await.expect("second frame processed");
        assert!(
            received.try_recv().is_err(),
            "a stalled UI gets only one wake"
        );
        {
            let state = state.lock().expect("call state lock");
            assert_eq!(state.frames.len(), 1);
            assert_eq!(state.frames[&key].bgra[0], 2);
        }
        control.send(CallUpdate::State(CallState::Ended {
            reason: calling::EndReason::LocalHangup,
        }));
        task.await.expect("forward task completes");
    }
}
