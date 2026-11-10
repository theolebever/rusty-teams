//! The composer under the open conversation, drawn as the GPUI app draws it
//! (`crates/app/src/views/composer.rs`): one rounded field with the format, emoji, attach, AI and
//! add icons and the send icon inline on its right. Attachments, a thread banner and send errors
//! stack above it.

use app_backend::schemes::active_palette;
use rune::*;

use crate::avatar::rgb;
use crate::icons::sym;
use crate::media;
use crate::types::Actions;
use crate::ui::{INK, LINE, MUTED, UiState, active_target, c, select_thread, send, ts};

const ICON: f32 = 20.0;
const BUTTON: f32 = 32.0;
const INSET: f32 = 6.0;
const ACCENT: &str = "#5B5FC7";

pub(crate) fn composer<A: Actions + 'static>(s: &UiState<A>) -> El {
    let palette = active_palette();
    let target = active_target(s);
    let has_text = !s.composer.text.trim().is_empty();
    let hover = rgb(palette.popup_hover);

    let field = text_input(&s.composer, ts(14.0, 400.0))
        .key("composer")
        .label("Message")
        .placeholder("Type a message", c(MUTED))
        .color(c(INK))
        .w_full()
        .on_edit(|s: &mut UiState<A>| &mut s.composer)
        .on_submit(|s: &mut UiState<A>, _tx: &mut Tx| send(s));
    let field = row()
        .key("composer-field")
        .grow(1.0)
        .min_w(0.0)
        .min_h(BUTTON)
        .center_y()
        .child(auto_grow(field, 1, 8).grow(1.0).min_w(0.0));

    let tool = |name: &'static str, key: &'static str| {
        row()
            .key(key)
            .size(BUTTON, BUTTON)
            .radius(8.0)
            .center()
            .hover(hover)
            .child(sym(name, ICON).color(c(MUTED)))
    };
    let attach = target.as_ref().map(|target| tool("attach_file", "attach-file").on_press_tx(media::open_picker::<A>(target)));
    let toolbar = row()
        .key("composer-tools")
        .gap(INSET)
        .center_y()
        .child(tool("text_format", "composer-format"))
        .child(tool("mood", "composer-emoji"))
        .children(attach)
        .child(tool("auto_fix_high", "composer-ai"))
        .child(tool("add", "composer-add"));

    let send_button = row()
        .key("send")
        .size(BUTTON, BUTTON)
        .radius(8.0)
        .center()
        .hover(hover)
        .on_press_tx(|s: &mut UiState<A>, tx: &mut Tx| {
            send(s);
            tx.focus("composer", false);
        })
        .child(sym("send", ICON).color(if has_text { c(ACCENT) } else { c(MUTED) }));

    let bar = row()
        .key("composer-bar")
        .w_full()
        .px(INSET)
        .py(INSET)
        .gap(INSET)
        .center_y()
        .radius(12.0)
        .fill(rgb(palette.surface_raised))
        .child(field)
        .child(toolbar)
        .child(el().w(1.0).h(20.0).fill(c(LINE)))
        .child(send_button);

    col()
        .w_full()
        .px(20.0)
        .pt(8.0)
        .pb(16.0)
        .gap(6.0)
        .children(target.clone().map(|target| media::attachments(s, &target)))
        .children(s.thread_root_id.as_ref().map(|_| row().key("close-reply").h(28.0).px(8.0).hover(c(crate::ui::HOVER))
            .on_press_tx(|s: &mut UiState<A>, _| select_thread(s, None))
            .child(text("Replying in thread · Close").font(ts(12.0, 600.0)).color(c(INK)))))
        .children(s.send_error.as_ref().map(|e| {
            text(e.clone())
                .key("send-error")
                .font(ts(12.0, 400.0))
                .color(c("#C4314B"))
                .wrap()
        }))
        .child(bar)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChatRow, SendTarget, Update};
    use rune::{Env, Runtime};

    struct Quiet;

    impl Actions for Quiet {
        fn open(&self, _conversation_id: &str) {}
        fn send(&self, _target: SendTarget, _text: String, _files: Vec<teams_core::FileReference>) {}
    }

    fn rt() -> Runtime<UiState<Quiet>> {
        let env = Env { system_dark: false, scale: 1.0 };
        let mut rt = Runtime::new(UiState::new(Quiet), crate::ui::view::<Quiet>, (1000.0, 700.0), env);
        rt.frame(0.0);
        rt.update(|s, _| {
            s.apply(Update::Chats(vec![ChatRow {
                id: "b".into(),
                title: "Bo".into(),
                preview: "hi".into(),
                unread: false,
                when: "14:05".into(),
            }]))
        });
        rt.frame(1.0 / 60.0);
        let (x, y) = rt.find("chat-b").expect("chat row");
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
        rt
    }

    /// Laid-out rect `[x, y, w, h]` of the built element at `key`.
    fn rect(rt: &Runtime<UiState<Quiet>>, key: &str) -> [f32; 4] {
        let suffix = format!("/{key}");
        rt.elements()
            .into_iter()
            .find(|e| e.path.ends_with(&suffix))
            .unwrap_or_else(|| panic!("no built element {key}"))
            .rect
    }

    #[test]
    fn attach_icon_and_send_sit_inline_right_of_the_field() {
        let rt = rt();
        let [field_x, _, field_w, _] = rect(&rt, "composer");
        let [attach_x, attach_y, attach_w, attach_h] = rect(&rt, "attach-file");
        let [send_x, send_y, send_w, send_h] = rect(&rt, "send");
        assert!(attach_x >= field_x + field_w, "attach icon at {attach_x} sits inside the field");
        assert!(send_x > attach_x, "send icon should follow the attach icon");
        assert_eq!([attach_w, attach_h], [BUTTON, BUTTON]);
        assert_eq!([send_w, send_h], [BUTTON, BUTTON]);
        assert!((attach_y - send_y).abs() < 0.5, "toolbar icons share one row");
    }
}
