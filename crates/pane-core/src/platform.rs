//! Operating systems a package or an action declares it supports, and the
//! explanation Pane gives where one is unavailable. Deliberately simple: a
//! list of systems, not a rule language. The systems themselves, and the
//! processors of native helpers, are named in `pane-target`.

pub use pane_target::Platform;

/// People's names for `platforms`: "Windows", "Windows and Linux",
/// "Windows, macOS and Linux".
pub(crate) fn names(platforms: &[Platform]) -> String {
    let names: Vec<String> = platforms.iter().map(Platform::to_string).collect();
    join(&names)
}

/// Joins `names` as a list in a sentence: "a", "a and b", "a, b and c".
pub(crate) fn join(names: &[String]) -> String {
    match names.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}

/// Why something that `subject` names ("this action", "this package")
/// cannot be used on this system, given the platforms it supports; `None`
/// when it can, or when it declares none (it then supports every system).
pub(crate) fn unavailable(supported: Option<&[Platform]>, subject: &str) -> Option<String> {
    let supported = supported?;
    let current = Platform::current();
    if current.is_some_and(|current| supported.contains(&current)) {
        return None;
    }
    let here = match current {
        Some(current) => current.to_string(),
        None => format!("this system ({})", std::env::consts::OS),
    };
    Some(if supported.is_empty() {
        format!("Not available on {here}: {subject} supports no operating system")
    } else {
        format!(
            "Not available on {here}: {subject} supports only {}",
            names(supported)
        )
    })
}
