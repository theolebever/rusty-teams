use calling::{CallCommand, Callee, MeetingOptions, MeetingRole, Modality};

use super::actions::NOT_CONNECTED_NOTICE;
use super::features::TransferCandidate;
use super::meeting_info_dialog::MeetingInfoDialog;
use super::model::{CallKind, Tile, TileState};
use super::options_dialog::OptionsDialog;
use crate::Host;
use crate::app_state::{AppEvent, AppState};
use crate::runtime;

const INVITE_FAILED_NOTICE: &str = "Could not add that person";
const NO_PEER_NOTICE: &str = "This call cannot be extended";

/// Whether `open_meeting_info` should start a Graph fetch for the join link: only when the
/// calendar lookup had none, there's an engine to ask, and a thread to ask about.
fn should_fetch_meeting_join_url(needs_fetch: bool, has_engine: bool, has_thread_id: bool) -> bool {
    needs_fetch && has_engine && has_thread_id
}

impl AppState {
    pub fn toggle_call_people(&mut self, cx: &mut dyn Host) {
        let Some(call) = self.call.as_mut() else {
            return;
        };
        call.people_open = !call.people_open;
        if call.people_open {
            call.chat_open = false;
            call.transcript_open = false;
        }
        cx.emit(AppEvent::Call);
        cx.notify();
    }

    pub fn set_call_lock(&mut self, locked: bool, _cx: &mut dyn Host) {
        if self
            .call
            .as_ref()
            .is_some_and(|call| call.model.organizes_meeting())
        {
            self.send_call_command(CallCommand::SetLock(locked));
        }
    }

    pub fn set_call_role(&mut self, mri: String, role: MeetingRole, _cx: &mut dyn Host) {
        let allowed = self.call.as_ref().is_some_and(|call| {
            call.model.can_change_roles(&mri) && role != MeetingRole::Organizer
        });
        if allowed {
            self.send_call_command(CallCommand::SetRole { mri, role });
        }
    }

    pub fn set_call_attendee_modality(
        &mut self,
        mri: String,
        modality: Modality,
        allowed: bool,
        _cx: &mut dyn Host,
    ) {
        if self
            .call
            .as_ref()
            .is_some_and(|call| call.model.can_change_roles(&mri))
        {
            self.send_call_command(CallCommand::SetAttendeeModality {
                mri,
                modality,
                allowed,
            });
        }
    }

    pub fn set_call_attendee_restrictions(
        &mut self,
        mic_off: bool,
        camera_off: bool,
        _cx: &mut dyn Host,
    ) {
        if self
            .call
            .as_ref()
            .is_some_and(|call| call.model.organizes_meeting())
        {
            self.send_call_command(CallCommand::SetAttendeeRestrictions {
                mic_off,
                camera_off,
            });
        }
    }

    pub fn open_meeting_options(&mut self, cx: &mut dyn Host) {
        let Some(call) = self
            .call
            .as_mut()
            .filter(|call| call.model.organizes_meeting())
        else {
            return;
        };
        call.options_dialog = Some(OptionsDialog::loading());
        self.send_call_command(CallCommand::LoadMeetingOptions);
        cx.notify();
    }

    pub fn close_meeting_options(&mut self, cx: &mut dyn Host) {
        if let Some(call) = self
            .call
            .as_mut()
            .filter(|call| call.options_dialog.is_some())
        {
            call.options_dialog = None;
            cx.notify();
        }
    }

    pub fn edit_meeting_options(
        &mut self,
        edit: impl FnOnce(&mut MeetingOptions),
        cx: &mut dyn Host,
    ) {
        let Some(draft) = self
            .call
            .as_mut()
            .and_then(|call| call.options_dialog.as_mut())
            .and_then(|dialog| dialog.draft.as_mut())
        else {
            return;
        };
        edit(draft);
        cx.notify();
    }

