//! Loading the icons that need the host's help (#142): web images, which
//! Pane downloads, and system icons, which it extracts. A list never waits
//! for them: [`IconLoads::want`] starts what an open command's items need
//! when their looks are remembered, and [`IconLoads::shown`] says what each
//! icon shows now, its fallback (or a neutral placeholder) until the image
//! is ready and for good if it fails. Each load that ends tells the window
//! the launcher changed, so the row draws its image.
//!
//! - **Web images**, by `http(s)` URL, are downloaded through Pane's HTTP
//!   client within the ceilings of the extension's own web requests (ADR
//!   0018: the response's head, its pieces, the whole request and a
//!   4 MiB body), and kept as the package's extension cache
//!   ([`ExtensionData::web_images`]): "Clear cache" removes them, and a
//!   restart finds them without downloading them again. A download that
//!   fails, is over the limits or is not an image leaves the fallback. One
//!   URL is downloaded once per package, however many rows name it. A
//!   download is on its package's generation's undo list: disabling,
//!   updating or uninstalling the package stops it. A command built into
//!   Pane has no cache, so its web images show their fallbacks.
//! - **System icons**, by path, are extracted by the host
//!   ([`crate::system_icons`]) once per path and version of the file, and
//!   kept as PNGs in Pane's own folder beside the installed packages.
//!
//! Loads run on a few threads of their own, never the window's or the
//! extension runtime's.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::UNIX_EPOCH;

use crate::extension_data::ExtensionData;
use crate::generation::Generation;
use crate::http::{self, GetError, Network};
use crate::icons::{
    Color, Icon, IconSource, Tint, Tone, WEB_IMAGE_KINDS, image_kind, is_web_url, web_image_stem,
};
use crate::packages::PackageIdentity;
use crate::system_icons::{NativeIcons, SystemIcon, SystemIcons};

/// How many loads run at once.
const WORKERS: usize = 4;

/// The folder beside the installed packages holding the system icons Pane
/// extracted.
const SYSTEM_ICONS_DIR: &str = "system-icons";

/// What the window is told when a load ended.
pub(super) type Changed = Arc<dyn Fn() + Send + Sync>;

/// The launcher's icon loads: what each web image and system icon is now,
/// and the threads loading them. Cloning shares them.
#[derive(Clone)]
pub(super) struct IconLoads {
    shared: Arc<Shared>,
}

struct Shared {
    /// Where the installed packages' data is: their web images' folders,
    /// and their generations. `None` for a launcher that installs no
    /// packages.
    data: Option<ExtensionData>,
    /// Where extracted system icons are kept; `None` likewise.
    system_folder: Option<PathBuf>,
    /// The runtime's web requests: the ceilings downloads keep to, and
    /// each package's network use.
    network: Option<Arc<Network>>,
    /// Tells the window the launcher changed.
    changed: Changed,
    /// Extracts system icons: the system's, or a test's.
    system_icons: Mutex<Arc<dyn SystemIcons>>,
    state: Mutex<Loads>,
}

#[derive(Default)]
struct Loads {
    /// Web images by package (its identity key) and URL.
    web: HashMap<(String, String), Load>,
    /// System icons by path.
    system: HashMap<PathBuf, Load>,
    queue: VecDeque<Job>,
    /// Threads loading now.
    workers: usize,
}

/// Where one image is.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Load {
    Loading,
    /// Ready to draw: the image's file.
    Ready(PathBuf),
    /// It could not be loaded: its fallback shows.
    Failed,
}

/// One load to run.
enum Job {
    Web {
        owner: String,
        url: String,
        folder: PathBuf,
        generation: Generation,
    },
    System {
        path: PathBuf,
    },
}

impl IconLoads {
    /// The loads of a launcher whose packages' data is `data` (beside
    /// which, in `folder`, system icons are kept), downloading through
    /// `network`, telling the window of each load that ended with
    /// `changed`.
    pub(super) fn new(
        data: Option<(ExtensionData, PathBuf)>,
        network: Option<Arc<Network>>,
        changed: Changed,
    ) -> IconLoads {
        let (data, system_folder) = match data {
            Some((data, folder)) => (Some(data), Some(folder.join(SYSTEM_ICONS_DIR))),
            None => (None, None),
        };
        IconLoads {
            shared: Arc::new(Shared {
                data,
                system_folder,
                network,
                changed,
                system_icons: Mutex::new(Arc::new(NativeIcons)),
                state: Mutex::new(Loads::default()),
            }),
        }
    }

