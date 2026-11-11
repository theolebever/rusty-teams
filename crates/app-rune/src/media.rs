//! Message images and file cards, using the core's authenticated image cache.

use std::collections::{HashMap, HashSet};
use std::cell::RefCell;
use std::time::{Duration, Instant};
use std::path::PathBuf;
use std::sync::Arc;

use rune::*;
use crate::ui::text;
use teams_core::{FileCard, FileReference, ImageRef, UploadedFile};

use crate::types::{Actions, SendTarget};
use crate::ui::UiState;
use crate::bridge::Shared;
use crate::types::Update;

const IMAGE_BUDGET: usize = 48 * 1024 * 1024;
const KEEP_RECENT: Duration = Duration::from_secs(10);
const PREVIEW_EDGE: u32 = 720;
pub type Preview = (u32, u32, Vec<u8>);

fn preview(bytes: &[u8]) -> Option<Preview> {
    let format = ::image::guess_format(bytes).ok()?;
    if !matches!(format, ::image::ImageFormat::Jpeg | ::image::ImageFormat::Png) { return None; }
    if format == ::image::ImageFormat::Png
        && ::image::codecs::png::PngDecoder::new(std::io::Cursor::new(bytes)).ok()?.is_apng().ok()?
    { return None; }
    let reader = || ::image::ImageReader::with_format(std::io::Cursor::new(bytes), format);
    let (width, height) = reader().into_dimensions().ok()?;
    if width.max(height) <= PREVIEW_EDGE { return None; }
    let image = reader().decode().ok()?.thumbnail(PREVIEW_EDGE, PREVIEW_EDGE).into_rgba8();
    Some((image.width(), image.height(), image.into_raw()))
}

pub fn load_image(shared: Arc<Shared>, image: ImageRef) {
    let Some(engine) = shared.engine() else { return };
    app_backend::runtime::handle().spawn(async move {
        let key = image.key().to_owned();
        match engine.fetch_image(&image).await {
            Ok(_) => {
                if let Some(stored) = engine.image(&key) {
                    let bytes = stored.bytes;
                    let prepared = tokio::task::spawn_blocking(move || {
                        let preview = preview(&bytes);
                        (bytes, preview)
                    }).await;
                    match prepared {
                        Ok((bytes, preview)) => shared.publish(Update::ImagePreview { key, bytes, preview }),
                        Err(error) => shared.publish(Update::ImageFailed { key, error: error.to_string() }),
                    }
                }
            }
            Err(error) => shared.publish(Update::ImageFailed { key, error: error.to_string() }),
        }
    });
}

pub fn download_file(shared: Arc<Shared>, url: String, destination: PathBuf) {
    if !is_web_url(&url) {
        shared.publish(Update::Status("Invalid attachment URL".into()));
        return;
    }
    let Some(engine) = shared.engine() else {
        shared.publish(Update::Status("Not connected yet".into()));
        return;
    };
    app_backend::runtime::handle().spawn(async move {
        let result: Result<(), String> = async {
            use std::io::Write;
            let directory = destination.parent().filter(|path| !path.as_os_str().is_empty())
                .unwrap_or_else(|| std::path::Path::new("."));
            let mut temporary = tempfile::NamedTempFile::new_in(directory).map_err(|error| error.to_string())?;
            engine.download_file(&url, |bytes| temporary.write_all(bytes), |_| {}).await
                .map_err(|error| error.to_string())?;
            temporary.as_file().sync_all().map_err(|error| error.to_string())?;
            temporary.persist(&destination).map_err(|error| error.to_string())?;
            Ok(())
        }.await;
        shared.publish(Update::Status(match result {
            Ok(()) => format!("Saved {}", destination.display()),
            Err(error) => format!("Could not save attachment: {error}"),
        }));
    });
}