    pub fn save_meeting_options(&mut self, cx: &mut dyn Host) {
        let writes = self
            .call
            .as_mut()
            .and_then(|call| call.options_dialog.as_mut())
            .and_then(OptionsDialog::begin_save);
        if let Some(writes) = writes {
            self.send_call_command(CallCommand::SaveMeetingOptions(writes));
            cx.notify();
        }
    }

    /// Opens "Meeting info": the join link and dial-in from the calendar event behind this
    /// meeting. Available to everyone in the call, unlike organizer-only "Meeting options...".
    pub fn open_meeting_info(&mut self, cx: &mut dyn Host) {
        let Some(call) = self.call.as_ref() else {
            return;
        };
        let call_id = call.id;
        let thread_id = call.chat_thread().map(str::to_owned);
        let (join_url, dial_in) = thread_id
            .as_deref()
            .and_then(|id| self.calendar_event_for_thread(id))
            .map_or((None, None), |event| {
                (event.join_url.clone(), event.dial_in.clone())
            });
        let needs_fetch = join_url.is_none();
        let Some(call) = self.call.as_mut() else {
            return;
        };
        call.meeting_info = Some(MeetingInfoDialog::from_calendar(join_url, dial_in));
        cx.notify();
        let has_engine = self.engine.is_some();
        if !should_fetch_meeting_join_url(needs_fetch, has_engine, thread_id.is_some()) {
            return;
        }
        let (Some(engine), Some(thread_id)) = (self.engine.clone(), thread_id) else {
            return;
        };
        let receiver = runtime::spawn(async move { engine.meeting_join_url(&thread_id).await });
        let this = cx.handle();
        cx.spawn(Box::pin(async move {
            let url = receiver.await.ok().and_then(Result::ok).flatten();
            this.update(move |state, cx| state.apply_meeting_join_url(call_id, url, cx))
                .await;
        }));
    }

    /// Applies a join-link fetch started by `open_meeting_info`, unless the user has since
    /// left this call or opened a different one (`call_id` is stale).
    fn apply_meeting_join_url(&mut self, call_id: u64, url: Option<String>, cx: &mut dyn Host) {
        if let Some(dialog) = self
            .call
            .as_mut()
            .filter(|call| call.id == call_id)
            .and_then(|call| call.meeting_info.as_mut())
        {
            dialog.join_url_loaded(url);
            cx.notify();
        }
    }

    /// Closes "Meeting info", opened by `open_meeting_info`.
    pub fn close_meeting_info(&mut self, cx: &mut dyn Host) {
        if let Some(call) = self.call.as_mut() {
            if call.meeting_info.take().is_some() {
                cx.notify();
            }
        }
    }

    pub fn invite_candidates(&self) -> Vec<TransferCandidate> {
        let present: Vec<&str> = self
            .call
            .as_ref()
            .map(|call| {
                call.model
                    .tiles
                    .iter()
                    .map(|tile| tile.mri.as_str())
                    .collect()
            })
            .unwrap_or_default();
        self.transfer_candidates()
            .into_iter()
            .filter(|candidate| !present.contains(&candidate.mri.as_str()))
            .collect()
    }

    pub fn open_invite_picker(&mut self, cx: &mut dyn Host) {
        if self
            .call
            .as_ref()
            .is_some_and(|call| call.model.can_invite())
        {
            self.transfer_picker_open = true;
            cx.notify();
        }
    }

    fn drop_invited_tile(&mut self, mri: &str) {
        if let Some(call) = self.call.as_mut() {
            call.model
                .tiles
                .retain(|tile| tile.mri != mri || tile.state != TileState::Invited);
        }
    }

