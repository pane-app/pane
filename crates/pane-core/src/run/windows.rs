//! The Run dialog's work on Windows (see the parent module): a Control
//! Panel applet runs through the Control Panel program, a bare name is
//! resolved on the search path the registry names at the time of the call
//! (machine and user, as the programs module reads it) and in App Paths —
//! so a tool installed after Pane started is found — and everything else
//! (programs, documents, folders, network paths, `shell:` and
//! `ms-settings:` addresses and other registered schemes) goes through
//! the system's own open (`ShellExecuteExW`, which also opens a folder in
//! File Explorer). An elevated run goes through the same call with the
//! `runas` verb (Windows' own prompt, ADR 0033), waiting only until the
//! shell has started it. The history is Explorer's RunMRU key in its own
//! format (`mru`); the key and the search path are given to the adapter,
//! so the tests use keys and paths of their own, never the user's. The
//! completions are gathered from the same places at each call: the
//! RunMRU key, App Paths' names, the search path's folders, the classes
//! root's registered schemes and the environment's variable names, each
//! read as it is and matched by the pure half. Windows Terminal, for a
//! command that runs a command line in a terminal, is `wt.exe` as App
//! Paths and then the search path spell it.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Environment::ExpandEnvironmentStringsW;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_ENUMERATE_SUB_KEYS,
    KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ,
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegEnumKeyW, RegGetValueW, RegOpenKeyExW,
    RegSetValueExW,
};
use windows::Win32::UI::Shell::{
    SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{HSTRING, PCWSTR};

use super::parse::Sources;
use super::{
    Candidates, Completion, Run, RunError, Target, classify, complete, mru, normalize, parse,
};
use crate::programs::runner::{ErrorKind, SearchPath};
use crate::programs::search;
use crate::util::wide;
use crate::windows_shell::Com;

/// Explorer's RunMRU key, under `HKEY_CURRENT_USER`: the Run dialog's own
/// history.
const EXPLORER_RUN_MRU: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\RunMRU";

/// The value that names the letters in the order the dialog lists them.
const MRU_LIST: &str = "MRUList";

/// Where App Paths registers a program by its name, in the machine's and
/// the user's software.
const APP_PATHS: &str = r"Software\Microsoft\Windows\CurrentVersion\App Paths";

/// The Control Panel program, which opens a Control Panel applet, found
/// by its bare name on the search path.
const CONTROL: &str = "control";

/// Windows Terminal's program, as App Paths registers it: what a command
/// that runs a command line in a terminal opens its tab with.
const TERMINAL: &str = "wt.exe";

/// The value that marks a classes-root key as a registered scheme.
const URL_PROTOCOL: &str = "URL Protocol";

/// The extensions a program's name takes when it has none, as `PATHEXT`
/// spells them: what the search path's program completions resolve
/// through.
const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";

/// The adapter: the Run dialog's history in the registry, and the search
/// path its bare names are found on.
pub struct WindowsRun {
    /// The registry subkey under `HKEY_CURRENT_USER` holding the Run
    /// dialog's history: Explorer's own, or a test's own key.
    mru: String,
    /// Where programs named by a bare name are found, asked at each
    /// call: the registry's search path, or a test's own.
    search_path: SearchPath,
}

impl WindowsRun {
    /// Windows' own Run dialog history and search path.
    pub fn new() -> WindowsRun {
        WindowsRun {
            mru: EXPLORER_RUN_MRU.into(),
            search_path: Arc::new(search::system_search_path),
        }
    }

    /// With `mru` as the registry subkey of the Run dialog's history and
    /// `search_path` as where bare names are found: the tests give a key
    /// of their own, never the user's, and a path of their own.
    pub fn with(mru: String, search_path: SearchPath) -> WindowsRun {
        WindowsRun { mru, search_path }
    }

    /// The file a bare name names, resolved on the search path the
    /// registry names, then in App Paths, as the Run dialog finds a
    /// program by its name.
    fn resolve(&self, name: &str) -> Result<PathBuf, RunError> {
        let search_path = (self.search_path)();
        match search::resolve(name, &search_path) {
            Ok(found) => Ok(found),
            // App Paths is the Run dialog's other source: what a
            // program's installer registered for its name.
            Err(error) if error.kind == ErrorKind::NotFound => match app_paths(name) {
                Some(found) => Ok(found),
                None => Err(RunError::Failed(error.message)),
            },
            Err(error) => Err(RunError::Failed(error.message)),
        }
    }

    /// The Control Panel program's path, found on the search path, which
    /// a Control Panel applet runs through.
    fn control(&self) -> Result<String, RunError> {
        match search::resolve(CONTROL, &(self.search_path)()) {
            Ok(found) => Ok(normalize::normalize(&found.to_string_lossy(), &real_case)),
            Err(error) => Err(RunError::Failed(error.message)),
        }
    }

    /// Records `line` in the Run dialog's history, newest first.
    fn record(&self, line: &str) -> Result<(), RunError> {
        let entries = read_history(&self.mru)?;
        let recorded = mru::record(entries, line);
        write_history(&self.mru, &recorded)
    }
}

impl Default for WindowsRun {
    fn default() -> WindowsRun {
        WindowsRun::new()
    }
}

impl Run for WindowsRun {
    fn run(&self, line: &str, elevated: bool) -> Result<(), RunError> {
        let line = line.trim();
        let split = parse::split(line, &WindowsSources)?;
        let head = split.head;
        let arguments = split.arguments;
        let launch = match classify::classify(&head) {
            // A Control Panel applet, through the Control Panel program.
            Target::Applet { path } => applet(
                &self.control()?,
                &normalize::normalize(&path, &real_case),
                &arguments,
            ),
            // A bare name, resolved on the search path and in App Paths;
            // an applet's own name resolves to one, run the same way.
            Target::Program { name } => match self.resolve(&name)? {
                found if is_applet(&found) => applet(
                    &self.control()?,
                    &normalize::normalize(&found.to_string_lossy(), &real_case),
                    &arguments,
                ),
                found => Launch {
                    file: normalize::normalize(&found.to_string_lossy(), &real_case),
                    parameters: arguments,
                },
            },
            // A program, document, folder or network path: the system's
            // own open, which says itself when nothing is there.
            Target::Path { path } => {
                let path = normalize::normalize(&path, &real_case);
                if !Path::new(&path).exists() {
                    return Err(RunError::Failed(format!("{path} does not exist")));
                }
                Launch {
                    file: path,
                    parameters: arguments,
                }
            }
            // A `shell:`, `ms-settings:` or other registered address.
            Target::Address { address } => Launch {
                file: address,
                parameters: arguments,
            },
        };
        execute(&launch, elevated, line)?;
        // The run answered: the Run dialog's history records what ran. A
        // history that could not be written fails the answer, saying the
        // run itself happened.
        self.record(line).map_err(|error| match error {
            RunError::Failed(why) => RunError::Failed(format!(
                "Windows ran it, but the Run dialog's history could not be written: {why}"
            )),
            other => other,
        })
    }

    fn history(&self) -> Result<Vec<String>, RunError> {
        read_history(&self.mru)
    }

    fn delete_from_history(&self, line: &str) -> Result<(), RunError> {
        let line = line.trim();
        let entries = read_history(&self.mru)?;
        match mru::remove(&entries, line) {
            Some(without) => write_history(&self.mru, &without),
            None => Err(RunError::Failed(format!(
                "“{line}” is not in the Run dialog’s history"
            ))),
        }
    }

    fn completions(&self, text: &str) -> Result<Vec<Completion>, RunError> {
        // The sources are read as they are at the time of the call, so a
        // tool installed after Pane started is completed; a source that
        // cannot be read contributes nothing, its own function reporting
        // why — the history's own read is the one that can fail here, and
        // a history that cannot be read holds nothing to complete.
        let (programs, applets, consoles) = on_path(&(self.search_path)());
        let candidates = Candidates {
            history: read_history(&self.mru).unwrap_or_default(),
            app_paths: app_path_names(),
            programs,
            applets,
            consoles,
            schemes: scheme_names(text),
            variables: variable_names(),
        };
        Ok(complete(&candidates, text))
    }

    fn terminal(&self) -> Result<Option<String>, RunError> {
        // App Paths is where Windows Terminal's installer registers it;
        // the search path is the other place its name resolves. Neither
        // starts it: the caller opens the tab.
        let found =
            app_paths(TERMINAL).or_else(|| search::resolve(TERMINAL, &(self.search_path)()).ok());
        Ok(found.map(|found| normalize(&found.to_string_lossy(), &real_case)))
    }
}

/// What the run starts: the file the shell opens, and its parameters.
struct Launch {
    /// The file `ShellExecuteExW` opens: a program's path, a document, a
    /// folder, a network path, or an address such as `shell:windows`.
    file: String,
    /// Its parameters: the command line's arguments, or the Control Panel
    /// program's applet and its arguments.
    parameters: String,
}

/// An applet's launch: the Control Panel program opens `path`, with the
/// command line's arguments after it.
fn applet(control: &str, path: &str, arguments: &str) -> Launch {
    let mut parameters = format!("\"{path}\"");
    if !arguments.is_empty() {
        parameters.push(' ');
        parameters.push_str(arguments);
    }
    Launch {
        file: control.to_owned(),
        parameters,
    }
}

/// Why the run could not ask Windows to start `shown`: COM could not be
/// started on this thread, saying `why`.
fn not_started(shown: &str, why: String) -> RunError {
    RunError::Failed(format!("Windows did not start {shown}: {why}"))
}

/// Whether `path` names a Control Panel applet (`.cpl`).
fn is_applet(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("cpl"))
}