pub fn upload_file(shared: Arc<Shared>, target: SendTarget, path: PathBuf) {
    if shared.read_only { return; }
    let Some(engine) = shared.engine() else {
        shared.publish(Update::Attachment { target, path, result: Err("Not connected yet".into()) });
        return;
    };
    app_backend::runtime::handle().spawn(async move {
        let result: Result<UploadedFile, String> = async {
            let file_name = path.file_name().and_then(|value| value.to_str())
                .ok_or_else(|| "Invalid file name".to_owned())?.to_owned();
            let read_path = path.clone();
            let bytes = tokio::task::spawn_blocking(move || std::fs::read(read_path)).await
                .map_err(|error| error.to_string())?.map_err(|error| error.to_string())?;
            let uploaded = engine.upload_attachment_file(&target.conversation_id, &file_name, &bytes, |_| {}).await
                .map_err(|error| error.to_string())?;
            if let Err(error) = engine.share_attachment(&target.conversation_id, &uploaded).await {
                if let Err(cleanup) = engine.discard_attachment(&uploaded).await {
                    return Err(format!("{error}; uploaded file could not be removed: {cleanup}"));
                }
                return Err(error.to_string());
            }
            Ok(uploaded)
        }.await;
        shared.publish(Update::Attachment { target, path, result });
    });
}

pub fn discard_attachment(shared: Arc<Shared>, uploaded: UploadedFile) {
    if shared.read_only { return; }
    let Some(engine) = shared.engine() else { return };
    app_backend::runtime::handle().spawn(async move {
        if let Err(error) = engine.discard_attachment(&uploaded).await {
            shared.publish(Update::Status(format!("Could not remove uploaded attachment: {error}")));
        }
    });
}

struct Attachment {
    path: Option<PathBuf>,
    name: String,
    reference: Option<FileReference>,
    uploaded: Option<UploadedFile>,
    error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Content {
    pub rich: crate::rich_text::Rich,
    pub audio: Vec<crate::voice::Clip>,
    pub images: Vec<ImageRef>,
    pub files: Vec<FileCard>,
}

impl Content {
    pub fn from_record(record: &store::MessageRecord) -> Self {
        if record.deleted {
            return Self::default();
        }
        Self { rich: crate::rich_text::Rich::from_record(record), audio: crate::voice::clips(record), images: teams_core::images(record), files: teams_core::files(record) }
    }
}

pub struct MediaState {
    pub voice: crate::voice::State,
    cache: ImageCache,
    /// The app icon the title bar draws.
    logo: Image,
    pub images: HashMap<String, Image>,
    originals: HashMap<String, Vec<u8>>,
    sizes: HashMap<String, usize>,
    shown: RefCell<HashMap<String, Instant>>,
    viewer_image: Option<Image>,
    visible: RefCell<HashSet<String>>,
    evicted: HashSet<String>,
    pub errors: HashMap<String, String>,
    /// Cache key and alt text of the image open in the full-window viewer.
    pub viewer: Option<(String, String)>,
    attachments: HashMap<SendTarget, Vec<Attachment>>,
}

impl Default for MediaState {
    fn default() -> Self {
        let cache = ImageCache::new(Arc::new(|| {
            if let Some(wake) = crate::WAKER.get() { wake(); }
        }));
        Self {
            voice: Default::default(),
            logo: cache.load(app_backend::appearance::AppIcon::MicrosoftTeams.png()),
            cache,
            images: HashMap::new(),
            originals: HashMap::new(),
            sizes: HashMap::new(),
            shown: RefCell::new(HashMap::new()),
            viewer_image: None,
            visible: RefCell::new(HashSet::new()),
            evicted: HashSet::new(),
            errors: HashMap::new(),
            viewer: None,
            attachments: HashMap::new(),
        }
    }
}

impl MediaState {
    pub fn logo(&self) -> &Image {
        &self.logo
    }


    pub fn loaded(&mut self, key: String, bytes: Vec<u8>) {
        self.loaded_preview(key, bytes, None);
    }

    pub fn loaded_preview(&mut self, key: String, bytes: Vec<u8>, preview: Option<Preview>) {
        self.evicted.remove(&key);
        self.errors.remove(&key);
        let (image, size) = if let Some((width, height, rgba)) = preview {
            let size = rgba.len().saturating_mul(4) / 3 + bytes.len();
            self.originals.insert(key.clone(), bytes);
            (self.cache.rgba(width, height, rgba), size)
        } else {
            let dimensions = ::image::ImageReader::new(std::io::Cursor::new(&bytes))
                .with_guessed_format().ok().and_then(|reader| reader.into_dimensions().ok());
            let size = dimensions.map_or(bytes.len(), |(w, h)| (w as usize).saturating_mul(h as usize).saturating_mul(16) / 3);
            self.originals.remove(&key);
            (self.cache.load(bytes), size)
        };
        self.shown.get_mut().insert(key.clone(), Instant::now());
        // Avatar publication is deduplicated by the bridge, so retain these until it supports reload.
        if !key.starts_with("avatar:") { self.sizes.insert(key.clone(), size); }
        self.images.insert(key, image);
        self.trim(Instant::now());
    }

