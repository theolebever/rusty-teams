pub use app_backend::notify::rules::*;
use calling::Progress;
use crate::call::{CallKind, CallModel};

pub fn in_call(server_in_call: bool, native_call: Option<&CallModel>) -> bool {
    server_in_call || native_call.is_some_and(CallModel::is_active)
}

pub fn should_ringback(native_call: Option<&CallModel>) -> bool {
    native_call.is_some_and(|model| {
        !model.incoming
            && model.kind != CallKind::Meeting
            && model.is_connecting()
            && matches!(model.progress, Some(Progress::Calling | Progress::Ringing))
    })
}


#[cfg(test)]
mod tests {
    use super::*;
    use calling::{CallState, EndReason};
    #[test]
    fn in_call_follows_server_presence_or_a_live_native_call() {
        let model = |state| {
            let mut model = CallModel::incoming("Bea", "8:orgid:b", "Bea");
            model.state = state;
            model
        };
        let since = std::time::Instant::now();
        assert!(!in_call(false, None));
        assert!(in_call(true, None));
        assert!(in_call(false, Some(&model(CallState::Connecting))));
        assert!(in_call(false, Some(&model(CallState::Connected { since }))));
        assert!(in_call(
            false,
            Some(&model(CallState::Reconnecting { since }))
        ));
        assert!(!in_call(false, Some(&model(CallState::Idle))));
        assert!(!in_call(
            false,
            Some(&model(CallState::Ended {
                reason: EndReason::LocalHangup
            }))
        ));
    }

    #[test]
    fn ringback_plays_only_while_an_outgoing_non_meeting_call_is_connecting() {
        let callees = [("8:orgid:a".to_owned(), "Ana".to_owned())];
        let outgoing = |progress: Option<Progress>, state: CallState| {
            let mut model = CallModel::people("Ana", &callees);
            model.progress = progress;
            model.state = state;
            model
        };
        let since = std::time::Instant::now();
        assert!(!should_ringback(None));
        assert!(!should_ringback(Some(&outgoing(
            None,
            CallState::Connecting
        ))));
        assert!(should_ringback(Some(&outgoing(
            Some(Progress::Calling),
            CallState::Connecting
        ))));
        assert!(should_ringback(Some(&outgoing(
            Some(Progress::Ringing),
            CallState::Connecting
        ))));
        assert!(!should_ringback(Some(&outgoing(
            Some(Progress::Ringing),
            CallState::Connected { since }
        ))));
        let ended = CallState::Ended {
            reason: EndReason::LocalHangup,
        };
        assert!(!should_ringback(Some(&outgoing(
            Some(Progress::Ringing),
            ended
        ))));
        let mut meeting = CallModel::meeting("Standup", false);
        meeting.progress = Some(Progress::Calling);
        assert!(!should_ringback(Some(&meeting)));
        let mut incoming = CallModel::incoming("Bea", "8:orgid:b", "Bea");
        incoming.progress = Some(Progress::Ringing);
        assert!(!should_ringback(Some(&incoming)));
    }

}