/// Starts `launch` as the Run dialog does, with the `runas` verb when
/// `elevated` — Windows' own prompt, which the user may decline —
/// waiting only until the shell has started it, naming what it started
/// `shown` in its answers.
fn execute(launch: &Launch, elevated: bool, shown: &str) -> Result<(), RunError> {
    let _com = Com::new().map_err(|why| not_started(shown, why))?;
    let file = wide(&launch.file);
    let parameters = wide(&launch.parameters);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        // Wait until the shell has started it (this thread ends next),
        // and report a failure here instead of in a dialog.
        fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: if elevated {
            windows::core::w!("runas")
        } else {
            PCWSTR::null()
        },
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(parameters.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    // SAFETY: `info` is initialized with its size, and its strings are
    // NUL-terminated and outlive the call.
    let started = unsafe { ShellExecuteExW(&mut info) };
    if let Err(error) = started {
        if elevated {
            return Err(super::elevated_answer(
                error.code().0,
                &error.message(),
                shown,
            ));
        }
        return Err(RunError::Failed(format!(
            "Windows did not start {shown}: {}",
            error.message()
        )));
    }
    Ok(())
}

/// The parser's file system and environment on Windows: what exists on
/// the disk, and Windows' own expansion of `%NAME%`.
struct WindowsSources;

impl Sources for WindowsSources {
    fn exists(&self, path: &str) -> bool {
        Path::new(path).exists()
    }

    fn expand(&self, text: &str) -> String {
        expand(text)
    }
}

/// `text` with each `%NAME%` expanded by Windows, as the Run dialog
/// expands them; a text Windows cannot expand is answered as it is.
fn expand(text: &str) -> String {
    let given = wide(text);
    // SAFETY: asks for the size only; `given` is NUL-terminated.
    let size = unsafe { ExpandEnvironmentStringsW(PCWSTR(given.as_ptr()), None) };
    if size == 0 {
        return text.to_owned();
    }
    let mut answer = vec![0u16; size as usize];
    // SAFETY: `answer` holds `size` units for the call to write.
    let written =
        unsafe { ExpandEnvironmentStringsW(PCWSTR(given.as_ptr()), Some(answer.as_mut_slice())) };
    if written == 0 {
        return text.to_owned();
    }
    let length = answer
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(answer.len());
    String::from_utf16_lossy(&answer[..length])
}

/// `path`'s real casing, as the file system spells it, following links to
/// where they point; a path that is not on disk is spelled as given.
fn real_case(path: &str) -> String {
    match std::fs::canonicalize(path) {
        Ok(real) => match real.to_str() {
            Some(real) => unverbatim(real),
            // A path that is not valid Unicode keeps its spelling.
            None => path.to_owned(),
        },
        Err(_) => path.to_owned(),
    }
}

/// `real`, as `canonicalize` answered it, without its verbatim prefix:
/// `\\?\C:\…` is `C:\…` and `\\?\UNC\server\…` is `\\server\…`.
fn unverbatim(real: &str) -> String {
    if let Some(unc) = real.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{unc}");
    }
    real.strip_prefix(r"\\?\")
        .map_or_else(|| real.to_owned(), str::to_owned)
}

/// The program App Paths names `name`, looking in the machine's and the
/// user's `Software\Microsoft\Windows\CurrentVersion\App Paths`: the
/// value for the name — tried with `.exe` when the name has no extension
/// — is the program's full path, its variables expanded, quoted or not.
/// `None` when neither holds a program that exists.
fn app_paths(name: &str) -> Option<PathBuf> {
    let mut candidates = vec![name.to_owned()];
    if Path::new(name).extension().is_none() {
        candidates.push(format!("{name}.exe"));
    }
    for candidate in candidates {
        for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
            let subkey = format!(r"{APP_PATHS}\{candidate}");
            if let Some(registered) = registry_string(root, &subkey, None) {
                let trimmed = registered.trim().trim_matches('"');
                let program = PathBuf::from(trimmed);
                if program.is_file() {
                    return Some(program);
                }
            }
        }
    }
    None
}