    pub fn image(&self, key: &str) -> Option<&Image> {
        let image = self.images.get(key)?;
        if let Some(shown) = self.shown.borrow_mut().get_mut(key) { *shown = Instant::now(); }
        let mut visible = self.visible.borrow_mut();
        if !visible.contains(key) { visible.insert(key.to_owned()); }
        Some(image)
    }

    pub fn clear_visible(&mut self) { self.visible.get_mut().clear(); }

    pub fn open_viewer(&mut self, key: String, alt: String) {
        self.viewer_image = self.originals.get(&key).map(|bytes| self.cache.load(bytes.clone()));
        self.viewer = Some((key, alt));
    }

    pub fn viewed_image(&self, key: &str) -> Option<&Image> {
        self.viewer_image.as_ref().or_else(|| self.image(key))
    }

    pub fn trim(&mut self, now: Instant) {
        if self.viewer.is_none() { self.viewer_image = None; }
        let mut total = self.sizes.values().fold(0usize, |total, size| total.saturating_add(*size));
        if total <= IMAGE_BUDGET { return; }
        let visible = self.visible.get_mut();
        let mut candidates: Vec<_> = self.shown.get_mut().iter()
            .filter(|(key, shown)| self.sizes.contains_key(*key) && !visible.contains(*key) && self.viewer.as_ref().is_none_or(|(viewed, _)| viewed != *key)
                && now.saturating_duration_since(**shown) >= KEEP_RECENT)
            .map(|(key, shown)| (key.clone(), *shown)).collect();
        candidates.sort_by_key(|(_, shown)| *shown);
        for (key, _) in candidates {
            if total <= IMAGE_BUDGET { break; }
            total = total.saturating_sub(self.sizes.remove(&key).unwrap_or(0));
            self.images.remove(&key);
            self.originals.remove(&key);
            self.shown.get_mut().remove(&key);
            self.evicted.insert(key);
        }
    }

    pub fn files(&self, target: &SendTarget) -> Vec<FileReference> {
        self.attachments.get(target).into_iter().flatten().filter_map(|file| file.reference.clone()).collect()
    }

    pub fn is_uploading(&self, target: &SendTarget) -> bool {
        self.attachments.get(target).into_iter().flatten().any(|file| file.reference.is_none())
    }

    pub fn clear(&mut self, target: &SendTarget) {
        self.attachments.remove(target);
    }

    pub fn restore_files(&mut self, target: SendTarget, files: Vec<FileReference>) {
        let pending = self.attachments.entry(target).or_default();
        for reference in files {
            if pending.iter().any(|file| file.reference.as_ref().is_some_and(|file| file.attachment_id == reference.attachment_id)) { continue; }
            // Delivery is uncertain after a failed send, so these files must never be deleted automatically.
            pending.push(Attachment { name: reference.name.clone(), reference: Some(reference), path: None, uploaded: None, error: None });
        }
    }

