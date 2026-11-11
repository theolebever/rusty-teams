//! Voice cards use the authenticated hosted-content transport and the existing Rodio audio stack.
use std::collections::{HashMap, HashSet};
use std::io::Cursor;

use crate::avatar::rgb;
use crate::types::Actions;
use crate::ui::{UiState, ts};
use rune::*;
use crate::ui::text;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clip {
    pub url: String,
    pub seconds: u64,
}

pub fn clips(record: &store::MessageRecord) -> Vec<Clip> {
    if record.deleted {
        return vec![];
    }
    teams_core::attachments(record)
        .into_iter()
        .filter(|a| a.content_type.as_deref() == Some("application/vnd.microsoft.card.audio"))
        .filter_map(|a| serde_json::from_str::<serde_json::Value>(a.content.as_deref()?).ok())
        .flat_map(|card| {
            let seconds = card
                .get("duration")
                .and_then(|v| v.as_str())
                .and_then(teams_core::parse_duration_seconds)
                .unwrap_or_default();
            card.get("media")
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
                .filter_map(|media| media.get("url").and_then(|v| v.as_str()))
                .map(|url| Clip {
                    url: url.to_owned(),
                    seconds,
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

struct Playback {
    url: String,
    player: rodio::Player,
    _output: rodio::MixerDeviceSink,
    speed: f32,
}

#[derive(Default)]
pub struct State {
    bytes: HashMap<String, Vec<u8>>,
    loading: HashSet<String>,
    errors: HashMap<String, String>,
    pending: Option<String>,
    playback: Option<Playback>,
}

impl State {
    pub fn is_playing(&self) -> bool {
        self.playback
            .as_ref()
            .is_some_and(|p| !p.player.is_paused() && !p.player.empty())
    }

    pub fn pause(&mut self) {
        self.pending = None;
        if let Some(p) = &self.playback {
            p.player.pause();
        }
    }

    pub fn loaded(&mut self, url: String, result: Result<Vec<u8>, String>) {
        self.loading.remove(&url);
        match result {
            Ok(bytes) => {
                self.bytes.insert(url.clone(), bytes);
                if self.pending.as_deref() == Some(&url) {
                    self.pending = None;
                    self.toggle(&url);
                }
            }
            Err(error) => {
                if self.pending.as_deref() == Some(&url) {
                    self.pending = None;
                }
                self.errors.insert(url, error);
            }
        }
    }

    fn toggle(&mut self, url: &str) {
        self.errors.remove(url);
        if let Some(p) = &self.playback
            && p.url == url
            && !p.player.empty()
        {
            if p.player.is_paused() {
                p.player.play();
            } else {
                p.player.pause();
            }
            return;
        }
        self.pause();
        self.playback = None;
        let Some(bytes) = self.bytes.get(url) else {
            return;
        };
        let result = (|| {
            let source = decode(bytes.clone()).map_err(|e| e.to_string())?;
            let mut output =
                rodio::DeviceSinkBuilder::open_default_sink().map_err(|e| e.to_string())?;
            output.log_on_drop(false);
            let player = rodio::Player::connect_new(output.mixer());
            player.append(source);
            Ok::<_, String>(Playback {
                url: url.to_owned(),
                player,
                _output: output,
                speed: 1.0,
            })
        })();
        match result {
            Ok(playback) => self.playback = Some(playback),
            Err(error) => {
                self.errors.insert(url.to_owned(), error);
            }
        }
    }
}

/// Teams voice messages are AAC in MP4; the demuxer needs a known length to seek to `moov`.
fn decode(bytes: Vec<u8>) -> Result<rodio::Decoder<Cursor<Vec<u8>>>, rodio::decoder::DecoderError> {
    let len = bytes.len() as u64;
    rodio::Decoder::builder().with_data(Cursor::new(bytes)).with_byte_len(len).build()
}

fn clock(seconds: u64) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

pub fn view<A: Actions + 'static>(s: &UiState<A>, clip: &Clip, key: &str) -> El {
    let palette = app_backend::schemes::active_palette();
    let state = &s.media.voice;
    let playback = state.playback.as_ref().filter(|p| p.url == clip.url);
    let playing = playback.is_some_and(|p| !p.player.is_paused() && !p.player.empty());
    let loading = state.loading.contains(&clip.url);
    let position = playback.map_or(0, |p| p.player.get_pos().as_secs());
    let progress = if clip.seconds == 0 {
        0.0
    } else {
        (position as f32 / clip.seconds as f32).clamp(0.0, 1.0)
    };
    let url = clip.url.clone();
    let control = row()
        .key(format!("{key}-play"))
        .size(36.0, 36.0)
        .radius(18.0)
        .center()
        .fill(rgb(palette.surface_raised))
        .hover(rgb(palette.row_hover))
        .label(if loading {
            "Loading audio"
        } else if playing {
            "Pause voice message"
        } else {
            "Play voice message"
        })
        .on_press(move |s: &mut UiState<A>| {
            if s.media.voice.loading.contains(&url) {
                return;
            }
            if s.media.voice.bytes.contains_key(&url) {
                s.media.voice.toggle(&url);
            } else {
                s.media.voice.pause();
                s.media.voice.pending = Some(url.clone());
                s.media.voice.loading.insert(url.clone());
                s.media.voice.errors.remove(&url);
                s.actions.load_audio(&url);
            }
        })
        .child(
            text(if loading {
                "…"
            } else if playing {
                "Ⅱ"
            } else {
                "▶"
            })
            .font(ts(16.0, 600.0))
            .color(rgb(palette.accent)),
        );
    let mut card = col()
        .key(key.to_owned())
        .w(280.0)
        .max_w(280.0)
        .gap(6.0)
        .padding(8.0)
        .child(
            row().gap(10.0).center_y().child(control).child(
                col()
                    .grow(1.0)
                    .gap(6.0)
                    .child(
                        text("Voice message")
                            .font(ts(12.0, 600.0))
                            .color(rgb(palette.foreground)),
                    )
                    .child(
                        el().w_full()
                            .h(3.0)
                            .radius(2.0)
                            .fill(rgb(palette.border_strong))
                            .child(el().w(150.0 * progress).h(3.0).fill(rgb(palette.accent))),
                    )
                    .child(
                        text(format!("{} / {}", clock(position), clock(clip.seconds)))
                            .font(ts(11.0, 400.0))
                            .color(rgb(palette.text_muted)),
                    ),
            ),
        );
    if let Some(p) = playback {
        let url = clip.url.clone();
        card = card.child(
            row()
                .gap(8.0)
                .child(crate::screens::button(
                    &format!("{}×", p.speed),
                    &format!("{key}-speed"),
                    move |s: &mut UiState<A>, _| {
                        if let Some(p) = &mut s.media.voice.playback
                            && p.url == url
                        {
                            p.speed = if p.speed == 1.0 {
                                1.5
                            } else if p.speed == 1.5 {
                                2.0
                            } else {
                                1.0
                            };
                            p.player.set_speed(p.speed);
                        }
                    },
                ))
                .child(crate::screens::button(
                    "Replay",
                    &format!("{key}-replay"),
                    |s: &mut UiState<A>, _| {
                        if let Some(p) = s.media.voice.playback.take() {
                            let url = p.url;
                            drop(p.player);
                            drop(p._output);
                            s.media.voice.toggle(&url);
                        }
                    },
                )),
        );
    }
    if let Some(error) = state.errors.get(&clip.url) {
        card = card.child(
            text(format!(
                "Could not play audio: {error}. Select Play to retry."
            ))
            .font(ts(12.0, 400.0))
            .color(rgb(palette.text_muted))
            .wrap(),
        );
    }
    card
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn audio_cards_preserve_the_authenticated_url_and_duration() {
        let mut record = store::MessageRecord {
            attachments_json: serde_json::json!([{
                "content_type": "application/vnd.microsoft.card.audio",
                "content": r#"{"duration":"PT46S","media":[{"url":"https://graph.microsoft.com/v1.0/chats/chat/messages/m/hostedContents/audio/$value"}]}"#
            }]).to_string(), ..Default::default()
        };
        let audio = clips(&record);
        assert_eq!(audio.len(), 1);
        assert_eq!(audio[0].seconds, 46);
        assert!(audio[0].url.ends_with("/$value"));
        record.deleted = true;
        assert!(clips(&record).is_empty());
    }
    #[test]
    fn late_audio_download_never_autoplays_after_leaving_a_chat() {
        let mut state = State {
            pending: Some("url".into()),
            ..Default::default()
        };
        state.pause();
        state.loaded("url".into(), Ok(vec![1, 2, 3]));
        assert!(!state.is_playing());
        assert!(state.errors.is_empty());
        assert_eq!(clock(76), "1:16");
    }
    #[test]
    fn teams_voice_format_aac_in_mp4_decodes() {
        let bytes = include_bytes!("../tests/data/voice.m4a").to_vec();
        let source = decode(bytes).expect("AAC/MP4 decoder");
        assert!(source.count() > 0);
    }
}
