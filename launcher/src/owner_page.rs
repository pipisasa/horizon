//! The owner gate in the window (`horizon_launcher::account::launch`): the screen that asks for a
//! Microsoft account that owns Minecraft: Java Edition. It covers the launcher once for players
//! without one (those who played offline before the gate), and again whenever Play is refused.
//! The sign-in runs on it, code and all; GitHub and Google open it for offline play.
use super::*;

impl Launcher {
    /// Whether the screen covers the window: no owner, and either Play was refused or the player
    /// hasn't read it yet. The data question comes first.
    pub(crate) fn asking_owner(&self) -> bool {
        self.owner_shown() == Some(false)
            && (self.owner_screen_open || !self.settings.owner_notice_seen)
            && !self.asking_consent()
            && self.opening.is_none()
    }

    /// `owner` as the window shows it: the developer option that lets offline names play without
    /// an owner (`dev_options`) hides the gate, as if the keychain hadn't answered yet.
    pub(crate) fn owner_shown(&self) -> Option<bool> {
        match self.owner {
            Some(false) if self.settings.dev_offline => None,
            owner => owner,
        }
    }

    /// Opens the screen and starts the sign-in on it.
    pub(crate) fn sign_in_to_play(&mut self) {
        self.owner_screen_open = true;
        self.start_sign_in();
    }

    /// Closes it; it comes back with the next refused Play.
    fn close_owner_screen(&mut self) {
        if let Some(s) = self.sign_in.take() {
            s.cancel();
        }
        self.sign_in_state = None;
        self.owner_screen_open = false;
        if !self.settings.owner_notice_seen {
            self.settings.owner_notice_seen = true;
            self.settings.save();
        }
    }

    pub(crate) fn owner_screen(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let t = self.theme;
        let configured = account::client_id().is_some();
        let external = Provider::ALL.into_iter().any(Provider::configured);
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .p(px(28.))
            .child(caps(tr("Microsoft account")).text_color(t.muted))
            .child(
                text(theme::HEADING)
                    .text_color(t.ink)
                    .child(format!("{}.", tr(account::NEEDS_OWNER))),
            )
            .child(text(theme::UI).text_color(t.muted).child(tr(
                "Horizon needs a Microsoft account that owns Minecraft: Java Edition, like other launchers.",
            )))
            .when(external, |d| {
                d.child(text(theme::UI).text_color(t.muted).child(tr(
                    "Or sign in with GitHub or Google to play offline: LAN and offline-mode servers, under a name you choose.",
                )))
            })
            .child(text(theme::UI).text_color(t.muted).child(tr(
                "Your worlds, offline names and settings stay where they are.",
            )));
        let status = match self.sign_in_status(cx) {
            _ if !configured && !external => Some(div().child(tr(account::NOT_CONFIGURED))),
            Some(_) if matches!(self.sign_in_state, Some(SignInState::Done(Ok(_)))) => None,
            status => status,
        };
        body = body.children(status.map(|s| {
            plate(&t)
                .p(px(16.))
                .text_size(px(theme::UI.px))
                .text_color(t.ink_soft)
                .child(s)
        }));
        let mut answers = div()
            .flex()
            .justify_end()
            .gap(px(12.))
            .px(px(28.))
            .pb(px(28.))
            .child(
                button("owner-later", "Not now", Kind::Secondary, &t).on_click(cx.listener(
                    |this, _: &ClickEvent, _, cx| {
                        this.close_owner_screen();
                        cx.notify();
                    },
                )),
            );
        answers = match &self.sign_in_state {
            Some(SignInState::Code(code)) => {
                let (uri, user_code) = (code.verification_uri.clone(), code.user_code.clone());
                answers
                    .child(
                        button("owner-copy", "Copy code", Kind::Secondary, &t).on_click(
                            move |_, _, cx| {
                                copy_to_clipboard(
                                    cx,
                                    gpui::ClipboardItem::new_string(user_code.clone()),
                                )
                            },
                        ),
                    )
                    .child(
                        button("owner-open", "Open link", Kind::Primary, &t)
                            .on_click(move |_, _, cx| cx.open_url(&uri)),
                    )
            }
            Some(SignInState::Starting | SignInState::ChooseName(_)) => answers,
            _ if !configured && !external => answers,
            _ => answers
                .child(self.external_sign_in_buttons("owner-sign-in", cx))
                .when(configured, |d| {
                    d.child(
                        button("owner-sign-in", "Sign in with Microsoft", Kind::Primary, &t)
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.sign_in_to_play();
                                cx.notify();
                            })),
                    )
                }),
        };
        let card = plate(&t)
            .flex()
            .flex_col()
            // A set width: text measured at it wraps where it is drawn.
            .w(px(560.))
            .max_w_full()
            .bg(t.page)
            .border_1()
            .border_color(t.rule)
            .child(body)
            .child(answers);
        div()
            .id("owner-gate")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .px(px(32.))
            .py(px(24.))
            .bg(t.page.opacity(0.92))
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .occlude()
            .child(card)
    }
}
