//! Persistent outgoing messages, using the shared delivery and retry policy.

use std::sync::Arc;
use crate::bridge::Shared;
use crate::types::{Actions, SendTarget, Update};
use crate::ui::{UiState, text, ts};
use rune::*;

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub id: String,
    pub target: SendTarget,
    pub text: String,
    pub error: Option<String>,
}

pub fn publish(shared: &Shared) {
    let result = shared.store.sidebar().and_then(|sidebar| {
        let mut rows = Vec::new();
        let ids = sidebar.chats.iter().map(|chat| chat.id.as_str())
            .chain(sidebar.teams.iter().flat_map(|team| team.channels.iter().map(|channel| channel.id.as_str())));
        for id in ids {
            for record in shared.store.outbox_for_conversation(id)? {
                let text = state::outbox::outgoing_of(&record).map(|outgoing| outgoing.text()).unwrap_or_else(|| "Unreadable saved message".into());
                rows.push(Row { id: record.id, target: SendTarget { conversation_id: record.conversation_id,
                    thread_root_id: record.thread_root_id, channel: record.target != store::OutboxTarget::Flat }, text, error: record.last_error });
            }
        }
        Ok(rows)
    });
    match result {
        Ok(rows) => shared.publish(Update::Outbox(rows)),
        Err(error) => shared.publish(Update::Status(format!("Could not read outbox: {error}"))),
    }
}

pub fn resume(shared: Arc<Shared>) {
    if shared.read_only { return; }
    app_backend::runtime::handle().spawn(async move {
        let _guard = shared.outbox_lock.lock().await;
        let Some(engine) = shared.engine() else { return };
        let records = match shared.store.sending_outbox() {
            Ok(records) => records,
            Err(error) => { shared.publish(Update::Status(format!("Could not read outbox: {error}"))); return; }
        };
        for record in records {
            let fresh = shared.fresh_outbox.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&record.id);
            let result = match state::outbox::outgoing_of(&record) {
                Some(outgoing) if fresh => state::outbox::send(&engine, &record, &outgoing).await,
                Some(outgoing) => state::outbox::resend(&engine, &record, &outgoing).await,
                None => Err("The saved message could not be read".into()),
            };
            let saved = match result {
                Ok(()) => shared.store.delete_outbox(&record.id),
                Err(error) => shared.store.mark_outbox_failed(&record.id, &error),
            };
            if let Err(error) = saved { shared.publish(Update::Status(format!("Could not update outbox: {error}"))); }
            publish(&shared);
            shared.publish_messages(&engine, &record.conversation_id);
        }
    });
}

pub fn retry(shared: Arc<Shared>, id: &str) {
    if shared.read_only { return; }
    let result = shared.store.failed_outbox_conversations().and_then(|ids| {
        for conversation in ids {
            for mut record in shared.store.outbox_for_conversation(&conversation)? {
                if record.id == id && record.state == store::OutboxState::Failed {
                    record.state = store::OutboxState::Sending;
                    record.last_error = None;
                    shared.store.put_outbox(&record)?;
                }
            }
        }
        Ok(())
    });
    if let Err(error) = result { shared.publish(Update::Status(format!("Could not retry message: {error}"))); return; }
    publish(&shared);
    resume(shared);
}

pub fn view<A: Actions + 'static>(s: &UiState<A>) -> El {
    let target = crate::ui::active_target(s);
    let mut list = col().key("outbox").w_full().gap(4.0).px(20.0);
    for row in s.outbox.iter().filter(|row| Some(&row.target) == target.as_ref()) {
        let mut message = col().key(format!("outbox-{}", row.id)).w_full().padding(8.0).gap(4.0)
            .child(text(row.text.clone()).font(ts(14.0, 400.0)).wrap())
            .child(text(row.error.clone().unwrap_or_else(|| "Sending…".into())).font(ts(12.0, 400.0)));
        if row.error.is_some() && !s.read_only {
            let id = row.id.clone();
            message = message.child(crate::screens::button("Retry", &format!("retry-{}", row.id), move |s: &mut UiState<A>, _| s.actions.retry_outbox(&id)));
        }
        list = list.child(message);
    }
    list
}

pub fn enqueue(shared: Arc<Shared>, target: SendTarget, composed: crate::types::Composed, files: Vec<teams_core::FileReference>) {
        if composed.text.trim().is_empty() && files.is_empty() && composed.gifs.is_empty() { return; }
        let outgoing = crate::composed::outgoing(&composed, &files);
        let outbox_id = state::outbox::new_outbox_id();
        let kind = if target.thread_root_id.is_some() { store::OutboxTarget::Thread } else if target.channel { store::OutboxTarget::Post } else { store::OutboxTarget::Flat };
        let mut fresh = shared.fresh_outbox.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        fresh.insert(outbox_id.clone());
        let result = if shared.read_only { Err("Read-only mode".to_owned()) } else {
            state::outbox::outbox_record(&outbox_id, &target.conversation_id, kind, target.thread_root_id.as_deref(), &outgoing, chrono::Utc::now())
                .ok_or_else(|| "Could not encode outgoing message".to_owned())
                .and_then(|record| shared.store.put_outbox(&record).map_err(|error| error.to_string()))
        };
        if result.is_err() { fresh.remove(&outbox_id); }
        drop(fresh);
        if let Err(error) = result {
            shared.publish(Update::ComposeFailed { target, composed, files, error });
            return;
        }
        crate::outbox::publish(&shared);
        crate::outbox::resume(shared.clone());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Fake(RefCell<Vec<String>>);
    impl Actions for Fake {
        fn open(&self, _: &str) {}
        fn send(&self, _: SendTarget, _: String, _: Vec<teams_core::FileReference>) {}
        fn retry_outbox(&self, id: &str) { self.0.borrow_mut().push(id.into()); }
    }

    #[test]
    fn failed_messages_offer_retry_only_in_their_own_conversation() {
        let mut state = UiState::new(Fake::default());
        state.selected = Some("chat".into());
        state.apply(Update::Outbox(vec![Row { id: "queued".into(), target: SendTarget {
            conversation_id: "chat".into(), thread_root_id: None, channel: false,
        }, text: "Keep me".into(), error: Some("offline".into()) }]));
        let mut rt = Runtime::new(state, |s, _| view(s), (800.0, 600.0), Env { system_dark: false, scale: 1.0 });
        rt.frame(0.0);
        let (x, y) = rt.find("retry-queued").expect("failed message offers retry");
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
        assert_eq!(*rt.state.actions.0.borrow(), ["queued"]);
        rt.update(|s, _| s.selected = Some("other".into()));
        rt.frame(1.0);
        assert!(rt.find("outbox-queued").is_none());
    }
}
