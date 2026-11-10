//! Message images and file cards, using the core's authenticated image cache.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use rune::*;
use teams_core::{FileCard, FileReference, ImageRef, UploadedFile};

use crate::types::{Actions, SendTarget};
use crate::ui::UiState;
use crate::bridge::Shared;
use crate::types::Update;

pub fn load_image(shared: Arc<Shared>, image: ImageRef) {
    let Some(engine) = shared.engine() else { return };
    app_backend::runtime::handle().spawn(async move {
        let key = image.key().to_owned();
        match engine.fetch_image(&image).await {
            Ok(_) => {
                if let Some(stored) = engine.image(&key) {
                    shared.publish(Update::Image { key, bytes: stored.bytes });
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Content {
    pub images: Vec<ImageRef>,
    pub files: Vec<FileCard>,
}

impl Content {
    pub fn from_record(record: &store::MessageRecord) -> Self {
        if record.deleted {
            return Self::default();
        }
        Self { images: teams_core::images(record), files: teams_core::files(record) }
    }
}

pub struct MediaState {
    cache: ImageCache,
    /// The app icon the title bar draws.
    logo: Image,
    pub images: HashMap<String, Image>,
    pub errors: HashMap<String, String>,
    attachments: HashMap<SendTarget, Vec<Attachment>>,
}

impl Default for MediaState {
    fn default() -> Self {
        let cache = ImageCache::new(Arc::new(|| {
            if let Some(wake) = crate::WAKER.get() { wake(); }
        }));
        Self {
            logo: cache.load(app_backend::appearance::AppIcon::default().png()),
            cache,
            images: HashMap::new(),
            errors: HashMap::new(),
            attachments: HashMap::new(),
        }
    }
}

impl MediaState {
    pub fn logo(&self) -> &Image {
        &self.logo
    }


    pub fn loaded(&mut self, key: String, bytes: Vec<u8>) {
        self.errors.remove(&key);
        self.images.insert(key, self.cache.load(bytes));
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

pub fn attachments<A: Actions + 'static>(s: &UiState<A>, target: &SendTarget) -> El {
    let select_target = target.clone();
    let mut body = col().key("attachments").w_full().gap(4.0)
        .child(row().key("attach-file").padding(6.0).on_press_tx(move |_: &mut UiState<A>, tx| {
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
        }).child(text("Attach file")));
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
                if let Some(files) = s.media.attachments.get_mut(&target) {
                    if index < files.len() {
                        if let Some(uploaded) = files.remove(index).uploaded { s.actions.discard_attachment(uploaded); }
                    }
                }
            }).child(text("Remove"))));
    }
    body
}

pub fn view<A: Actions + 'static>(s: &UiState<A>, content: &Content, message_id: &str) -> El {
    let mut body = col().key(format!("media-{message_id}")).w_full().gap(6.0);
    for (index, image) in content.images.iter().enumerate() {
        let key = image.key().to_owned();
        let mut picture = col().key(format!("image-{message_id}-{index}")).w_full().h(220.0);
        match s.media.images.get(&key) {
            Some(image) if image.error().is_none() => {
                picture = picture.pixels_fit(image.pixels(), SurfaceFit::Fit);
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
    body
}

fn is_web_url(value: &str) -> bool {
    value.starts_with("https://") || value.starts_with("http://")
}

#[cfg(test)]
mod tests {
    use super::*;

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
        show(&mut rt, message("m", Content { images: vec![image.clone()], files: vec![] }));
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
        show(&mut rt, message("m", Content { images: vec![], files }));
        assert!(rt.find("file-open-m-0").is_some() && rt.find("file-save-m-0").is_some());
        assert!(rt.find("file-open-m-1").is_some(), "loop components open in the web app");
        assert!(rt.find("file-save-m-1").is_none(), "loop components are not downloadable");
        assert!(rt.find("file-open-m-2").is_none(), "local paths never get an open action");
        assert!(rt.find("file-save-m-2").is_none());
    }
}