/// The names of the subkeys of the key `subkey` under `root`, in the
/// order the registry lists them; empty when the key cannot be opened.
/// A key name is at most 255 characters.
fn subkey_names(root: HKEY, subkey: &str) -> Vec<String> {
    let given = wide(subkey);
    let mut key = HKEY::default();
    // SAFETY: `key` is a handle the call fills in when it succeeds.
    let opened = unsafe {
        RegOpenKeyExW(
            root,
            PCWSTR(given.as_ptr()),
            None,
            KEY_ENUMERATE_SUB_KEYS,
            &mut key,
        )
    };
    if opened != ERROR_SUCCESS {
        return Vec::new();
    }
    let mut names = Vec::new();
    let mut at: u32 = 0;
    loop {
        let mut buffer = [0u16; 256];
        // SAFETY: `key` is open for enumeration, and `buffer` holds room
        // for a key name with its NUL.
        let read = unsafe { RegEnumKeyW(key, at, Some(&mut buffer)) };
        if read != ERROR_SUCCESS {
            break;
        }
        let length = buffer
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(buffer.len());
        names.push(String::from_utf16_lossy(&buffer[..length]));
        at += 1;
    }
    // SAFETY: the handle the open returned, closed once here.
    unsafe {
        let _ = RegCloseKey(key);
    }
    names
}

