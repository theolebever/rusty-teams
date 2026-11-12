use gpui_kit::*;

use crate::activity::ActivityModel;
use crate::activity::feed::Feed;
use crate::agent_api;
use crate::app_state::{AppEvent, AppState};

/// GPUI entity around [`ActivityModel`]: follows state events, agent
/// proposals and whether the main window is in the foreground.
pub struct ActivityCenter {
    app: Entity<AppState>,
    main_window: AnyWindowHandle,
    model: ActivityModel,
    _subscription: Subscription,
    _proposal_observation: Option<Subscription>,
}

impl ActivityCenter {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let model = app.update(cx, |state, cx| ActivityModel::new(state, cx));
        let subscription = cx.subscribe(&app, |this, _, event: &AppEvent, cx| {
            this.on_app_event(event, cx);
        });
        let proposal_observation =
            agent_api::handle(cx).map(|api| cx.observe(&api, |this, _, cx| this.on_proposals(cx)));
        ActivityCenter {
            app,
            main_window: window.window_handle(),
            model,
            _subscription: subscription,
            _proposal_observation: proposal_observation,
        }
    }

    pub fn feed(&self) -> &Feed {
        self.model.feed()
    }

    pub fn mark_read(&mut self, id: &str, cx: &mut Context<Self>) {
        self.app.update(cx, |state, cx| self.model.mark_read(id, state, cx));
        cx.notify();
    }

    pub fn mark_all_read(&mut self, cx: &mut Context<Self>) {
        agent_api::mark_all_proposals_seen(cx);
        self.app.update(cx, |state, cx| self.model.mark_all_read(state, cx));
        cx.notify();
    }

    pub fn mark_seen(&mut self, cx: &mut Context<Self>) {
        self.model.mark_seen(self.app.read(cx));
        cx.notify();
    }

    fn on_app_event(&mut self, event: &AppEvent, cx: &mut Context<Self>) {
        match event {
            AppEvent::Activity => {
                self.app.update(cx, |state, cx| self.model.reload(state, cx));
                cx.notify();
            }
            AppEvent::Selection => {
                let Some(conversation_id) = self.selected_conversation(cx) else {
                    return;
                };
                agent_api::mark_conversation_proposals_seen(&conversation_id, cx);
                self.mark_conversation_read(&conversation_id, cx);
            }
            AppEvent::Messages(conversation_id) if self.in_foreground(conversation_id, cx) => {
                self.mark_conversation_read(conversation_id, cx);
            }
            _ => {}
        }
    }

    fn mark_conversation_read(&mut self, conversation_id: &str, cx: &mut Context<Self>) {
        self.app.update(cx, |state, cx| {
            self.model.mark_conversation_read(conversation_id, state, cx)
        });
        cx.notify();
    }

    fn on_proposals(&mut self, cx: &mut Context<Self>) {
        if let Some(conversation_id) = self.selected_conversation(cx)
            && self.in_foreground(&conversation_id, cx)
        {
            agent_api::mark_conversation_proposals_seen(&conversation_id, cx);
        }
        cx.notify();
    }

    fn selected_conversation(&self, cx: &App) -> Option<String> {
        self.app
            .read(cx)
            .selection
            .as_ref()
            .map(|selection| selection.conversation_id().to_owned())
    }

    fn in_foreground(&self, conversation_id: &str, cx: &mut Context<Self>) -> bool {
        self.selected_conversation(cx).as_deref() == Some(conversation_id)
            && self
                .main_window
                .update(cx, |_, window, _| window.is_window_active())
                .unwrap_or(false)
    }
}
