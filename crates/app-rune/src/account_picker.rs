//! The account picker: shown when the Teams session expired at startup, or from the status menu.
//! Choosing an account restarts Rune on its folder; the bridge does the work (`bridge.rs`).

use std::path::PathBuf;

use app_backend::schemes::active_palette;
use rune::*;

use crate::accounts::Account;
use crate::avatar::rgb;
use crate::icons::sym;
use crate::types::Actions;
use crate::ui::{UiState, text, ts};

#[derive(Debug, Clone, PartialEq)]
pub struct Picker {
    pub expired: bool,
    /// Set once the sign-in window of the current account was asked for.
    pub signing_in: bool,
    /// The registry and this run's folder, read once when the picker opens.
    pub accounts: Vec<Account>,
    pub current: Option<PathBuf>,
}

/// Shows the picker; the session banner goes with it.
pub(crate) fn open_picker<A: Actions>(s: &mut UiState<A>, expired: bool) {
    s.account_picker = Some(Picker {
        expired,
        signing_in: false,
        accounts: s.actions.accounts(),
        current: s.actions.current_folder(),
    });
    s.toast.session_expired = false;
}

pub fn view<A: Actions + 'static>(s: &UiState<A>) -> El {
    let Some(picker) = s.account_picker.as_ref() else {
        return col();
    };
    let palette = active_palette();
    let muted = rgb(palette.text_muted);
    let mut list = col().key("account-list").w_full().gap(4.0);
    for (index, account) in picker.accounts.iter().cloned().enumerate() {
        let is_current = picker.current.as_deref() == Some(account.folder.as_path());
        list = list.child(account_row::<A>(
            index,
            account,
            is_current,
            picker.expired && is_current,
        ));
    }
    let add = row()
        .grow(1.0)
        .gap(10.0)
        .center_y()
        .child(sym("person_add", 16.0).color(rgb(palette.foreground)))
        .child(
            text("Add an account")
                .font(ts(14.0, 400.0))
                .color(rgb(palette.foreground)),
        );
    let panel = col()
        .w(420.0)
        .gap(12.0)
        .child(
            text("Choose an account")
                .font(ts(20.0, 600.0))
                .color(rgb(palette.foreground)),
        )
        .children(picker.expired.then(|| {
            text("Your session expired. Choose an account.")
                .font(ts(14.0, 400.0))
                .color(muted)
                .wrap()
        }))
        .child(list)
        .child(choice(
            "account-add",
            add,
            |s: &mut UiState<A>, tx: &mut Tx| match s.actions.add_account() {
                Ok(()) => tx.quit(),
                Err(error) => s.toast.show(error),
            },
        ))
        .children(picker.signing_in.then(|| {
            text("Sign in in the window that opened")
                .font(ts(13.0, 400.0))
                .color(muted)
                .wrap()
        }))
        .children((!picker.expired).then(|| {
            crate::screens::button("Cancel", "account-cancel", |s: &mut UiState<A>, _| {
                s.account_picker = None
            })
        }));
    col()
        .key("account-picker-body")
        .grow(1.0)
        .w_full()
        .center()
        .child(panel)
}

/// The current account signs in again; any other account restarts Rune on its folder.
fn account_row<A: Actions + 'static>(
    index: usize,
    account: Account,
    is_current: bool,
    expired: bool,
) -> El {
    let palette = active_palette();
    let name = if account.name.is_empty() {
        account
            .folder
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
    } else {
        account.name.clone()
    };
    let details = col()
        .grow(1.0)
        .min_w(0.0)
        .child(
            text(name)
                .font(ts(14.0, 600.0))
                .color(rgb(palette.foreground))
                .truncate(),
        )
        .child(
            text(account.email.clone())
                .key(format!("account-email-{index}"))
                .font(ts(12.0, 400.0))
                .color(rgb(palette.text_muted))
                .truncate(),
        );
    let content = row()
        .grow(1.0)
        .min_w(0.0)
        .gap(10.0)
        .center_y()
        .child(details)
        .children(expired.then(|| {
            text("Expired")
                .key("account-badge")
                .font(ts(12.0, 600.0))
                .color(rgb(palette.amber))
        }));
    let folder = account.folder;
    let email = account.email;
    choice(
        &format!("account-{index}"),
        content,
        move |s: &mut UiState<A>, tx: &mut Tx| {
            if is_current {
                s.actions.sign_in(&email);
                if let Some(picker) = &mut s.account_picker {
                    picker.signing_in = true;
                }
            } else {
                match s.actions.switch_account(&folder) {
                    Ok(()) => tx.quit(),
                    Err(error) => s.toast.show(error),
                }
            }
        },
    )
}

