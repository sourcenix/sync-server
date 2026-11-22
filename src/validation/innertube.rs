use crate::{
    dto::{CreateVideo, ExtendedPlaylist, ExtendedPublicPlaylist},
    models::Channel,
    validation::{ValidationError, ValidationResult, Validator},
};
use async_trait::async_trait;
use innertube::{Innertube, InnertubeError};

pub struct InnertubeValidator {
    innertube: &'static Innertube,
}

impl InnertubeValidator {
    pub fn new(innertube: &'static Innertube) -> Self {
        Self { innertube }
    }
}

macro_rules! unwrap {
    ($fn:expr) => {
        match $fn {
            Ok(resp) => resp,
            Err(err) => {
                return if matches!(err, InnertubeError::NotFoundInJSON(_)) {
                    ValidationError::Invalid.into()
                } else {
                    ValidationResult::Skip
                };
            }
        }
    };
}

#[async_trait]
impl Validator for InnertubeValidator {
    async fn is_channel_information_valid(&self, channel: &Channel) -> ValidationResult<Channel> {
        let it_channel = self.innertube.channel(&channel.id);
        let it_metadata = unwrap!(it_channel.metadata().await);

        if channel.name != it_metadata.title {
            return ValidationError::InvalidWithContext(
                "invalid channel name provided".to_string(),
            )
            .into();
        }

        ValidationResult::ValidAfterChanges(Channel {
            id: it_channel.id().to_string(),
            name: it_metadata.title.clone(),
            avatar: Some(it_metadata.avatar.max_resolution().url.clone()),
            verified: channel.verified,
        })
    }

    async fn is_video_information_valid(
        &self,
        video_data: &CreateVideo,
    ) -> ValidationResult<CreateVideo> {
        let mut video_data_uploader = video_data.uploader.clone();
        let it_video = self.innertube.video(&video_data.id);
        let it_metadata = unwrap!(it_video.metadata().await);

        if video_data.title != it_metadata.title {
            return ValidationError::InvalidWithContext("invalid video title provided".to_string())
                .into();
        }

        if video_data.uploader.id == it_metadata.channel_id
            && self
                .is_channel_information_valid(&video_data.uploader)
                .await
                .assign_changes(&mut video_data_uploader)
                .is_valid()
        {
            ValidationResult::ValidAfterChanges(CreateVideo {
                id: it_video.id().to_string(),
                title: it_metadata.title.clone(),
                uploader: video_data_uploader,
                thumbnail_url: it_metadata.thumbnail.max_resolution().url,
                duration: it_metadata.length_seconds as i32,
                upload_date: video_data.upload_date,
            })
        } else {
            ValidationError::Invalid.into()
        }
    }

    async fn is_playlist_information_valid(
        &self,
        playlist: &ExtendedPublicPlaylist,
    ) -> ValidationResult<ExtendedPublicPlaylist> {
        let it_playlist = self.innertube.playlist(&playlist.playlist.id);
        let it_metadata = unwrap!(it_playlist.metadata().await);

        if playlist.playlist.title != it_metadata.title {
            return ValidationError::InvalidWithContext(
                "invalid playlist title provided".to_string(),
            )
            .into();
        }

        let mut uploader = playlist.uploader.clone();

        if playlist
            .playlist
            .video_count
            .is_some_and(|video_count| video_count != it_metadata.video_count as u64)
        {
            return ValidationError::InvalidWithContext(
                "video count is not equal to the actual amount of videos".to_string(),
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
                id: it_playlist.id().to_string(),
                title: it_metadata.title.clone(),
                description: it_metadata.description.clone().unwrap_or_default(),
                thumbnail_url: Some(it_metadata.thumbnail.max_resolution().url),
                video_count: Some(it_metadata.video_count as u64),
            },
            uploader,
        })
    }
}