    /// Returns an upload to discard when its draft was removed while uploading.
    pub fn uploaded(&mut self, target: SendTarget, path: PathBuf, result: Result<UploadedFile, String>) -> Option<UploadedFile> {
        let pending = self.attachments.get_mut(&target).and_then(|files| files.iter_mut().find(|file| file.path.as_ref() == Some(&path)));
        match (pending, result) {
            (Some(file), Ok(uploaded)) => {
                file.reference = uploaded.reference();
                if file.reference.is_none() { file.error = Some("Uploaded file has no attachment ID".into()); }
                file.uploaded = Some(uploaded);
                None
            }
            (Some(file), Err(error)) => { file.error = Some(error); None }
            (None, uploaded) => uploaded.ok(),
        }
    }
}

pub fn rows_visible<A: Actions>(s: &mut UiState<A>, rows: std::ops::Range<usize>) -> bool {
    let displayed = s.search_results.as_ref().unwrap_or(&s.messages);
    let mut visible = HashSet::new();
    for row in displayed.iter().skip(rows.start).take(rows.len()) {
        for image in row.media.images.iter().cloned().chain(row.media.rich.images()) {
            visible.insert(image.key().to_owned());
            if s.media.evicted.remove(image.key()) && !s.media.errors.contains_key(image.key()) {
                s.actions.load_image(image);
            }
        }
    }
    *s.media.visible.get_mut() = visible;
    crate::find::rows_visible(s, rows)
}

/// Opens the file picker for `target`; the chosen file joins that draft's attachments.
pub fn open_picker<A: Actions + 'static>(target: &SendTarget) -> impl Fn(&mut UiState<A>, &mut Tx) + 'static {
    let select_target = target.clone();
    move |_: &mut UiState<A>, tx: &mut Tx| {
        let target = select_target.clone();
        tx.open_file(FileDialogOptions::default(), move |s: &mut UiState<A>, result, _| {
            if s.read_only { return; }
            match result {
                Ok(Some(path)) => {
                    let pending = s.media.attachments.entry(target.clone()).or_default();
                    if pending.iter().any(|file| file.path.as_ref() == Some(&path)) { return; }
                    let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                    pending.push(Attachment { path: Some(path.clone()), name, reference: None, uploaded: None, error: None });
                    s.actions.upload_file(target, path);
                }
                Ok(None) => {},
                Err(error) => s.status = error.to_string(),
            }
        });
    }
}

pub fn attachments<A: Actions + 'static>(s: &UiState<A>, target: &SendTarget) -> El {
    let mut body = col().key("attachments").w_full().gap(4.0);
    for (index, file) in s.media.attachments.get(target).into_iter().flatten().enumerate() {
        let target = target.clone();
        let label = match (&file.reference, &file.error) {
            (_, Some(error)) => format!("{}: {error}", file.name),
            (None, None) => format!("{}: Uploading…", file.name),
            (Some(_), None) => file.name.clone(),
        };
        body = body.child(row().key(format!("attachment-{index}")).w_full().gap(8.0)
            .child(text(label).wrap().grow(1.0).min_w(0.0))
            .child(row().key(format!("attachment-remove-{index}")).padding(6.0).on_press(move |s: &mut UiState<A>| {
                if let Some(files) = s.media.attachments.get_mut(&target) && index < files.len()
                    && let Some(uploaded) = files.remove(index).uploaded {
                    s.actions.discard_attachment(uploaded);
                }
            }).child(text("Remove"))));
    }
    body
}