/// A clickable row, found by its key.
fn choice<A: Actions + 'static>(
    key: &str,
    content: El,
    press: impl Fn(&mut UiState<A>, &mut Tx) + 'static,
) -> El {
    let palette = active_palette();
    row()
        .key(key.to_owned())
        .w_full()
        .min_h(48.0)
        .px(12.0)
        .gap(10.0)
        .center_y()
        .radius(8.0)
        .hover(rgb(palette.row_hover))
        .on_press_tx(press)
        .child(content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{SendTarget, Update};
    use crate::ui::view;
    use rune::{Env, Runtime};
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};

    #[derive(Default)]
    struct Fake {
        calls: RefCell<Vec<String>>,
        current: Option<PathBuf>,
        accounts: Vec<Account>,
    }

    impl Actions for Fake {
        fn open(&self, _: &str) {}
        fn send(&self, _: SendTarget, _: String, _: Vec<teams_core::FileReference>) {}
        fn accounts(&self) -> Vec<Account> {
            self.accounts.clone()
        }
        fn current_folder(&self) -> Option<PathBuf> {
            self.current.clone()
        }
        fn sign_in(&self, email: &str) {
            self.calls.borrow_mut().push(format!("sign_in {email}"));
        }
        fn switch_account(&self, folder: &Path) -> Result<(), String> {
            self.calls
                .borrow_mut()
                .push(format!("switch {}", folder.display()));
            Ok(())
        }
        fn add_account(&self) -> Result<(), String> {
            self.calls.borrow_mut().push("add".into());
            Ok(())
        }
    }

    fn account(folder: &str, email: &str) -> Account {
        Account {
            folder: PathBuf::from(folder),
            email: email.into(),
            name: "Ada".into(),
            last_used: 0,
        }
    }

    fn rt(accounts: Vec<Account>) -> Runtime<UiState<Fake>> {
        let fake = Fake {
            current: accounts.first().map(|account| account.folder.clone()),
            accounts,
            ..Fake::default()
        };
        let mut rt = Runtime::new(
            UiState::new(fake),
            view::<Fake>,
            (1000.0, 700.0),
            Env {
                system_dark: false,
                scale: 1.0,
            },
        );
        rt.frame(0.0);
        rt
    }

    fn click(rt: &mut Runtime<UiState<Fake>>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
    }

    fn apply(rt: &mut Runtime<UiState<Fake>>, update: Update) {
        rt.update(|s, _| s.apply(update));
        rt.frame(1.0 / 60.0);
    }

    fn open(rt: &mut Runtime<UiState<Fake>>, expired: bool) {
        rt.update(move |s, _| open_picker(s, expired));
        rt.frame(1.0 / 60.0);
    }

    fn calls(rt: &Runtime<UiState<Fake>>) -> Vec<String> {
        rt.state.actions.calls.borrow().clone()
    }

    /// `(expired, signing_in)` of the open picker, `None` when closed.
    fn flags(rt: &Runtime<UiState<Fake>>) -> Option<(bool, bool)> {
        rt.state
            .account_picker
            .as_ref()
            .map(|picker| (picker.expired, picker.signing_in))
    }

    #[test]
    fn expired_start_shows_picker() {
        let mut rt = rt(vec![account("C:/a", "ada@x.com")]);
        rt.update(|s, _| s.starting = true);
        apply(&mut rt, Update::SessionExpired);
        assert_eq!(flags(&rt), Some((true, false)));
        assert!(
            rt.find("account-badge").is_some(),
            "the current account is marked expired"
        );
    }

    #[test]
    fn rows_show_email() {
        let mut rt = rt(vec![
            account("C:/a", "ada@x.com"),
            account("C:/b", "ada@y.com"),
        ]);
        open(&mut rt, false);
        assert!(rt.find("account-email-0").is_some() && rt.find("account-email-1").is_some());
    }

    #[test]
    fn accounts_are_read_once_when_the_picker_opens() {
        let mut rt = rt(vec![account("C:/a", "ada@x.com")]);
        open(&mut rt, false);
        let picker = rt.state.account_picker.clone().expect("picker open");
        assert_eq!(picker.accounts, vec![account("C:/a", "ada@x.com")]);
        assert_eq!(picker.current, Some(PathBuf::from("C:/a")));
    }

    #[test]
    fn clicking_current_signs_in() {
        let mut rt = rt(vec![
            account("C:/a", "ada@x.com"),
            account("C:/b", "ada@y.com"),
        ]);
        open(&mut rt, true);
        click(&mut rt, "account-0");
        assert_eq!(calls(&rt), ["sign_in ada@x.com"]);
        assert_eq!(flags(&rt), Some((true, true)));
    }

    #[test]
    fn sign_in_failure_reopens_the_choice_with_the_reason() {
        let mut rt = rt(vec![account("C:/a", "ada@x.com")]);
        open(&mut rt, true);
        click(&mut rt, "account-0");
        apply(&mut rt, Update::SignInFailed("no window".into()));
        assert_eq!(flags(&rt), Some((true, false)));
        assert!(rt.find("toast").is_some(), "the reason is shown");
    }

    #[test]
    fn clicking_other_switches() {
        let mut rt = rt(vec![
            account("C:/a", "ada@x.com"),
            account("C:/b", "ada@y.com"),
        ]);
        open(&mut rt, true);
        click(&mut rt, "account-1");
        assert_eq!(calls(&rt), ["switch C:/b"]);
    }

    #[test]
    fn add_button_adds() {
        let mut rt = rt(vec![account("C:/a", "ada@x.com")]);
        open(&mut rt, true);
        click(&mut rt, "account-add");
        assert_eq!(calls(&rt), ["add"]);
    }

    #[test]
    fn status_menu_opens_picker() {
        let mut rt = rt(vec![account("C:/a", "ada@x.com")]);
        click(&mut rt, "title-account");
        click(&mut rt, "status-switch-account");
        assert_eq!(flags(&rt), Some((false, false)));
        assert!(!rt.state.own.menu);
        assert!(
            rt.find("account-cancel").is_some(),
            "a picker opened by choice can be cancelled"
        );
    }

    #[test]
    fn cancel_closes_a_chosen_picker() {
        let mut rt = rt(vec![account("C:/a", "ada@x.com")]);
        open(&mut rt, false);
        click(&mut rt, "account-cancel");
        assert_eq!(flags(&rt), None);
    }

    #[test]
    fn expired_after_cached_chats_opens_picker() {
        let mut rt = rt(vec![account("C:/a", "ada@x.com")]);
        apply(
            &mut rt,
            Update::Chats(vec![crate::types::ChatRow {
                id: "a".into(),
                title: "Ada".into(),
                preview: String::new(),
                unread: false,
                when: String::new(),
            }]),
        );
        apply(&mut rt, Update::SessionExpired);
        assert_eq!(flags(&rt), Some((true, false)));
    }

    #[test]
    fn expired_after_online_shows_banner() {
        let mut rt = rt(vec![account("C:/a", "ada@x.com")]);
        apply(&mut rt, Update::Status("Online".into()));
        apply(&mut rt, Update::SessionExpired);
        assert_eq!(flags(&rt), None, "the banner waits for a click");
        assert!(rt.state.toast.session_expired);
    }

    #[test]
    fn session_banner_opens_the_expired_picker() {
        let mut rt = rt(vec![account("C:/a", "ada@x.com")]);
        apply(&mut rt, Update::Status("Online".into()));
        apply(&mut rt, Update::SessionExpired);
        assert_eq!(flags(&rt), None, "the banner waits for a click");
        click(&mut rt, "session-sign-in");
        assert_eq!(flags(&rt), Some((true, false)));
        assert!(
            !rt.state.toast.session_expired,
            "the banner clears when the picker opens"
        );
    }

    #[test]
    fn online_clears_the_session_banner() {
        let mut rt = rt(vec![account("C:/a", "ada@x.com")]);
        apply(&mut rt, Update::Status("Online".into()));
        apply(&mut rt, Update::SessionExpired);
        assert!(rt.find("session-expired-banner").is_some());
        apply(&mut rt, Update::Status("Connecting…".into()));
        apply(&mut rt, Update::Status("Online".into()));
        assert!(rt.find("session-expired-banner").is_none());
    }

    #[test]
    fn chats_after_signing_in_close_the_picker() {
        let mut rt = rt(vec![account("C:/a", "ada@x.com")]);
        open(&mut rt, true);
        click(&mut rt, "account-0");
        apply(&mut rt, Update::Chats(Vec::new()));
        assert_eq!(flags(&rt), None);
    }
}