    pub fn invite_call_person(&mut self, target: TransferCandidate, cx: &mut dyn Host) {
        self.close_transfer_picker(cx);
        let Some(call) = self.call.as_mut().filter(|call| call.model.can_invite()) else {
            return;
        };
        let callee = Callee {
            mri: target.mri.clone(),
            display_name: target.name.clone(),
        };
        if !call.model.tiles.iter().any(|tile| tile.mri == callee.mri) {
            call.model
                .tiles
                .push(Tile::invited(&callee.mri, &callee.display_name));
        }
        let one_to_one = call.model.kind == CallKind::Direct;
        let peer_user_id = call
            .model
            .tiles
            .iter()
            .find(|tile| tile.mri != callee.mri)
            .and_then(|tile| tile.user_id.clone());
        let call_id = call.id;
        cx.emit(AppEvent::Call);
        cx.notify();
        if !one_to_one || self.mode.demo {
            self.send_call_command(CallCommand::Invite {
                callees: vec![callee],
                group_thread: None,
            });
            return;
        }
        let (Some(engine), Some(peer_user_id), Some(invited_user_id)) =
            (self.engine.clone(), peer_user_id, target.user_id.clone())
        else {
            let notice = if self.engine.is_none() {
                NOT_CONNECTED_NOTICE
            } else {
                NO_PEER_NOTICE
            };
            self.drop_invited_tile(&callee.mri);
            self.raise_notice(notice.to_owned(), None, cx);
            return;
        };
        let receiver = runtime::spawn(async move {
            engine
                .create_group(&[peer_user_id, invited_user_id], None)
                .await
        });
        let invited_mri = callee.mri.clone();
        let this = cx.handle();
        cx.spawn(Box::pin(async move {
            let created = receiver.await.ok().and_then(Result::ok);
            this.update(move |state, cx| match created {
                Some(thread_id) if state.call.as_ref().is_some_and(|call| call.id == call_id) => {
                    state.send_call_command(CallCommand::Invite {
                        callees: vec![callee],
                        group_thread: Some(thread_id),
                    });
                }
                _ => {
                    if state.call.as_ref().is_some_and(|call| call.id == call_id) {
                        state.drop_invited_tile(&invited_mri);
                    }
                    state.raise_notice(INVITE_FAILED_NOTICE.to_owned(), None, cx);
                }
            })
            .await;
        }));
    }
}

#[cfg(all(test, feature = "gpui"))]
mod tests {
    use std::sync::Arc;
    use std::time::Instant;

    use calling::{
        CallCommand, CallControl, CallState, CallUpdate, MeetingCapability, MeetingOptions,
        MeetingRole, Modality, RosterEntry, call_channel,
    };
    use gpui_kit::{AppContext as _, Entity, TestAppContext};
    use serde_json::json;
    use store::Store;

    use super::should_fetch_meeting_join_url;
    use crate::app_state::{AppState, Mode};
    use crate::call::{ActiveCall, CallModel, TransferCandidate};

    fn app_with_meeting(
        cx: &mut TestAppContext,
        organizer: bool,
    ) -> (Entity<AppState>, CallControl) {
        cx.update(gpui_kit::init);
        let store = Arc::new(Store::open_in_memory().unwrap());
        let app = cx.update(|cx| cx.new(|_| AppState::new(store, Mode::default())));
        let (handle, control) = call_channel();
        let mut model = CallModel::meeting("Standup", organizer);
        model.apply(CallUpdate::State(CallState::Connected {
            since: Instant::now(),
        }));
        model.apply(CallUpdate::OwnIdentity {
            mri: "8:orgid:me".into(),
        });
        model.apply(CallUpdate::Roster(vec![RosterEntry {
            mri: "8:orgid:a".into(),
            display_name: "Ana".into(),
            role: Some(MeetingRole::Attendee),
            ..RosterEntry::default()
        }]));
        cx.update(|cx| {
            app.update(cx, |state, _| {
                state.call = Some(ActiveCall::new(
                    7,
                    model,
                    handle.commands,
                    Some("19:meeting_x@thread.v2".into()),
                    handle.video,
                ));
            })
        });
        (app, control)
    }