pub fn view<A: Actions + 'static>(s: &UiState<A>, content: &Content, message_id: &str) -> El {
    let mut body = col().key(format!("media-{message_id}")).w_full().gap(6.0);
    for (index, clip) in content.audio.iter().enumerate() {
        body = body.child(crate::voice::view(s, clip, &format!("audio-{message_id}-{index}")));
    }
    for (index, image) in content.images.iter().enumerate() {
        let key = image.key().to_owned();
        let width = image.width.filter(|&width| width > 0).unwrap_or(320).min(400) as f32;
        let height = match (image.width, image.height) {
            (Some(w), Some(h)) if w > 0 && h > 0 => (width * h as f32 / w as f32).min(250.0),
            _ => 220.0,
        };
        let mut picture = col().key(format!("image-{message_id}-{index}")).w(width).max_w(400.0).h(height)
            .label(image.alt.clone().unwrap_or_else(|| "Message image".into()));
        let alt = image.alt.clone().unwrap_or_else(|| "Message image".into());
        match s.media.image(&key) {
            Some(image) if image.error().is_none() => {
                let viewed = (key.clone(), alt.clone());
                picture = picture.pixels_fit(image.pixels(), SurfaceFit::Fit)
                    .on_press(move |s: &mut UiState<A>| s.media.open_viewer(viewed.0.clone(), viewed.1.clone()));
            }
            _ => {
                let label = s.media.errors.get(&key).cloned().unwrap_or_else(|| "Load image".into());
                let image = image.clone();
                picture = picture.child(text(label).wrap()).on_press(move |s: &mut UiState<A>| {
                    s.actions.load_image(image.clone());
                });
            }
        }
        body = body.child(picture);
    }
    for (index, file) in content.files.iter().enumerate() {
        let url = file.open_url.clone();
        let name = file.name.clone();
        let download_url = url.clone();
        let can_open = is_web_url(&url);
        let mut card = row().key(format!("file-{message_id}-{index}")).w_full().gap(8.0).center_y()
            .child(text(name.clone()).wrap().grow(1.0).min_w(0.0));
        if can_open {
            card = card.child(row().key(format!("file-open-{message_id}-{index}")).padding(6.0)
                .on_press_tx(move |s: &mut UiState<A>, tx| {
                    if let Err(error) = tx.open_url(&url) { s.status = error.to_string(); }
                }).child(text("Open")));
            // Loop components open in their web app and are not downloadable files.
            if file.kind != teams_core::FileKind::Loop {
                card = card.child(row().key(format!("file-save-{message_id}-{index}")).padding(6.0)
                    .on_press_tx(move |_: &mut UiState<A>, tx| {
                        let url = download_url.clone();
                        let file_name = PathBuf::from(&name).file_name().map(|value| value.to_string_lossy().into_owned());
                        tx.save_file(FileDialogOptions { file_name, ..Default::default() }, move |s: &mut UiState<A>, result, _| {
                            match result {
                                Ok(Some(path)) => s.actions.download_file(url, path),
                                Ok(None) => {},
                                Err(error) => s.status = error.to_string(),
                            }
                        });
                    }).child(text("Save as")));
            }
        }
        body = body.child(card);
    }
    let rich = &content.rich;
    if let Some(banner) = &rich.banner {
        body = body.child(crate::cards::banner(s, banner, &format!("banner-{message_id}")));
    }
    for (index, card) in rich.cards.iter().enumerate() {
        body = body.child(crate::cards::view(s, card, &format!("card-{message_id}-{index}")));
    }
    if let Some(preview) = &rich.preview {
        body = body.child(crate::link_preview::view(s, preview, message_id));
    }
    body
}