/// The names App Paths registered programs for, as completion lines: the
/// subkeys of the machine's and the user's App Paths whose registered
/// program exists, each named as the Run dialog takes it — a name with
/// an extension without it, so `wt.exe` offers `wt`, which the Run
/// dialog finds there again.
fn app_path_names() -> Vec<String> {
    let mut names = Vec::new();
    for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        for name in subkey_names(root, APP_PATHS) {
            let subkey = format!(r"{APP_PATHS}\{name}");
            let registered = registry_string(root, &subkey, None)
                .map(|value| PathBuf::from(value.trim().trim_matches('"')));
            // A name whose program is not there is not offered: one whose
            // registration is gone does not complete.
            if registered.is_some_and(|program| program.is_file()) {
                names.push(without_extension(&name));
            }
        }
    }
    sorted(names)
}

/// `name`, as the Run dialog takes it: a name with an extension without
/// it, a name without one as it is.
fn without_extension(name: &str) -> String {
    Path::new(name).file_stem().map_or_else(
        || name.to_owned(),
        |stem| stem.to_string_lossy().into_owned(),
    )
}

/// What the search path's folders hold for the completions, read at each
/// call so a tool installed after Pane started is completed: the names of
/// their programs — each without the extension the search path resolves,
/// as the Run dialog takes a bare name — their Control Panel applets and
/// their management consoles, by their file names, which the Run dialog
/// runs as they are. An applet and a console are that whatever `PATHEXT`
/// spells — a machine whose `PATHEXT` names `.MSC`, as Windows' own does,
/// still completes the console by its file name, never as a bare program
/// the search path would resolve.
fn on_path(search_path: &OsStr) -> (Vec<String>, Vec<String>, Vec<String>) {
    let extensions = program_extensions();
    let mut programs = Vec::new();
    let mut applets = Vec::new();
    let mut consoles = Vec::new();
    for folder in std::env::split_paths(search_path).filter(|folder| folder.is_absolute()) {
        // A folder that cannot be read holds nothing to complete.
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some((_, extension)) = name.rsplit_once('.') else {
                continue;
            };
            if extension.eq_ignore_ascii_case("cpl") {
                applets.push(name);
            } else if extension.eq_ignore_ascii_case("msc") {
                consoles.push(name);
            } else if extensions
                .iter()
                .any(|known| known.eq_ignore_ascii_case(extension))
                && let Some(stem) = Path::new(&name).file_stem()
                && !stem.is_empty()
            {
                programs.push(stem.to_string_lossy().into_owned());
            }
        }
    }
    (sorted(programs), sorted(applets), sorted(consoles))
}

