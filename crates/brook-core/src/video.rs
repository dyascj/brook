//! Recognizing YouTube and Vimeo videos by their addresses.

use url::Url;

/// Where a video is hosted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Provider {
    /// youtube.com and youtu.be.
    YouTube,
    /// vimeo.com.
    Vimeo,
}

impl Provider {
    /// The provider's display name.
    pub fn name(self) -> &'static str {
        match self {
            Provider::YouTube => "YouTube",
            Provider::Vimeo => "Vimeo",
        }
    }
}

/// A video Brook can show as a card that opens in the browser.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Video {
    /// The host.
    pub provider: Provider,
    /// The host's id for the video.
    pub id: String,
}

impl Video {
    /// Recognizes a watch page, short link, or embed address.
    pub fn from_url(address: &str) -> Option<Self> {
        let url = Url::parse(address.trim()).ok()?;
        let host = url.host_str()?.trim_start_matches("www.").trim_start_matches("m.");
        let mut segments = url.path_segments()?.filter(|s| !s.is_empty());
        let (provider, id) = match host {
            "youtube.com" | "youtube-nocookie.com" | "music.youtube.com" => {
                let first = segments.next();
                let id = match first {
                    Some("watch") => {
                        url.query_pairs().find(|(key, _)| key == "v").map(|(_, value)| value.into_owned())
                    }
                    Some("embed" | "v" | "shorts" | "live") => segments.next().map(str::to_owned),
                    _ => None,
                }?;
                (Provider::YouTube, id)
            }
            "youtu.be" => (Provider::YouTube, segments.next()?.to_owned()),
            "vimeo.com" => {
                (Provider::Vimeo, segments.find(|s| s.chars().all(|c| c.is_ascii_digit()))?.to_owned())
            }
            "player.vimeo.com" => match (segments.next(), segments.next()) {
                (Some("video"), Some(id)) => (Provider::Vimeo, id.to_owned()),
                _ => return None,
            },
            _ => return None,
        };
        let valid = match provider {
            Provider::YouTube => {
                id.len() == 11 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            }
            Provider::Vimeo => !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()),
        };
        valid.then_some(Video { provider, id })
    }

    /// The page that plays the video.
    pub fn watch_url(&self) -> String {
        match self.provider {
            Provider::YouTube => format!("https://www.youtube.com/watch?v={}", self.id),
            Provider::Vimeo => format!("https://vimeo.com/{}", self.id),
        }
    }

    /// A still frame, where the host offers one without an API call.
    pub fn thumbnail_url(&self) -> Option<String> {
        match self.provider {
            Provider::YouTube => Some(format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", self.id)),
            Provider::Vimeo => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(url: &str) -> Option<(Provider, String)> {
        Video::from_url(url).map(|video| (video.provider, video.id))
    }

    #[test]
    fn recognizes_youtube_addresses() {
        let expected = Some((Provider::YouTube, "dQw4w9WgXcQ".to_owned()));
        for url in [
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://youtube.com/watch?feature=share&v=dQw4w9WgXcQ",
            "https://m.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://youtu.be/dQw4w9WgXcQ?t=42",
            "https://www.youtube.com/embed/dQw4w9WgXcQ?rel=0",
            "https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ",
            "https://www.youtube.com/shorts/dQw4w9WgXcQ",
        ] {
            assert_eq!(id(url), expected, "{url}");
        }
    }

    #[test]
    fn recognizes_vimeo_addresses() {
        assert_eq!(id("https://vimeo.com/76979871"), Some((Provider::Vimeo, "76979871".into())));
        assert_eq!(
            id("https://vimeo.com/channels/staffpicks/76979871"),
            Some((Provider::Vimeo, "76979871".into()))
        );
        assert_eq!(
            id("https://player.vimeo.com/video/76979871?h=x"),
            Some((Provider::Vimeo, "76979871".into()))
        );
    }

    #[test]
    fn ignores_everything_else() {
        for url in [
            "https://www.youtube.com/@channel",
            "https://www.youtube.com/watch?v=short",
            "https://example.com/watch?v=dQw4w9WgXcQ",
            "https://vimeo.com/about",
            "not a url",
        ] {
            assert_eq!(id(url), None, "{url}");
        }
    }

    #[test]
    fn builds_watch_and_thumbnail_urls() {
        let video = Video::from_url("https://youtu.be/dQw4w9WgXcQ").unwrap();
        assert_eq!(video.watch_url(), "https://www.youtube.com/watch?v=dQw4w9WgXcQ");
        assert_eq!(video.thumbnail_url().unwrap(), "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg");
        let video = Video::from_url("https://vimeo.com/1").unwrap();
        assert_eq!((video.watch_url().as_str(), video.thumbnail_url()), ("https://vimeo.com/1", None));
    }
}