fn is_web_url(value: &str) -> bool {
    value.starts_with("https://") || value.starts_with("http://")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = std::io::Cursor::new(Vec::new());
        ::image::RgbaImage::new(width, height).write_to(&mut bytes, ::image::ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    #[test]
    fn previews_limit_stills_and_viewer_keeps_the_original_resolution() {
        let bytes = png(1440, 960);
        let thumb = preview(&bytes).expect("large PNG has a thumbnail");
        assert_eq!((thumb.0, thumb.1), (720, 480));
        let mut state = MediaState::default();
        state.loaded_preview("large".into(), bytes, Some(thumb));
        assert_eq!(state.images["large"].wait().unwrap(), [720, 480]);
        state.open_viewer("large".into(), "Original".into());
        assert_eq!(state.viewed_image("large").unwrap().wait().unwrap(), [1440, 960]);
        assert!(preview(&png(32, 32)).is_none());
        let mut gif = std::io::Cursor::new(Vec::new());
        ::image::RgbaImage::new(800, 1).write_to(&mut gif, ::image::ImageFormat::Gif).unwrap();
        assert!(preview(&gif.into_inner()).is_none(), "animations must use Rune's original decoder");
    }

    #[test]
    fn eviction_keeps_visible_recent_viewer_and_avatar_images() {
        let mut state = MediaState::default();
        for key in ["old", "visible", "recent", "viewer", "avatar:user"] {
            state.loaded(key.into(), png(1, 1));
        }
        let now = Instant::now();
        for key in ["old", "visible", "viewer", "avatar:user"] {
            state.shown.get_mut().insert(key.into(), now - KEEP_RECENT);
        }
        state.sizes.insert("old".into(), IMAGE_BUDGET);
        state.visible.get_mut().insert("visible".into());
        state.viewer = Some(("viewer".into(), String::new()));
        state.trim(now);
        assert!(!state.images.contains_key("old"));
        assert!(state.evicted.contains("old"));
        for key in ["visible", "recent", "viewer", "avatar:user"] {
            assert!(state.images.contains_key(key), "{key} must stay loaded");
        }
    }

    #[test]
    fn scrolling_an_evicted_message_into_view_reloads_it_once() {
        let image = ImageRef { id: "i".into(), url: "https://example.com/a.png".into(), width: None, height: None, alt: None };
        let mut state = UiState::new(Recorder::default());
        state.messages.push(message("m", Content { images: vec![image.clone()], ..Default::default() }));
        state.media.loaded(image.key().into(), png(1, 1));
        state.media.shown.get_mut().insert(image.key().into(), Instant::now() - KEEP_RECENT);
        state.media.sizes.insert(image.key().into(), IMAGE_BUDGET + 1);
        state.media.trim(Instant::now());
        rows_visible(&mut state, 0..1);
        rows_visible(&mut state, 0..1);
        assert_eq!(state.actions.calls.borrow().as_slice(), ["load https://example.com/a.png"]);
    }

    #[test]
    fn image_only_messages_reserve_a_visible_image_box() {
        let mut rt = rt();
        let image = ImageRef { id: "gif".into(), url: "https://media0.giphy.com/media/test/giphy.gif".into(), width: Some(320), height: Some(200), alt: Some("Animated GIF".into()) };
        let mut row = message("gif", Content { images: vec![image], files: vec![], ..Default::default() });
        row.text.clear();
        show(&mut rt, row);
        let picture = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("image-gif-0")).unwrap();
        assert!(picture.rect[2] >= 300.0, "image-only bubbles must not collapse: {:?}", picture.rect);
        assert_eq!(picture.rect[3], 200.0);
    }

    #[test]
    fn deleted_messages_do_not_keep_media_and_files_cannot_launch_local_programs() {
        let mut record = store::MessageRecord { body_html: "<img src=\"https://example.com/image.png\">".into(), ..Default::default() };
        assert_eq!(Content::from_record(&record).images.len(), 1);
        record.deleted = true;
        assert_eq!(Content::from_record(&record), Content::default());
        assert!(!is_web_url("file:///C:/Windows/notepad.exe"));
        assert!(!is_web_url("javascript:alert(1)"));
        assert!(is_web_url("https://example.com/file"));
    }

    #[test]
    fn restored_attachments_stay_with_their_thread_and_are_not_deleted_on_remove() {
        let root = SendTarget { conversation_id: "channel".into(), thread_root_id: None, channel: true };
        let reply = SendTarget { thread_root_id: Some("post".into()), ..root.clone() };
        let reference = FileReference { attachment_id: "file".into(), content_url: "https://example.com/file".into(), name: "report.txt".into() };
        let mut state = MediaState::default();
        state.restore_files(root.clone(), vec![reference.clone()]);
        state.restore_files(root.clone(), vec![reference.clone()]);
        assert_eq!(state.files(&root), vec![reference.clone()]);
        assert!(state.files(&reply).is_empty());
        state.restore_files(reply.clone(), vec![reference.clone()]);
        state.clear(&root);
        assert_eq!(state.files(&reply), vec![reference]);
        assert!(!state.is_uploading(&reply));
        assert!(state.attachments[&reply][0].uploaded.is_none(), "uncertain delivery must keep the shared file intact");
    }

    #[derive(Default)]
    struct Recorder {
        calls: std::cell::RefCell<Vec<String>>,
    }

    impl Actions for Recorder {
        fn open(&self, _: &str) {}
        fn send(&self, _: SendTarget, _: String, _: Vec<teams_core::FileReference>) {}
        fn load_image(&self, image: ImageRef) {
            self.calls.borrow_mut().push(format!("load {}", image.key()));
        }
    }

    fn rt() -> rune::Runtime<UiState<Recorder>> {
        let env = rune::Env { system_dark: false, scale: 1.0 };
        let mut rt = rune::Runtime::new(UiState::new(Recorder::default()), crate::ui::view::<Recorder>, (1000.0, 700.0), env);
        rt.frame(0.0);
        rt
    }

    fn click(rt: &mut rune::Runtime<UiState<Recorder>>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
    }

    fn message(id: &str, media: Content) -> crate::types::MessageRow {
        crate::types::MessageRow {
            sender_id: None,
            system: false,
            id: id.into(),
            reply_to_id: None,
            deleted: false,
            can_delete: false,
            reactions: vec![],
            sender: "Ada".into(),
            when: "14:06".into(),
            text: "see attached".into(),
            mine: false,
            media,
        }
    }

    fn show(rt: &mut rune::Runtime<UiState<Recorder>>, row: crate::types::MessageRow) {
        rt.update(|s, _| {
            s.selected = Some("chat".into());
            s.apply(Update::Messages { conversation_id: "chat".into(), rows: vec![row] });
        });
        rt.frame(1.0 / 60.0);
    }

    #[test]
    fn images_load_when_their_chat_opens_and_a_failed_one_retries_on_click() {
        let mut rt = rt();
        let image = ImageRef { id: "i".into(), url: "https://example.com/a.png".into(), width: None, height: None, alt: None };
        show(&mut rt, message("m", Content { images: vec![image.clone()], files: vec![], ..Default::default() }));
        assert_eq!(rt.state.actions.calls.borrow().as_slice(), ["load https://example.com/a.png"]);
        rt.update(|s, _| s.apply(Update::ImageFailed { key: image.key().into(), error: "offline".into() }));
        rt.frame(1.0 / 60.0);
        assert_eq!(rt.state.actions.calls.borrow().len(), 1, "a failed image is not re-requested on every update");
        click(&mut rt, "image-m-0");
        assert_eq!(rt.state.actions.calls.borrow().len(), 2);
    }

    #[test]
    fn file_chips_open_web_links_and_save_only_downloadable_files() {
        let mut rt = rt();
        let card = |name: &str, kind, open_url: &str| teams_core::FileCard {
            name: name.into(),
            kind,
            content_type: None,
            size: None,
            open_url: open_url.into(),
        };
        let files = vec![
            card("plan.docx", teams_core::FileKind::Word, "https://contoso.sharepoint.com/plan.docx"),
            card("Loop page", teams_core::FileKind::Loop, "https://loop.microsoft.com/page"),
            card("tool.exe", teams_core::FileKind::Other, "file:///C:/Windows/notepad.exe"),
        ];
        show(&mut rt, message("m", Content { images: vec![], files, ..Default::default() }));
        assert!(rt.find("file-open-m-0").is_some() && rt.find("file-save-m-0").is_some());
        assert!(rt.find("file-open-m-1").is_some(), "loop components open in the web app");
        assert!(rt.find("file-save-m-1").is_none(), "loop components are not downloadable");
        assert!(rt.find("file-open-m-2").is_none(), "local paths never get an open action");
        assert!(rt.find("file-save-m-2").is_none());
    }

    #[test]
    fn clicking_a_loaded_image_opens_the_viewer_and_escape_closes_it() {
        let mut png = std::io::Cursor::new(Vec::new());
        ::image::RgbaImage::new(1, 1).write_to(&mut png, ::image::ImageFormat::Png).expect("encode test image");
        let mut rt = rt();
        let image = ImageRef { id: "i".into(), url: "https://example.com/a.png".into(), width: Some(100), height: Some(100), alt: None };
        show(&mut rt, message("m", Content { images: vec![image.clone()], ..Default::default() }));
        rt.update(|s, _| s.apply(Update::Image { key: image.key().into(), bytes: png.into_inner() }));
        rt.frame(1.0 / 60.0);
        click(&mut rt, "image-m-0");
        assert!(rt.state.media.viewer.is_some() && rt.find("image-viewer").is_some());
        rt.key(rune::KeyEvent { key: rune::Key::Escape, mods: Default::default() });
        rt.frame(1.0 / 60.0);
        assert!(rt.state.media.viewer.is_none() && rt.find("image-viewer").is_none());
    }

    #[test]
    fn link_previews_and_banners_render_under_the_message() {
        let mut rt = rt();
        let preview = teams_core::LinkPreview { url: "https://example.com/post".into(), title: Some("A post".into()), description: None, image_url: None, image_width: None, image_height: None };
        let banner = teams_core::AnnouncementBanner { title: "Big news".into(), background: teams_core::BannerBackground::Color("rose".into()) };
        let rich = crate::rich_text::Rich { preview: Some(preview), banner: Some(banner), ..Default::default() };
        show(&mut rt, message("m", Content { rich, ..Default::default() }));
        assert!(rt.find("link-preview-m").is_some() && rt.find("banner-m").is_some());
    }
}