/// The extensions a program's name takes when it has none, as `PATHEXT`
/// spells them, without their dots and in lower case.
fn program_extensions() -> Vec<String> {
    std::env::var("PATHEXT")
        .unwrap_or_else(|_| DEFAULT_PATHEXT.to_owned())
        .split(';')
        .map(str::trim)
        .filter(|extension| extension.starts_with('.') && extension.len() > 1)
        .map(|extension| extension[1..].to_ascii_lowercase())
        .collect()
}

/// The registered schemes the typed text can complete to, as lines
/// (`ms-settings:`): the classes root's keys whose offered line starts
/// with the typed text, ignoring case — only those are read, so the
/// classes root's many keys are enumerated but not each opened — and
/// that carry the `URL Protocol` marker, as a registered scheme does.
fn scheme_names(text: &str) -> Vec<String> {
    let text = text.trim().to_lowercase();
    if text.is_empty() {
        return Vec::new();
    }
    let mut schemes = Vec::new();
    for name in subkey_names(HKEY_CLASSES_ROOT, "") {
        // The line a scheme offers is its name with the `:` the Run
        // dialog runs it with.
        let line = format!("{name}:");
        if !line.to_lowercase().starts_with(&text) {
            continue;
        }
        if registry_string(HKEY_CLASSES_ROOT, &name, Some(URL_PROTOCOL)).is_some() {
            schemes.push(line);
        }
    }
    sorted(schemes)
}

/// The environment's variable names, as the `%NAME%` lines the Run
/// dialog expands.
fn variable_names() -> Vec<String> {
    sorted(
        std::env::vars()
            .map(|(name, _)| format!("%{name}%"))
            .collect(),
    )
}

/// `names`, sorted ignoring case, so a source the registry or the file
/// system lists in its own order answers the same every time. The
/// history keeps its own order, newest first.
fn sorted(mut names: Vec<String>) -> Vec<String> {
    names.sort_by_key(|name| name.to_lowercase());
    names
}

/// The history the RunMRU key `mru` holds, decoded; a key that does not
/// exist holds none, since nothing was ever recorded in it.
fn read_history(mru: &str) -> Result<Vec<String>, RunError> {
    let mut values = Vec::new();
    for letter in mru::LETTERS {
        let name = letter.to_string();
        match read_string(mru, &name) {
            Ok(Some(data)) => values.push((name, data)),
            Ok(None) => {}
            Err(problem) => return Err(problem),
        }
    }
    let list = read_string(mru, MRU_LIST)?.unwrap_or_default();
    Ok(mru::decode(&values, &list))
}

/// The string value `value` of the RunMRU key `mru` under
/// `HKEY_CURRENT_USER`, expanded; `Ok(None)` when the key or the value is
/// not there.
fn read_string(mru: &str, value: &str) -> Result<Option<String>, RunError> {
    let problem = |result: WIN32_ERROR| {
        RunError::Failed(format!(
            "the Run dialog's history could not be read: error {}",
            result.0
        ))
    };
    let (subkey, value) = (HSTRING::from(mru), HSTRING::from(value));
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ;
    let mut bytes: u32 = 0;
    // SAFETY: asks for the size only; every pointer is valid or absent.
    let sized = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &subkey,
            &value,
            flags,
            None,
            None,
            Some(&raw mut bytes),
        )
    };
    if sized == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if sized != ERROR_SUCCESS || bytes == 0 {
        return Err(problem(sized));
    }
    for _ in 0..4 {
        let mut buffer = vec![0u16; (bytes as usize).div_ceil(2) + 1];
        let mut size = (buffer.len() * 2) as u32;
        // SAFETY: `buffer` holds `size` bytes and outlives the call.
        let read = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                &subkey,
                &value,
                flags,
                None,
                Some(buffer.as_mut_ptr().cast::<core::ffi::c_void>()),
                Some(&raw mut size),
            )
        };
        if read == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if read == ERROR_SUCCESS {
            let length = buffer
                .iter()
                .position(|&unit| unit == 0)
                .unwrap_or(buffer.len());
            return Ok(Some(String::from_utf16_lossy(&buffer[..length])));
        }
        if size <= bytes {
            return Err(problem(read));
        }
        bytes = size;
    }
    Err(problem(ERROR_SUCCESS))
}

