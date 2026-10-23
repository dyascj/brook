//! Keeping Brook current: a daily check against GitHub releases and a
//! one-click install. The mechanics live in `brook_core::update`.

use std::time::Duration;

use brook_core::update::{self, Release};
use gpui::{App, AppContext, Context, Entity, EventEmitter, SharedString, Task};

use crate::library::{Library, Notice};

/// How often the loop wakes to see whether a check is due.
const WAKE: Duration = Duration::from_secs(60 * 60);

/// Checks are due a day after the last successful one.
const CHECK_EVERY_SECS: i64 = 24 * 60 * 60;

const LAST_CHECKED: &str = "updates.last_checked_at";

/// Where updating stands.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Status {
    /// Nothing to do.
    #[default]
    Idle,
    /// Asking GitHub.
    Checking,
    /// A newer release is ready to install.
    Available(Release),
    /// Downloading and swapping it in.
    Installing(Release),
    /// Installing failed.
    Failed(Release, SharedString),
}

/// The update state and the loop that checks.
pub struct Updater {
    library: Entity<Library>,
    /// Where updating stands.
    pub status: Status,
    _loop: Option<Task<()>>,
}

impl EventEmitter<Notice> for Updater {}

impl Updater {
    /// Starts checking in the background, unless updates are off.
    pub fn new(library: Entity<Library>, cx: &mut Context<Self>) -> Self {
        let looping = update::disabled_reason().is_none().then(|| {
            cx.spawn(async move |this, cx| {
                loop {
                    let alive = this.update(cx, |this, cx| {
                        if this.is_due(cx) {
                            this.check(false, cx);
                        }
                    });
                    if alive.is_err() {
                        break;
                    }
                    cx.background_executor().timer(WAKE).await;
                }
            })
        });
        Self { library, status: Status::Idle, _loop: looping }
    }

    fn is_due(&self, cx: &App) -> bool {
        let library = self.library.read(cx);
        if !library.settings.check_for_updates {
            return false;
        }
        let last = library.state(LAST_CHECKED).and_then(|value| value.parse::<i64>().ok());
        last.is_none_or(|last| chrono::Utc::now().timestamp() - last >= CHECK_EVERY_SECS)
    }

    /// Asks GitHub for a newer release. `manual` reports the outcome even
    /// when there is nothing new.
    pub fn check(&mut self, manual: bool, cx: &mut Context<Self>) {
        if let Some(reason) = update::disabled_reason() {
            if manual {
                cx.emit(Notice::Info(
                    format!("Updates are off: {reason}. Opening the releases page.").into(),
                ));
                cx.open_url(update::RELEASES_URL);
            }
            return;
        }
        if matches!(self.status, Status::Checking | Status::Installing(_)) {
            return;
        }
        let previous = std::mem::replace(&mut self.status, Status::Checking);
        cx.notify();
        let client = self.library.read(cx).client().clone();
        let lookup = cx.background_spawn(async move { update::check(&client) });
        cx.spawn(async move |this, cx| {
            let result = lookup.await;
            this.update(cx, |this, cx| {
                this.status = match result {
                    Ok(Some(release)) => Status::Available(release),
                    Ok(None) => {
                        if manual {
                            cx.emit(Notice::Info("You're on the latest version.".into()));
                        }
                        Status::Idle
                    }
                    Err(error) => {
                        log::warn!("update check failed: {error}");
                        if manual {
                            cx.emit(Notice::Error("Couldn't check for updates. Try again later.".into()));
                        }
                        match previous {
                            Status::Checking => Status::Idle,
                            other => other,
                        }
                    }
                };
                if !matches!(this.status, Status::Checking) {
                    let now = chrono::Utc::now().timestamp().to_string();
                    this.library.read(cx).set_state(LAST_CHECKED, &now);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Installs the available release and relaunches into it.
    pub fn install(&mut self, cx: &mut Context<Self>) {
        let release = match &self.status {
            Status::Available(release) | Status::Failed(release, _) => release.clone(),
            _ => return,
        };
        self.status = Status::Installing(release.clone());
        cx.notify();
        let client = self.library.read(cx).client().clone();
        let work = {
            let release = release.clone();
            cx.background_spawn(async move { update::install(&client, &release) })
        };
        cx.spawn(async move |this, cx| {
            let result = work.await;
            this.update(cx, |this, cx| match result.and_then(|()| update::relaunch()) {
                Ok(()) => cx.quit(),
                Err(error) => {
                    this.status = Status::Failed(release, error.to_string().into());
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }
}