    /// Extracts system icons with `system_icons` from now on, forgetting
    /// those extracted before.
    pub(super) fn set_system_icons(&self, system_icons: Arc<dyn SystemIcons>) {
        *lock(&self.shared.system_icons) = system_icons;
        self.shared.lock().system.clear();
    }

    /// Starts loading what `icon` and its fallbacks need that is not
    /// loaded or loading yet, for the package with identity `owner`
    /// (`None` for a command built into Pane).
    pub(super) fn want(&self, owner: Option<&PackageIdentity>, icon: &Icon) {
        let mut next = Some(icon);
        while let Some(icon) = next {
            match &icon.source {
                IconSource::Url(url) if is_web_url(url) => self.want_web(owner, url),
                IconSource::File(path) => self.want_system(path),
                _ => {}
            }
            next = icon.fallback.as_deref();
        }
    }

    fn want_web(&self, identity: Option<&PackageIdentity>, url: &str) {
        let shared = &self.shared;
        let (Some(identity), Some(data)) = (identity, &shared.data) else {
            // Built into Pane: no cache to keep it in.
            return;
        };
        let owner = identity.key();
        let key = (owner.clone(), url.to_owned());
        let mut state = shared.lock();
        if let Some(Load::Loading | Load::Failed) = state.web.get(&key) {
            return;
        }
        if let Some(Load::Ready(file)) = state.web.get(&key)
            && file.is_file()
        {
            return;
        }
        // Downloaded before, this session or another: kept on disk.
        let folder = data.web_images(&owner);
        let stem = web_image_stem(url);
        let kept = WEB_IMAGE_KINDS
            .iter()
            .map(|kind| folder.join(format!("{stem}.{kind}")))
            .find(|file| file.is_file());
        if let Some(file) = kept {
            state.web.insert(key, Load::Ready(file));
            return;
        }
        state.web.insert(key, Load::Loading);
        let generation = data.owned_by(identity).generation().clone();
        state.queue.push_back(Job::Web {
            owner,
            url: url.to_owned(),
            folder,
            generation,
        });
        self.start_worker(&mut state);
    }

    fn want_system(&self, path: &Path) {
        if self.shared.system_folder.is_none() {
            return;
        }
        let mut state = self.shared.lock();
        match state.system.get(path) {
            Some(Load::Loading | Load::Failed) => return,
            Some(Load::Ready(file)) if file.is_file() => return,
            _ => {}
        }
        state.system.insert(path.to_path_buf(), Load::Loading);
        state.queue.push_back(Job::System {
            path: path.to_path_buf(),
        });
        self.start_worker(&mut state);
    }

    /// Starts another thread loading, unless enough are.
    fn start_worker(&self, state: &mut Loads) {
        if state.workers >= WORKERS {
            return;
        }
        state.workers += 1;
        let shared = self.shared.clone();
        let started = std::thread::Builder::new()
            .name("pane-icon-loads".into())
            .spawn(move || shared.work());
        if let Err(error) = started {
            state.workers -= 1;
            eprintln!("pane: could not start a thread loading icons: {error}");
        }
    }

    /// `icon` as a row shows it now, for the package with identity key
    /// `owner`: a web image or a system icon that is ready as its image
    /// file; one loading or failed as its fallback (shown alike), or a
    /// neutral placeholder without one, keeping its tooltip. Anything else
    /// as it is.
    pub(super) fn shown(&self, owner: Option<&str>, icon: &Icon) -> Icon {
        let fallback = icon
            .fallback
            .as_deref()
            .map(|fallback| self.shown(owner, fallback));
        let file = match &icon.source {
            IconSource::Url(url) if is_web_url(url) => Some(owner.and_then(|owner| {
                let key = (owner.to_owned(), url.clone());
                match self.shared.lock().web.get(&key) {
                    Some(Load::Ready(file)) => Some(file.clone()),
                    _ => None,
                }
            })),
            IconSource::File(path) => Some(match self.shared.lock().system.get(path) {
                Some(Load::Ready(file)) => Some(file.clone()),
                _ => None,
            }),
            _ => None,
        };
        let source = match file {
            // Not one Pane loads: as it is.
            None => icon.source.clone(),
            Some(Some(file)) => IconSource::Image {
                light: file.clone(),
                dark: file,
            },
            Some(None) => {
                let mut stand_in = fallback.unwrap_or_else(placeholder);
                if stand_in.tooltip.is_none() {
                    stand_in.tooltip = icon.tooltip.clone();
                }
                return stand_in;
            }
        };
        Icon {
            source,
            tint: icon.tint,
            mask: icon.mask,
            fallback: fallback.map(Box::new),
            tooltip: icon.tooltip.clone(),
        }
    }