    fn options() -> MeetingOptions {
        let option =
            |name: &str, value: serde_json::Value| json!({"name": name, "currentValue": value});
        MeetingOptions::from_response(&json!({"options": [
            option("AutoAdmittedUsers", json!("EveryoneInCompany")),
            option("PresenterOption", json!("Everyone")),
            option("AttendeeRestrictions", json!("UnRestricted")),
            option("AllowMeetingChat", json!("Enabled")),
            option("AllowTeamsMeetingReactions", json!(true)),
            option("AutoRecordAndTranscribeMode", json!("Off")),
        ]}))
        .unwrap()
    }

    #[gpui_kit::test]
    fn the_people_panel_and_the_chat_panel_exclude_each_other(cx: &mut TestAppContext) {
        let (app, _control) = app_with_meeting(cx, true);
        let open = |app: &Entity<AppState>, cx: &mut TestAppContext| {
            cx.update(|cx| {
                let call = app.read(cx).call.as_ref().unwrap();
                (call.people_open, call.chat_open)
            })
        };
        cx.update(|cx| app.update(cx, |state, cx| state.toggle_call_chat(cx)));
        assert_eq!(open(&app, cx), (false, true));
        cx.update(|cx| app.update(cx, |state, cx| state.toggle_call_people(cx)));
        assert_eq!(open(&app, cx), (true, false));
        cx.update(|cx| app.update(cx, |state, cx| state.toggle_call_chat(cx)));
        assert_eq!(open(&app, cx), (false, true));
        cx.update(|cx| app.update(cx, |state, cx| state.toggle_call_people(cx)));
        cx.update(|cx| app.update(cx, |state, cx| state.toggle_call_people(cx)));
        assert_eq!(open(&app, cx), (false, false));
    }

    #[gpui_kit::test]
    fn organizer_actions_become_commands_and_attendees_send_nothing(cx: &mut TestAppContext) {
        let (attendee, mut attendee_control) = app_with_meeting(cx, false);
        cx.update(|cx| {
            attendee.update(cx, |state, cx| {
                state.set_call_lock(true, cx);
                state.set_call_role("8:orgid:a".into(), MeetingRole::Presenter, cx);
                state.set_call_attendee_modality("8:orgid:a".into(), Modality::Mic, false, cx);
                state.set_call_attendee_restrictions(true, false, cx);
                state.open_meeting_options(cx);
            })
        });
        assert_eq!(attendee_control.try_recv_command(), None);
        let (organizer, mut control) = app_with_meeting(cx, true);
        cx.update(|cx| {
            organizer.update(cx, |state, cx| {
                state.set_call_lock(true, cx);
                state.set_call_role("8:orgid:a".into(), MeetingRole::Presenter, cx);
                state.set_call_role("8:orgid:a".into(), MeetingRole::Organizer, cx);
                state.set_call_attendee_modality("8:orgid:a".into(), Modality::Camera, false, cx);
                state.set_call_attendee_restrictions(true, true, cx);
            })
        });
        assert_eq!(control.try_recv_command(), Some(CallCommand::SetLock(true)));
        assert_eq!(
            control.try_recv_command(),
            Some(CallCommand::SetRole {
                mri: "8:orgid:a".into(),
                role: MeetingRole::Presenter
            })
        );
        assert_eq!(
            control.try_recv_command(),
            Some(CallCommand::SetAttendeeModality {
                mri: "8:orgid:a".into(),
                modality: Modality::Camera,
                allowed: false
            })
        );
        assert_eq!(
            control.try_recv_command(),
            Some(CallCommand::SetAttendeeRestrictions {
                mic_off: true,
                camera_off: true
            })
        );
        assert_eq!(control.try_recv_command(), None);
    }