/// Writes `entries` to the RunMRU key `mru`: the lettered values and
/// MRUList, the letters no entry takes deleted. Creates the key, since a
/// history nothing was recorded in has none.
fn write_history(mru: &str, entries: &[String]) -> Result<(), RunError> {
    let (values, list) = mru::encode(entries);
    let subkey = wide(mru);
    let mut key = HKEY::default();
    // SAFETY: `key` is a handle the call fills in when it succeeds.
    let created = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
    };
    let problem = |result: WIN32_ERROR| {
        RunError::Failed(format!(
            "the Run dialog's history could not be written: error {}",
            result.0
        ))
    };
    if created != ERROR_SUCCESS {
        return Err(problem(created));
    }
    let mut written = ERROR_SUCCESS;
    for letter in mru::LETTERS {
        let name = letter.to_string();
        let handle = HSTRING::from(name.as_str());
        let result = match values.iter().find(|(value, _)| *value == name) {
            // The letters name the entries in order, newest first.
            Some((_, data)) => set_string(key, &handle, data),
            // A letter no entry takes is deleted, whether or not it held
            // one.
            None => unsafe { RegDeleteValueW(key, &handle) },
        };
        if result != ERROR_SUCCESS && result != ERROR_FILE_NOT_FOUND {
            written = result;
            break;
        }
    }
    if written == ERROR_SUCCESS {
        written = set_string(key, &HSTRING::from(MRU_LIST), &list);
    }
    // SAFETY: the handle the create returned, asked for once here.
    unsafe {
        let _ = RegCloseKey(key);
    }
    if written != ERROR_SUCCESS {
        return Err(problem(written));
    }
    Ok(())
}

/// Writes `data` as the string value `name` of `key`, a plain
/// NUL-terminated UTF-16 string as Explorer's are; the call's result.
fn set_string(key: HKEY, name: &HSTRING, data: &str) -> WIN32_ERROR {
    let mut bytes: Vec<u8> = data.encode_utf16().flat_map(u16::to_le_bytes).collect();
    bytes.extend_from_slice(&[0, 0]);
    // SAFETY: `key` is open for writing, and `bytes` is a plain byte
    // buffer the call copies before returning.
    unsafe { RegSetValueExW(key, name, None, REG_SZ, Some(bytes.as_slice())) }
}

/// The string value named `value` (`None` for the default one) of the
/// subkey `subkey` under `root`, its variables expanded; `None` when the
/// key, the value or its data cannot be read.
fn registry_string(root: HKEY, subkey: &str, value: Option<&str>) -> Option<String> {
    let subkey = HSTRING::from(subkey);
    let value = value.map(HSTRING::from);
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ;
    let mut bytes: u32 = 0;
    // SAFETY: asks for the size only; every pointer is valid or absent.
    let sized = unsafe {
        RegGetValueW(
            root,
            &subkey,
            value
                .as_ref()
                .map_or(PCWSTR::null(), |value| PCWSTR(value.as_ptr())),
            flags,
            None,
            None,
            Some(&raw mut bytes),
        )
    };
    if sized != ERROR_SUCCESS || bytes == 0 {
        return None;
    }
    for _ in 0..4 {
        let mut buffer = vec![0u16; (bytes as usize).div_ceil(2) + 1];
        let mut size = (buffer.len() * 2) as u32;
        // SAFETY: `buffer` holds `size` bytes and outlives the call.
        let read = unsafe {
            RegGetValueW(
                root,
                &subkey,
                value
                    .as_ref()
                    .map_or(PCWSTR::null(), |value| PCWSTR(value.as_ptr())),
                flags,
                None,
                Some(buffer.as_mut_ptr().cast::<core::ffi::c_void>()),
                Some(&raw mut size),
            )
        };
        if read == ERROR_SUCCESS {
            let length = buffer
                .iter()
                .position(|&unit| unit == 0)
                .unwrap_or(buffer.len());
            return Some(String::from_utf16_lossy(&buffer[..length]));
        }
        if size <= bytes {
            return None;
        }
        bytes = size;
    }
    None
}