    /// Forgets the web images of the package with identity key `owner`,
    /// whose cache was cleared: the next list naming them downloads them
    /// again.
    pub(super) fn forget(&self, owner: &str) {
        self.shared.lock().web.retain(|(of, _), _| of != owner);
    }

    /// Waits up to `limit` until no load is queued or running; whether none
    /// is. For tests.
    #[cfg(any(test, debug_assertions))]
    pub(super) fn wait_idle(&self, limit: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + limit;
        loop {
            {
                let state = self.shared.lock();
                if state.workers == 0 && state.queue.is_empty() {
                    return true;
                }
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}

impl super::Launcher {
    /// This launcher extracting system icons with `system_icons` instead
    /// of the system's adapter ([`NativeIcons`]): a test's stand-in.
    pub fn with_system_icons(self, system_icons: Arc<dyn SystemIcons>) -> Self {
        self.lock().icon_loads.set_system_icons(system_icons);
        self
    }

    /// Waits up to `limit` until no web image or system icon is loading;
    /// whether none is. For tests; debug builds only.
    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn wait_for_icons(&self, limit: std::time::Duration) -> bool {
        let loads = self.lock().icon_loads.clone();
        loads.wait_idle(limit)
    }
}

/// What an icon shows while its image loads, or when it failed, if it has
/// no fallback: a neutral image glyph in the secondary tone.
fn placeholder() -> Icon {
    Icon {
        tint: Some(Tint::Same(Color::Tone(Tone::Secondary))),
        ..Icon::new(IconSource::Builtin {
            name: "image".into(),
            filled: false,
        })
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Loads> {
        lock(&self.state)
    }

    /// A loading thread: runs queued loads until none is left.
    fn work(&self) {
        loop {
            let job = {
                let mut state = self.lock();
                match state.queue.pop_front() {
                    Some(job) => job,
                    None => {
                        state.workers -= 1;
                        return;
                    }
                }
            };
            match job {
                Job::Web {
                    owner,
                    url,
                    folder,
                    generation,
                } => {
                    let loaded = self.download(&owner, &url, &folder, &generation);
                    if let Err(why) = &loaded {
                        eprintln!("pane: the web image {url} shows its fallback: {why}");
                    }
                    let load = loaded.map_or(Load::Failed, Load::Ready);
                    self.lock().web.insert((owner, url), load);
                }
                Job::System { path } => {
                    let load = self.extract(&path).map_or(Load::Failed, Load::Ready);
                    self.lock().system.insert(path, load);
                }
            }
            (self.changed)();
        }
    }

    /// Downloads the web image at `url` for the package with identity key
    /// `owner` into `folder`, unless its `generation` ended first; the
    /// image's file.
    fn download(
        &self,
        owner: &str,
        url: &str,
        folder: &Path,
        generation: &Generation,
    ) -> Result<PathBuf, String> {
        let network = self
            .network
            .as_ref()
            .ok_or("Pane's extension runtime is not running")?;
        if let Some(address) = http::address_of(url) {
            network.note_contacted(owner, address);
        }
        // On the generation's undo list while it downloads: its end stops
        // it.
        let (abort, aborted) = tokio::sync::oneshot::channel::<()>();
        let _undo = generation.on_end("web image", move || {
            let _ = abort.send(());
            Ok(())
        });
        let answer =
            http::get_for_extension(url, network.limits(), Some(aborted)).map_err(|error| {
                match error {
                    GetError::TooLarge => "it is larger than an extension may download".to_owned(),
                    GetError::Failed(why) => why,
                }
            })?;
        if !(200..300).contains(&answer.status) {
            return Err(format!("the server answered {}", answer.status));
        }
        let kind = image_kind(&answer.body).ok_or("it is not an image Pane draws")?;
        if generation.ended().is_some() {
            return Err("its extension stopped".into());
        }
        let file = folder.join(format!("{}.{kind}", web_image_stem(url)));
        crate::atomic::write_atomically(&file, &answer.body, crate::atomic::Readers::Default)
            .map_err(|error| format!("cannot keep it: {error}"))?;
        Ok(file)
    }

    /// Extracts the system icon of `path`, keeping it as a PNG in Pane's
    /// folder; the image's file.
    fn extract(&self, path: &Path) -> Result<PathBuf, String> {
        let folder = self
            .system_folder
            .as_ref()
            .ok_or("this launcher keeps no system icons")?;
        // Keyed by the path and the file's version: a changed application
        // is extracted again.
        let modified = std::fs::metadata(path)
            .ok()
            .map(|metadata| {
                let at = metadata
                    .modified()
                    .ok()
                    .and_then(|at| at.duration_since(UNIX_EPOCH).ok())
                    .map_or(0, |at| at.as_millis());
                format!("{at}:{}", metadata.len())
            })
            .unwrap_or_default();
        let key = format!("{}\n{modified}", path.display());
        let file = folder.join(format!("{}.png", web_image_stem(&key)));
        if file.is_file() {
            return Ok(file);
        }
        let system_icons = lock(&self.system_icons).clone();
        match system_icons.icon(path) {
            Ok(SystemIcon::File(image)) => Ok(image),
            Ok(SystemIcon::Png(png)) => {
                crate::atomic::write_atomically(&file, &png, crate::atomic::Readers::Default)
                    .map_err(|error| format!("cannot keep it: {error}"))?;
                Ok(file)
            }
            Err(why) => Err(why),
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loads() -> IconLoads {
        IconLoads::new(None, None, Arc::new(|| {}))
    }

    #[test]
    fn an_image_not_loaded_shows_its_fallback_or_a_placeholder_with_its_tooltip() {
        let loads = loads();
        let web = |fallback: Option<Icon>| Icon {
            fallback: fallback.map(Box::new),
            tooltip: Some("Logo".into()),
            ..Icon::new(IconSource::Url("https://example.com/logo.png".into()))
        };
        let star = Icon::new(IconSource::Builtin {
            name: "star".into(),
            filled: false,
        });
        let shown = loads.shown(Some("owner"), &web(Some(star.clone())));
        assert_eq!(shown.source, star.source);
        assert_eq!(shown.tooltip.as_deref(), Some("Logo"));
        let shown = loads.shown(Some("owner"), &web(None));
        assert_eq!(
            shown.source,
            IconSource::Builtin {
                name: "image".into(),
                filled: false
            }
        );
        assert_eq!(shown.tooltip.as_deref(), Some("Logo"));
        // Anything else shows as it is.
        assert_eq!(loads.shown(None, &star), star);
    }

    #[test]
    fn a_ready_image_shows_its_file_with_the_icons_tint_and_mask() {
        let loads = loads();
        let file = PathBuf::from("/cache/abc.png");
        let url = "https://example.com/a.png".to_owned();
        loads
            .shared
            .lock()
            .web
            .insert(("owner".into(), url.clone()), Load::Ready(file.clone()));
        let icon = Icon {
            mask: Some(crate::icons::Mask::Circle),
            ..Icon::new(IconSource::Url(url))
        };
        let shown = loads.shown(Some("owner"), &icon);
        assert_eq!(
            shown.source,
            IconSource::Image {
                light: file.clone(),
                dark: file
            }
        );
        assert_eq!(shown.mask, Some(crate::icons::Mask::Circle));
        // Another package's is its own.
        assert!(matches!(
            loads.shown(Some("other"), &icon).source,
            IconSource::Builtin { .. }
        ));
        loads.forget("owner");
        assert!(matches!(
            loads.shown(Some("owner"), &icon).source,
            IconSource::Builtin { .. }
        ));
    }
}