    #[gpui_kit::test]
    fn the_capability_update_marks_the_meeting_as_locked(cx: &mut TestAppContext) {
        let (app, _control) = app_with_meeting(cx, true);
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.apply_call_update(
                    7,
                    CallUpdate::Capability(MeetingCapability {
                        locked: true,
                        ..MeetingCapability::default()
                    }),
                    cx,
                )
            })
        });
        assert!(cx.update(|cx| app.read(cx).call.as_ref().unwrap().model.capability.locked));
    }

    #[gpui_kit::test]
    fn the_options_dialog_loads_saves_only_changes_and_keeps_errors_inline(
        cx: &mut TestAppContext,
    ) {
        let (app, mut control) = app_with_meeting(cx, true);
        cx.update(|cx| app.update(cx, |state, cx| state.open_meeting_options(cx)));
        assert_eq!(
            control.try_recv_command(),
            Some(CallCommand::LoadMeetingOptions)
        );
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.apply_call_update(
                    7,
                    CallUpdate::MeetingOptions(Err("The options could not be read".into())),
                    cx,
                )
            })
        });
        let error = cx.update(|cx| {
            app.read(cx)
                .call
                .as_ref()
                .unwrap()
                .options_dialog
                .as_ref()
                .unwrap()
                .error
                .clone()
        });
        assert_eq!(error.as_deref(), Some("The options could not be read"));
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.apply_call_update(7, CallUpdate::MeetingOptions(Ok(options())), cx);
                state.save_meeting_options(cx);
                state.edit_meeting_options(|draft| draft.meeting_chat = "InMeeting".into(), cx);
                state.save_meeting_options(cx);
            })
        });
        let Some(CallCommand::SaveMeetingOptions(writes)) = control.try_recv_command() else {
            panic!("a save command");
        };
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].name, "AllowMeetingChat");
        assert_eq!(control.try_recv_command(), None);
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.apply_call_update(
                    7,
                    CallUpdate::MeetingOptionsSaved(Err(
                        "the service refused AllowMeetingChat".into()
                    )),
                    cx,
                )
            })
        });
        assert!(cx.update(|cx| app.read(cx).call.as_ref().unwrap().options_dialog.is_some()));
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.apply_call_update(7, CallUpdate::MeetingOptionsSaved(Ok(options())), cx)
            })
        });
        assert!(cx.update(|cx| app.read(cx).call.as_ref().unwrap().options_dialog.is_none()));
    }

    #[gpui_kit::test]
    fn inviting_into_a_meeting_sends_the_person_and_shows_them_as_ringing(cx: &mut TestAppContext) {
        let (app, mut control) = app_with_meeting(cx, true);
        let candidate = TransferCandidate {
            mri: "8:orgid:bo".into(),
            name: "Bo".into(),
            user_id: Some("bo".into()),
            chat_id: "19:bo".into(),
        };
        cx.update(|cx| app.update(cx, |state, cx| state.invite_call_person(candidate, cx)));
        let Some(CallCommand::Invite {
            callees,
            group_thread,
        }) = control.try_recv_command()
        else {
            panic!("an invite command");
        };
        assert_eq!(callees.len(), 1);
        assert_eq!(callees[0].mri, "8:orgid:bo");
        assert_eq!(group_thread, None);
        let ringing = cx.update(|cx| {
            let tiles = &app.read(cx).call.as_ref().unwrap().model.tiles;
            tiles
                .iter()
                .any(|tile| tile.mri == "8:orgid:bo" && tile.invited)
        });
        assert!(ringing);
    }

    fn calendar_event_with_dial_in() -> chatsvc::CalendarEvent {
        chatsvc::CalendarEvent {
            thread_id: Some("19:meeting_x@thread.v2".into()),
            join_url: Some("https://teams.microsoft.com/l/meetup-join/x".into()),
            dial_in: Some(chatsvc::DialIn {
                toll_number: "+49 30 123456".into(),
                conference_id: "987654321".into(),
            }),
            ..Default::default()
        }
    }

    #[gpui_kit::test]
    fn meeting_info_uses_the_matching_calendar_event_with_no_fetch(cx: &mut TestAppContext) {
        let (app, _control) = app_with_meeting(cx, false);
        cx.update(|cx| {
            app.update(cx, |state, _| {
                state.calendar_events = vec![calendar_event_with_dial_in()];
            })
        });
        cx.update(|cx| app.update(cx, |state, cx| state.open_meeting_info(cx)));
        let dialog = cx.update(|cx| app.read(cx).call.as_ref().unwrap().meeting_info.clone());
        let dialog = dialog.expect("meeting info open");
        assert!(!dialog.loading);
        assert_eq!(
            dialog.join_url.as_deref(),
            Some("https://teams.microsoft.com/l/meetup-join/x")
        );
        assert_eq!(
            dialog.copy_text().as_deref(),
            Some(
                "Microsoft Teams meeting\nJoin: https://teams.microsoft.com/l/meetup-join/x\n\nDial in by phone\n+49 30 123456,,987654321#\nPhone conference ID: 987654321#"
            )
        );
    }

    #[gpui_kit::test]
    fn meeting_info_without_a_matching_event_has_no_dial_in(cx: &mut TestAppContext) {
        let (app, _control) = app_with_meeting(cx, false);
        cx.update(|cx| app.update(cx, |state, cx| state.open_meeting_info(cx)));
        let dialog = cx.update(|cx| app.read(cx).call.as_ref().unwrap().meeting_info.clone());
        let dialog = dialog.expect("meeting info open");
        // No calendar match and no engine in this test harness: stays loading, like a Meet
        // Now with no invite until a join-link fetch lands (or never does).
        assert!(dialog.loading);
        assert_eq!(dialog.join_url, None);
        assert_eq!(dialog.dial_in, None);
    }

    #[gpui_kit::test]
    fn close_meeting_info_clears_the_dialog(cx: &mut TestAppContext) {
        let (app, _control) = app_with_meeting(cx, false);
        cx.update(|cx| app.update(cx, |state, cx| state.open_meeting_info(cx)));
        assert!(cx.update(|cx| app.read(cx).call.as_ref().unwrap().meeting_info.is_some()));
        cx.update(|cx| app.update(cx, |state, cx| state.close_meeting_info(cx)));
        assert!(cx.update(|cx| app.read(cx).call.as_ref().unwrap().meeting_info.is_none()));
    }

    #[gpui_kit::test]
    fn apply_meeting_join_url_ignores_a_stale_call_id(cx: &mut TestAppContext) {
        let (app, _control) = app_with_meeting(cx, false);
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.open_meeting_info(cx);
                state.apply_meeting_join_url(
                    999,
                    Some("https://teams.microsoft.com/l/meetup-join/stale".into()),
                    cx,
                );
            })
        });
        let dialog = cx.update(|cx| app.read(cx).call.as_ref().unwrap().meeting_info.clone());
        assert_eq!(dialog.unwrap().join_url, None);
    }

    #[gpui_kit::test]
    fn apply_meeting_join_url_updates_the_matching_call(cx: &mut TestAppContext) {
        let (app, _control) = app_with_meeting(cx, false);
        cx.update(|cx| {
            app.update(cx, |state, cx| {
                state.open_meeting_info(cx);
                state.apply_meeting_join_url(
                    7,
                    Some("https://teams.microsoft.com/l/meetup-join/late".into()),
                    cx,
                );
            })
        });
        let dialog = cx.update(|cx| app.read(cx).call.as_ref().unwrap().meeting_info.clone());
        let dialog = dialog.unwrap();
        assert_eq!(
            dialog.join_url.as_deref(),
            Some("https://teams.microsoft.com/l/meetup-join/late")
        );
        assert!(!dialog.loading);
    }

    #[test]
    fn should_fetch_meeting_join_url_needs_all_three_conditions() {
        assert!(should_fetch_meeting_join_url(true, true, true));
        assert!(!should_fetch_meeting_join_url(false, true, true));
        assert!(!should_fetch_meeting_join_url(true, false, true));
        assert!(!should_fetch_meeting_join_url(true, true, false));
    }
}
