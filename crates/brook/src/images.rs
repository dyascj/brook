//! Article images and site icons, loaded on first sight.
//!
//! Views ask for an image while rendering. The first ask starts a download
//! into the disk cache and returns `None`; when the file lands, observers
//! re-render and the next ask returns its path.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use brook_core::cache::DiskCache;
use brook_core::http::Client;
use brook_core::model::Feed;
use gpui::{AppContext, Context};

#[derive(Clone, Debug)]
enum State {
    Loading,
    Ready(Arc<Path>),
    Missing,
}

/// Downloaded images, by address.
pub struct Images {
    cache: DiskCache,
    client: Client,
    states: HashMap<String, State>,
}

impl Images {
    /// Images stored in `cache`, fetched with `client`.
    pub fn new(cache: DiskCache, client: Client) -> Self {
        Self { cache, client, states: HashMap::new() }
    }

    /// The image at `url`, once downloaded.
    pub fn image(&mut self, url: &str, cx: &mut Context<Self>) -> Option<Arc<Path>> {
        if let Some(state) = self.states.get(url) {
            return ready(state);
        }
        if let Some(path) = self.cache.cached_image(url) {
            let path: Arc<Path> = path.into();
            self.states.insert(url.to_owned(), State::Ready(path.clone()));
            return Some(path);
        }
        let (cache, client, key) = (self.cache.clone(), self.client.clone(), url.to_owned());
        self.load(url.to_owned(), cx, move || cache.fetch_image(&client, &key).ok());
        None
    }

    /// Whether the image at `url` failed to download.
    pub fn is_missing(&self, url: &str) -> bool {
        matches!(self.states.get(url), Some(State::Missing))
    }

    /// The icon of the site behind `feed`, once found.
    pub fn feed_icon(&mut self, feed: &Feed, cx: &mut Context<Self>) -> Option<Arc<Path>> {
        self.icon(feed.site_url.as_deref().unwrap_or(&feed.url), cx)
    }

    /// The icon of the website at `site`, once found.
    pub fn icon(&mut self, site: &str, cx: &mut Context<Self>) -> Option<Arc<Path>> {
        // The disk cache files icons by exact host; match it.
        let host = url::Url::parse(site).ok()?.host_str()?.to_owned();
        let key = format!("icon:{host}");
        if let Some(state) = self.states.get(&key) {
            return ready(state);
        }
        if let Some(path) = self.cache.cached_icon(&host) {
            let path: Arc<Path> = path.into();
            self.states.insert(key, State::Ready(path.clone()));
            return Some(path);
        }
        let site = site.to_owned();
        let (cache, client) = (self.cache.clone(), self.client.clone());
        self.load(key, cx, move || cache.fetch_icon(&client, &site).ok());
        None
    }

    fn load(
        &mut self,
        key: String,
        cx: &mut Context<Self>,
        fetch: impl FnOnce() -> Option<std::path::PathBuf> + Send + 'static,
    ) {
        self.states.insert(key.clone(), State::Loading);
        let task = cx.background_spawn(async move { fetch() });
        cx.spawn(async move |this, cx| {
            let path = task.await;
            this.update(cx, |this, cx| {
                let state = path.map_or(State::Missing, |path| State::Ready(path.into()));
                this.states.insert(key, state);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

fn ready(state: &State) -> Option<Arc<Path>> {
    match state {
        State::Ready(path) => Some(path.clone()),
        State::Loading | State::Missing => None,
    }
}
