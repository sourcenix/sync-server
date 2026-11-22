use async_trait::async_trait;
use innertube::Innertube;
use itertools::Itertools;
use ytrss::YouTubeError;

use crate::{
    dto::{CreateVideo, ExtendedPlaylist, ExtendedPublicPlaylist},
    models::Channel,
    validation::{ValidationError, ValidationResult, Validator},
};

pub struct RssValidator {
    innertube: Option<&'static Innertube>,
}

impl RssValidator {
    pub fn new(innertube: Option<&'static Innertube>) -> Self {
        Self { innertube }
    }
}

fn alphanumeric_words(s: &str) -> Vec<String> {
    s.trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .split(" ")
        .map(|w| w.trim().to_string())
        .collect_vec()
}

/// Checks whether the texts look somewhat similar.
/// Retrieves `None` if approximation is not possible. (if a or b is empty or contains non-alphanumeric symbols)
pub fn approx_cmp_title(a: &str, b: &str) -> Option<bool> {
    let a_words = alphanumeric_words(a);
    let b_words = alphanumeric_words(b);

    if a_words.contains(&String::new()) || b_words.contains(&String::new()) {
        return None;
    }

    let max_word_count = std::cmp::max(a_words.len(), b_words.len());
    let mut matching_words = 0;
    for a_word in &a_words {
        for b_word in &b_words {
            if a_word.contains(b_word) || b_word.contains(a_word) {
                matching_words += 1;
                break;
            }
        }
    }

    Some(matching_words >= 1 && matching_words > max_word_count / 3)
}

macro_rules! unwrap {
    ($fn:expr) => {
        match $fn {
            Ok(resp) => resp,
            Err(err) => match err {
                YouTubeError::ConnectionError => return ValidationResult::Skip,
                _ => return ValidationError::Invalid.into(),
            },
        }
    };
}

#[async_trait]
impl Validator for RssValidator {
    async fn is_channel_information_valid(&self, channel: &Channel) -> ValidationResult<Channel> {
        let rss_channel = unwrap!(ytrss::RssChannel::fetch_from_channel_id(&channel.id).await);

        let is_eq_or_similar =
            if let Some(cmp) = approx_cmp_title(&channel.name, rss_channel.name()) {
                cmp
            } else if let Some(it) = self.innertube {
                it.channel(&channel.id)
                    .title()
                    .await
                    .is_ok_and(|title| title.as_ref() == Some(&channel.name))
            } else {
                return ValidationResult::Skip;
            };

        if !is_eq_or_similar {
            return ValidationError::InvalidWithContext(
                "invalid channel name provided".to_string(),
            )
            .into();
        }

        ValidationResult::ValidAfterChanges(Channel {
            id: channel.id.clone(),
            name: rss_channel.name().to_string(),
            avatar: channel.avatar.clone(),
            verified: channel.verified,
        })
    }

    async fn is_video_information_valid(
        &self,
        video_data: &CreateVideo,
    ) -> ValidationResult<CreateVideo> {
        let rss_channel =
            unwrap!(ytrss::RssChannel::fetch_from_channel_id(&video_data.uploader.id).await);

        // Video is older than the videos in the feed
        if rss_channel
            .oldest_video_date()
            .is_some_and(|oldest| oldest.timestamp_millis() > video_data.upload_date)
        {
            return ValidationResult::Skip;
        }

        let Some(rss_video) = rss_channel.find_video(&video_data.id) else {
            return ValidationError::Invalid.into();
        };

        let is_eq_or_similar =
            if let Some(cmp) = approx_cmp_title(&video_data.title, rss_video.title()) {
                cmp
            } else if let Some(it) = self.innertube {
                it.video(&video_data.id)
                    .title()
                    .await
                    .is_ok_and(|title| title.as_ref() == Some(&video_data.title))
            } else {
                return ValidationResult::Skip;
            };

        if !is_eq_or_similar {
            return ValidationError::InvalidWithContext("invalid video title provided".to_string())
                .into();
        }

        ValidationResult::ValidAfterChanges(CreateVideo {
            id: video_data.id.clone(),
            title: rss_video.title().to_string(),
            upload_date: rss_video.date().timestamp_millis(),
            uploader: Channel {
                id: video_data.uploader.id.clone(),
                name: rss_channel.name().to_string(),
                avatar: video_data.uploader.avatar.clone(),
                verified: video_data.uploader.verified,
            },
            thumbnail_url: rss_video.thumbnail_url().to_string(),
            duration: video_data.duration,
        })
    }

    async fn is_playlist_information_valid(
        &self,
        playlist: &ExtendedPublicPlaylist,
    ) -> ValidationResult<ExtendedPublicPlaylist> {
        let rss_playlist =
            unwrap!(ytrss::RssPlaylist::fetch_from_playlist_id(&playlist.playlist.id).await);

        let is_eq_or_similar =
            if let Some(cmp) = approx_cmp_title(&playlist.playlist.title, rss_playlist.title()) {
                cmp
            } else if let Some(it) = self.innertube {
                it.playlist(&playlist.playlist.id)
                    .title()
                    .await
                    .is_ok_and(|title| title.as_ref() == Some(&playlist.playlist.title))
            } else {
                return ValidationResult::Skip;
            };

        if !is_eq_or_similar {
            return ValidationError::InvalidWithContext(
                "invalid playlist title provided".to_string(),
            )
            .into();
        }

        let mut uploader = playlist.uploader.clone();

        if playlist
            .playlist
            .video_count
            .is_some_and(|video_count| video_count < rss_playlist.video_count() as u64)
        {
            return ValidationError::InvalidWithContext(
                "video count is less than actual amount of videos".to_string(),
            )
            .into();
        };

        match self
            .is_channel_information_valid(&playlist.uploader)
            .await
            .assign_changes(&mut uploader)
        {
            ValidationResult::Valid => (),
            ValidationResult::ValidAfterChanges(changes) => uploader = changes,
            ValidationResult::Skip => return ValidationResult::Skip,
            ValidationResult::Invalid(err) => return ValidationResult::Invalid(err),
        };

        ValidationResult::ValidAfterChanges(ExtendedPublicPlaylist {
            playlist: ExtendedPlaylist {
                id: playlist.playlist.id.clone(),
                title: rss_playlist.title().to_string(),
                description: playlist.playlist.description.clone(),
                thumbnail_url: playlist.playlist.thumbnail_url.clone(),
                video_count: playlist.playlist.video_count,
            },
            uploader,
        })
    }
}
