//! The previous world, while its window lasts (ADR 0046): the 0.1
//! extension API under `wit/previous/`, bound beside the current one and
//! linked through an adapter written here, in the host. Everything the
//! previous world needs lives in this module: the bindgen twins of
//! `runtime.rs`'s (the main world and each export world, from the same
//! WIT the 0.1 contract froze), the previous interface names a
//! component's exports are looked up by, the adapter — the previous
//! world's host traits, implemented over the same host code the current
//! world's use, converting the shapes that differ between the
//! versions — and the wrappers that keep an instance's calls shape-ident
//! to the current world's, so no call site branches on the version.
//! When
//! the window ends, with the first Pane release after the one that
//! replaced 0.1, this module and `wit/previous/` are deleted together:
//! nothing outside them knows the previous world's names.
//!
//! A component built for 0.1 is instantiated against this world
//! wherever the current world's would be: every interface it imports is
//! served through the adapter, and every export the current world still
//! calls — a command's screen (`render` and `handle-event`, its run, its
//! root, indexed and published results, its service, events and
//! lifecycle) — is called through it, indistinguishably. The screens the
//! current API has since replaced are not: a 0.1 command's typed forms
//! and custom views — the current world renders designed views instead
//! — are the same the current API refuses of any component (see
//! `tree`), and its command-search export is no longer driven by the
//! launcher (root search answers through `root-results` and
//! `indexed-results`). The imports the two worlds share an instance
//! name for — WASI 0.3 and
//! `wasi:http` — are registered once, as `Code::new` does; the previous
//! interfaces' instance names (`@0.1.0`) do not collide with the current
//! ones (`@0.2.0`) in the one linker, so a component of either world
//! instantiates against it.
//!
//! Which world a component was built for is read from its own exports,
//! which name the versioned command interface — the same mechanism
//! `Code::check_exports` looks a component's command interface up by —
//! never from the manifest's
//! `apiVersion`: the manifest's version decides whether Pane runs the
//! package at all (see `packages::EXTENSION_API_PREVIOUS`), and the
//! component's exports decide which world runs it, so a build that
//! names one version and exports another is refused by the shape check
//! against the world its exports name, the safety net (ADR 0046).

pub(crate) mod bindings {
    wasmtime::component::bindgen!({
        path: "../../wit/previous",
        world: "extension-with-typed-folder",
        with: {
            // The host type behind each owned registration resource: the
            // registration's id in the registry, as the current world's
            // binding maps them — the resources are the host's, shared by
            // both worlds.
            "pane:extension/registrations.root-item": crate::registrations::RootItemHandle,
            "pane:extension/registrations.timer": crate::registrations::TimerHandle,
            "pane:extension/registrations.watcher": crate::registrations::WatcherHandle,
            "pane:extension/registrations.provision": crate::registrations::ProvisionHandle,
        },
        // The same imports configuration as the current world's: the host
        // code behind each interface is the same, so its waits run the
        // same way — off the runtime thread, unmetered — for a component
        // of either world.
        imports: {
            "pane:extension/operations": store,
            "pane:extension/helpers": store,
            "pane:extension/programs": store,
            "pane:extension/settings.set": async,
            "pane:extension/content.set": async,
            "pane:extension/cache.set": async,
            "pane:extension/credentials.set": async,
            "pane:extension/system": async,
            "pane:extension/typed-folder": async,
            "pane:extension/run": async,
            "pane:extension/system-commands": async,
            "pane:extension/windows": async,
            "pane:extension/registrations": trappable,
        },
        exports: { default: async | store },
    });
}

/// The previous world's `root-results` export.
pub(crate) mod root_bindings {
    wasmtime::component::bindgen!({
        path: "../../wit/previous",
        world: "root-results-provider",
        exports: { default: async | store },
    });
}

/// The previous world's `indexed-results` export.
pub(crate) mod indexed_bindings {
    wasmtime::component::bindgen!({
        path: "../../wit/previous",
        world: "indexed-results-provider",
        exports: { default: async | store },
    });
}

/// The previous world's `service` export.
pub(crate) mod service_bindings {
    wasmtime::component::bindgen!({
        path: "../../wit/previous",
        world: "service-provider",
        exports: { default: async | store },
    });
}

/// The previous world's `published-operations` export.
pub(crate) mod operations_bindings {
    wasmtime::component::bindgen!({
        path: "../../wit/previous",
        world: "operations-provider",
        exports: { default: async | store },
    });
}

/// The previous world's `events` export.
pub(crate) mod events_bindings {
    wasmtime::component::bindgen!({
        path: "../../wit/previous",
        world: "events-provider",
        exports: { default: async | store },
    });
}

/// The previous world's `lifecycle` export.
pub(crate) mod lifecycle_bindings {
    wasmtime::component::bindgen!({
        path: "../../wit/previous",
        world: "lifecycle-provider",
        exports: { default: async | store },
    });
}

/// The interface an extension command built for the previous version
/// exports.
pub(crate) const COMMAND_INTERFACE: &str = "pane:extension/command@0.1.0";

/// The interface a command that computes root results built for the
/// previous version also exports.
pub(crate) const ROOT_RESULTS_INTERFACE: &str = "pane:extension/root-results@0.1.0";

/// The interface a command that supplies root results ahead of the query
/// built for the previous version also exports.
pub(crate) const INDEXED_RESULTS_INTERFACE: &str = "pane:extension/indexed-results@0.1.0";

/// The interface a component serving published operations built for the
/// previous version also exports.
pub(crate) const OPERATIONS_INTERFACE: &str = "pane:extension/published-operations@0.1.0";

/// The interface a command that runs a continuing service built for the
/// previous version also exports.
pub(crate) const SERVICE_INTERFACE: &str = "pane:extension/service@0.1.0";

/// The interface a component that opts in to the state handoff built for
/// the previous version exports.
pub(crate) const LIFECYCLE_INTERFACE: &str = "pane:extension/lifecycle@0.1.0";

/// Which world a component was built for: the current extension API's, or
/// the previous one this Pane still runs (ADR 0046).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Which {
    /// The current API's, as `wit/` declares.
    Current,
    /// The previous version's, as `wit/previous/` declares, served
    /// through the adapter in this module.
    Previous,
}

impl Which {
    /// The world `component`'s own exports name: the previous one when
    /// they name the previous version's command interface, the current
    /// one otherwise — including a component that names neither, which
    /// only the current world's checks refuse, in Pane's words. A
    /// component that names both is one of the current world's: whatever
    /// else it exports, the current contract is the one it carries.
    pub(crate) fn of(component: &Component, engine: &Engine) -> Which {
        let ty = component.component_type();
        let names = |interface: &str| ty.get_export(engine, interface).is_some();
        match (names(super::COMMAND_INTERFACE), names(COMMAND_INTERFACE)) {
            (true, _) => Which::Current,
            (false, true) => Which::Previous,
            (false, false) => Which::Current,
        }
    }
}

use wasmtime::Engine;
use wasmtime::component::{Access, Accessor, Component, Linker, Resource, ResourceAny};

use super::bindings::exports::pane::extension::command as current_command;
use super::events_bindings::exports::pane::extension::events as current_events;
use super::indexed_bindings::exports::pane::extension::indexed_results as current_indexed;
use super::lifecycle_bindings::exports::pane::extension::lifecycle as current_lifecycle;
use super::operations_bindings::exports::pane::extension::published_operations as current_operations;
use super::root_bindings::exports::pane::extension::root_results as current_root;
use super::service_bindings::exports::pane::extension::service as current_service;
use super::{CallError, Checked, Exports, GuestState, lock, stopped_code};
use bindings::exports::pane::extension::command;
use bindings::pane::extension::commands as launching;
use bindings::pane::extension::feedback as feedback_host;
use bindings::pane::extension::file_index;
use bindings::pane::extension::files;
use bindings::pane::extension::helpers;
use bindings::pane::extension::operations;
use bindings::pane::extension::preferences as preference_values;
use bindings::pane::extension::programs;
use bindings::pane::extension::registrations as registering;
use bindings::pane::extension::run as run_host;
use bindings::pane::extension::system as system_host;
use bindings::pane::extension::system_commands as system_commands_host;
use bindings::pane::extension::typed_folder as typed_folder_host;
use bindings::pane::extension::window as window_host;
use bindings::pane::extension::windows as windows_host;
use bindings::pane::extension::{
    applications, cache, clipboard_history, content, credentials, settings,
};

// The kinds of data the data interfaces serve, for `data_host!` below.
use super::data_host;
use crate::extension_data::DataKind;
use events_bindings::exports::pane::extension::events;
use indexed_bindings::exports::pane::extension::indexed_results;
use lifecycle_bindings::exports::pane::extension::lifecycle;
use operations_bindings::exports::pane::extension::published_operations;
use root_bindings::exports::pane::extension::root_results;
use service_bindings::exports::pane::extension::service;

// =========================================================== the exports

/// A running instance's bindings: the current world's, or the previous
/// one's, whichever the component was built for. The accessor and call
/// names below are the current world's, so no call site branches on the
/// version; the previous world's are converted at this seam — the one
/// place the two worlds meet, besides the host traits below.
pub(crate) enum World {
    /// A component built for the current API.
    Current(super::bindings::ExtensionWithAlerts),
    /// A component built for the previous version, run through the
    /// adapter.
    Previous(bindings::ExtensionWithTypedFolder),
}

impl World {
    /// The command interface the component exports, as
    /// `pane_extension_command` is on the current world's bindings: the
    /// handle lent, which the calls keep as long as the instance's.
    pub(crate) fn pane_extension_command(&self) -> WorldCommand<'_> {
        match self {
            World::Current(bindings) => WorldCommand::Current(bindings.pane_extension_command()),
            World::Previous(bindings) => WorldCommand::Previous(bindings.pane_extension_command()),
        }
    }
}

/// The command interface of an instance: the current world's handle, or
/// the previous one's, lent by the instance's bindings.
pub(crate) enum WorldCommand<'a> {
    Current(&'a current_command::Guest),
    Previous(&'a command::Guest),
}

impl WorldCommand<'_> {
    pub(crate) async fn call_render(
        &self,
        store: &Accessor<GuestState>,
        launch: super::launching::LaunchRecord,
    ) -> wasmtime::Result<Result<String, String>> {
        match self {
            WorldCommand::Current(command) => command.call_render(store, launch).await,
            WorldCommand::Previous(command) => command.call_render(store, launch.into()).await,
        }
    }

    pub(crate) async fn call_run(
        &self,
        store: &Accessor<GuestState>,
        command: String,
        launch: super::launching::LaunchRecord,
    ) -> wasmtime::Result<Result<String, String>> {
        match self {
            WorldCommand::Current(exported) => exported.call_run(store, command, launch).await,
            WorldCommand::Previous(exported) => {
                exported.call_run(store, command, launch.into()).await
            }
        }
    }

    pub(crate) async fn call_handle_event(
        &self,
        store: &Accessor<GuestState>,
        callback: String,
        details: String,
    ) -> wasmtime::Result<Result<String, String>> {
        match self {
            WorldCommand::Current(command) => {
                command.call_handle_event(store, callback, details).await
            }
            WorldCommand::Previous(command) => {
                command.call_handle_event(store, callback, details).await
            }
        }
    }

    pub(crate) async fn call_open_view(
        &self,
        store: &Accessor<GuestState>,
        command: String,
        launch: super::launching::LaunchRecord,
    ) -> wasmtime::Result<Result<ResourceAny, String>> {
        match self {
            WorldCommand::Current(exported) => {
                exported.call_open_view(store, command, launch).await
            }
            // The previous world's `open-view` opens an item's custom
            // view, a screen the current API replaced with the designed
            // view (#242): no command built for the previous version
            // opens one, so a component whose manifest names a designed
            // command while its exports name 0.1 is refused here — the
            // shape check's safety net (ADR 0046), in Pane's words.
            WorldCommand::Previous(_) => Ok(Err(
                "this component was built for extension API 0.1, which opens no designed \
                 view; rebuild it for extension API 0.2"
                    .into(),
            )),
        }
    }
}

/// An instance's `root-results` export: the current world's provider, or
/// the previous one's, with the same accessor.
pub(crate) enum WorldRootResults {
    Current(super::root_bindings::RootResultsProvider),
    Previous(root_bindings::RootResultsProvider),
}

impl WorldRootResults {
    pub(crate) fn pane_extension_root_results(&self) -> RootResultsHandle {
        match self {
            WorldRootResults::Current(provider) => {
                RootResultsHandle::Current(provider.pane_extension_root_results().clone())
            }
            WorldRootResults::Previous(provider) => {
                RootResultsHandle::Previous(provider.pane_extension_root_results().clone())
            }
        }
    }
}

/// The `root-results` interface an instance exports, of either world.
#[derive(Clone)]
pub(crate) enum RootResultsHandle {
    Current(current_root::Guest),
    Previous(root_results::Guest),
}

impl RootResultsHandle {
    pub(crate) async fn call_results_for(
        &self,
        store: &Accessor<GuestState>,
        query: String,
        at: current_root::WallTime,
    ) -> wasmtime::Result<Result<Vec<current_root::RootResult>, String>> {
        match self {
            RootResultsHandle::Current(provider) => {
                provider.call_results_for(store, query, at).await
            }
            RootResultsHandle::Previous(provider) => provider
                .call_results_for(store, query, root_results::WallTime::from(at))
                .await
                .map(|answer| {
                    answer.map(|results| {
                        results
                            .into_iter()
                            .map(current_root::RootResult::from)
                            .collect()
                    })
                }),
        }
    }
}

/// An instance's `indexed-results` export, of either world.
pub(crate) enum WorldIndexedResults {
    Current(super::indexed_bindings::IndexedResultsProvider),
    Previous(indexed_bindings::IndexedResultsProvider),
}

impl WorldIndexedResults {
    pub(crate) fn pane_extension_indexed_results(&self) -> IndexedResultsHandle {
        match self {
            WorldIndexedResults::Current(provider) => {
                IndexedResultsHandle::Current(provider.pane_extension_indexed_results().clone())
            }
            WorldIndexedResults::Previous(provider) => {
                IndexedResultsHandle::Previous(provider.pane_extension_indexed_results().clone())
            }
        }
    }
}

/// The `indexed-results` interface an instance exports, of either world.
#[derive(Clone)]
pub(crate) enum IndexedResultsHandle {
    Current(current_indexed::Guest),
    Previous(indexed_results::Guest),
}

impl IndexedResultsHandle {
    pub(crate) async fn call_results(
        &self,
        store: &Accessor<GuestState>,
    ) -> wasmtime::Result<Result<Vec<current_indexed::IndexedResult>, String>> {
        match self {
            IndexedResultsHandle::Current(provider) => provider.call_results(store).await,
            IndexedResultsHandle::Previous(provider) => {
                provider.call_results(store).await.map(|answer| {
                    answer.map(|results| {
                        results
                            .into_iter()
                            .map(current_indexed::IndexedResult::from)
                            .collect()
                    })
                })
            }
        }
    }
}

/// An instance's `published-operations` export, of either world.
pub(crate) enum WorldOperations {
    Current(super::operations_bindings::OperationsProvider),
    Previous(operations_bindings::OperationsProvider),
}

impl WorldOperations {
    pub(crate) fn pane_extension_published_operations(&self) -> OperationsHandle {
        match self {
            WorldOperations::Current(provider) => {
                OperationsHandle::Current(provider.pane_extension_published_operations().clone())
            }
            WorldOperations::Previous(provider) => {
                OperationsHandle::Previous(provider.pane_extension_published_operations().clone())
            }
        }
    }
}

/// The `published-operations` interface an instance exports, of either
/// world. Its one function takes and answers only strings, so the two
/// worlds' are the same call.
#[derive(Clone)]
pub(crate) enum OperationsHandle {
    Current(current_operations::Guest),
    Previous(published_operations::Guest),
}

impl OperationsHandle {
    pub(crate) async fn call_run_operation(
        &self,
        store: &Accessor<GuestState>,
        operation: String,
        input: String,
    ) -> wasmtime::Result<Result<String, String>> {
        match self {
            OperationsHandle::Current(provider) => {
                provider.call_run_operation(store, operation, input).await
            }
            OperationsHandle::Previous(provider) => {
                provider.call_run_operation(store, operation, input).await
            }
        }
    }
}

/// An instance's `service` export, of either world.
pub(crate) enum WorldService {
    Current(super::service_bindings::ServiceProvider),
    Previous(service_bindings::ServiceProvider),
}

impl WorldService {
    pub(crate) fn pane_extension_service(&self) -> ServiceHandle {
        match self {
            WorldService::Current(provider) => {
                ServiceHandle::Current(provider.pane_extension_service().clone())
            }
            WorldService::Previous(provider) => {
                ServiceHandle::Previous(provider.pane_extension_service().clone())
            }
        }
    }
}

/// The `service` interface an instance exports, of either world.
#[derive(Clone)]
pub(crate) enum ServiceHandle {
    Current(current_service::Guest),
    Previous(service::Guest),
}

impl ServiceHandle {
    pub(crate) async fn call_run_cycle(
        &self,
        store: &Accessor<GuestState>,
        command: String,
    ) -> wasmtime::Result<Result<current_service::Cycle, String>> {
        match self {
            ServiceHandle::Current(provider) => provider.call_run_cycle(store, command).await,
            ServiceHandle::Previous(provider) => provider
                .call_run_cycle(store, command)
                .await
                .map(|answer| answer.map(current_service::Cycle::from)),
        }
    }
}

/// An instance's `events` export, of either world.
pub(crate) enum WorldEvents {
    Current(super::events_bindings::EventsProvider),
    Previous(events_bindings::EventsProvider),
}

impl WorldEvents {
    pub(crate) fn pane_extension_events(&self) -> EventsHandle {
        match self {
            WorldEvents::Current(provider) => {
                EventsHandle::Current(provider.pane_extension_events().clone())
            }
            WorldEvents::Previous(provider) => {
                EventsHandle::Previous(provider.pane_extension_events().clone())
            }
        }
    }
}

/// The `events` interface an instance exports, of either world.
#[derive(Clone)]
pub(crate) enum EventsHandle {
    Current(current_events::Guest),
    Previous(events::Guest),
}

impl EventsHandle {
    pub(crate) async fn call_handle_event(
        &self,
        store: &Accessor<GuestState>,
        event: current_events::Event,
    ) -> wasmtime::Result<Result<(), String>> {
        match self {
            EventsHandle::Current(provider) => provider.call_handle_event(store, event).await,
            EventsHandle::Previous(provider) => {
                provider
                    .call_handle_event(store, events::Event::from(event))
                    .await
            }
        }
    }
}

/// An instance's `lifecycle` export, of either world.
pub(crate) enum WorldLifecycle {
    Current(super::lifecycle_bindings::LifecycleProvider),
    Previous(lifecycle_bindings::LifecycleProvider),
}

impl WorldLifecycle {
    pub(crate) fn pane_extension_lifecycle(&self) -> LifecycleHandle {
        match self {
            WorldLifecycle::Current(provider) => {
                LifecycleHandle::Current(provider.pane_extension_lifecycle().clone())
            }
            WorldLifecycle::Previous(provider) => {
                LifecycleHandle::Previous(provider.pane_extension_lifecycle().clone())
            }
        }
    }
}

/// The `lifecycle` interface an instance exports, of either world. Its
/// functions take and answer only bytes and strings, so the two worlds'
/// are the same calls.
#[derive(Clone)]
pub(crate) enum LifecycleHandle {
    Current(current_lifecycle::Guest),
    Previous(lifecycle::Guest),
}

impl LifecycleHandle {
    pub(crate) async fn call_activate(&self, store: &Accessor<GuestState>) -> wasmtime::Result<()> {
        match self {
            LifecycleHandle::Current(provider) => provider.call_activate(store).await,
            LifecycleHandle::Previous(provider) => provider.call_activate(store).await,
        }
    }

    pub(crate) async fn call_snapshot(
        &self,
        store: &Accessor<GuestState>,
    ) -> wasmtime::Result<Option<Vec<u8>>> {
        match self {
            LifecycleHandle::Current(provider) => provider.call_snapshot(store).await,
            LifecycleHandle::Previous(provider) => provider.call_snapshot(store).await,
        }
    }

    pub(crate) async fn call_restore(
        &self,
        store: &Accessor<GuestState>,
        state: Vec<u8>,
    ) -> wasmtime::Result<Result<(), String>> {
        match self {
            LifecycleHandle::Current(provider) => provider.call_restore(store, state).await,
            LifecycleHandle::Previous(provider) => provider.call_restore(store, state).await,
        }
    }
}

// The conversions between the two worlds' shapes, at the seam above and
// below: structurally identical (the breaking change of 0.2 retyped only
// `show-hud`'s style, on the host-import side), so each is a field-by-
// field copy. They live nowhere but here: nothing else may know both
// worlds' types.

impl From<super::launching::LaunchRecord> for launching::LaunchRecord {
    fn from(launch: super::launching::LaunchRecord) -> launching::LaunchRecord {
        launching::LaunchRecord {
            launch_type: match launch.launch_type {
                super::launching::LaunchType::UserInitiated => launching::LaunchType::UserInitiated,
                super::launching::LaunchType::Background => launching::LaunchType::Background,
            },
            source: match launch.source {
                super::launching::LaunchSource::RootSearch => launching::LaunchSource::RootSearch,
                super::launching::LaunchSource::Alias => launching::LaunchSource::Alias,
                super::launching::LaunchSource::Fallback => launching::LaunchSource::Fallback,
                super::launching::LaunchSource::Hotkey => launching::LaunchSource::Hotkey,
                super::launching::LaunchSource::QuickSlot => launching::LaunchSource::QuickSlot,
                super::launching::LaunchSource::Command => launching::LaunchSource::Command,
                super::launching::LaunchSource::Schedule => launching::LaunchSource::Schedule,
            },
            arguments: launch
                .arguments
                .into_iter()
                .map(|argument| launching::ArgumentValue {
                    name: argument.name,
                    value: argument.value,
                })
                .collect(),
            fallback_text: launch.fallback_text,
            context: launch.context,
            command: launch.command,
        }
    }
}

impl From<current_root::WallTime> for root_results::WallTime {
    fn from(at: current_root::WallTime) -> root_results::WallTime {
        root_results::WallTime {
            milliseconds: at.milliseconds,
            offset: at.offset,
        }
    }
}

impl From<root_results::RootResult> for current_root::RootResult {
    fn from(result: root_results::RootResult) -> current_root::RootResult {
        current_root::RootResult {
            id: result.id,
            title: result.title,
            subtitle: result.subtitle,
            action: match result.action {
                root_results::RootAction::Copy(text) => current_root::RootAction::Copy(text),
                root_results::RootAction::OpenUrl(url) => current_root::RootAction::OpenUrl(url),
                root_results::RootAction::OpenFile(path) => {
                    current_root::RootAction::OpenFile(path)
                }
            },
            answer: result.answer.map(|answer| current_root::AnswerDetail {
                section: answer.section,
                swatch: answer.swatch,
                copies: answer
                    .copies
                    .into_iter()
                    .map(|copy| current_root::AnswerCopy {
                        title: copy.title,
                        text: copy.text,
                    })
                    .collect(),
            }),
        }
    }
}

impl From<indexed_results::IndexedResult> for current_indexed::IndexedResult {
    fn from(result: indexed_results::IndexedResult) -> current_indexed::IndexedResult {
        current_indexed::IndexedResult {
            id: result.id,
            title: result.title,
            subtitle: result.subtitle,
            alternate_titles: result.alternate_titles,
            keywords: result.keywords,
            action: match result.action {
                indexed_results::IndexedAction::OpenApplication(id) => {
                    current_indexed::IndexedAction::OpenApplication(id)
                }
                indexed_results::IndexedAction::Open(open) => {
                    current_indexed::IndexedAction::Open(current_indexed::OpenTarget {
                        target: open.target,
                        application: open.application,
                    })
                }
            },
        }
    }
}

impl From<service::Cycle> for current_service::Cycle {
    fn from(cycle: service::Cycle) -> current_service::Cycle {
        current_service::Cycle {
            status: cycle.status,
            next_seconds: cycle.next_seconds,
        }
    }
}

impl From<current_events::Event> for events::Event {
    fn from(event: current_events::Event) -> events::Event {
        match event {
            current_events::Event::Timer(tag) => events::Event::Timer(tag),
            current_events::Event::Watcher(watcher) => {
                events::Event::Watcher(events::WatcherEvent {
                    tag: watcher.tag,
                    changes: match watcher.changes {
                        current_events::WatcherChanges::Paths(paths) => {
                            events::WatcherChanges::Paths(paths)
                        }
                        current_events::WatcherChanges::Rescan => events::WatcherChanges::Rescan,
                    },
                })
            }
        }
    }
}

// ============================================================== the checks

/// The previous world's twin of `Code::check_exports`: type-checks the
/// functions of the component's command interface against those Pane
/// calls, in the previous world's types, from the component's type
/// alone, so no guest code runs. A component without the interface is
/// left to [`check`], which says so; the previous bindgen's types are
/// the ones the check must name, so a rebuild for the previous version
/// is what the refusal asks for (ADR 0046).
pub(crate) fn check_exports(component: &Component, engine: &Engine) -> Result<(), CallError> {
    use wasmtime::component::types::{ComponentFunc, ComponentItem};
    use wasmtime::component::{ComponentNamedList, Lift, Lower, ResourceAny};

    let ty = component.component_type();
    let Some(ComponentItem::ComponentInstance(interface)) = ty
        .get_export(engine, COMMAND_INTERFACE)
        .map(|export| export.ty)
    else {
        return Ok(());
    };
    let cx = ty.instance_type();
    let api = crate::packages::EXTENSION_API_PREVIOUS.unwrap_or(crate::packages::EXTENSION_API);
    let older = |problem: String| CallError::OlderApiShape {
        api,
        reason: problem,
    };
    let func = |name: &str| match interface.get_export(engine, name).map(|e| e.ty) {
        Some(ComponentItem::ComponentFunc(func)) => Ok(func),
        _ => Err(older(format!("it has no function `{name}`"))),
    };
    fn check<P: ComponentNamedList + Lower, R: ComponentNamedList + Lift>(
        name: &str,
        func: ComponentFunc,
        cx: &wasmtime::component::__internal::InstanceType<'_>,
        api: (u64, u64),
    ) -> Result<(), CallError> {
        func.typecheck::<P, R>(cx)
            .map_err(|error| CallError::OlderApiShape {
                api,
                reason: format!("`{name}`: {error:#}"),
            })
    }
    check::<(launching::LaunchRecord,), (Result<String, String>,)>(
        "render",
        func("render")?,
        &cx,
        api,
    )?;
    check::<(String, launching::LaunchRecord), (Result<String, String>,)>(
        "run",
        func("run")?,
        &cx,
        api,
    )?;
    check::<(String, String), (Result<String, String>,)>(
        "handle-event",
        func("handle-event")?,
        &cx,
        api,
    )?;
    check::<(String, Vec<command::FieldValue>), (Result<String, command::FormError>,)>(
        "submit-form",
        func("submit-form")?,
        &cx,
        api,
    )?;
    check::<(String,), (Result<ResourceAny, String>,)>("open-view", func("open-view")?, &cx, api)?;
    let render = "[method]custom-view.render";
    check::<(ResourceAny,), (command::Frame,)>(render, func(render)?, &cx, api)?;
    let handle_event = "[method]custom-view.handle-event";
    check::<(ResourceAny, command::ViewEvent), (Result<(), String>,)>(
        handle_event,
        func(handle_event)?,
        &cx,
        api,
    )
}

/// The previous world's twin of `Code::check_root_results`: type-checks the
/// component's root results' `results-for` against the call Pane makes,
/// when it exports them, so a component of another shape of the version it
/// names — the default extensions released before 0.1 was frozen answered
/// the query alone — is refused when it is installed, not when root search
/// asks it (ADR 0046).
pub(crate) fn check_root_results(component: &Component, engine: &Engine) -> Result<(), CallError> {
    use wasmtime::component::types::ComponentItem;

    let ty = component.component_type();
    let Some(ComponentItem::ComponentInstance(interface)) = ty
        .get_export(engine, ROOT_RESULTS_INTERFACE)
        .map(|export| export.ty)
    else {
        return Ok(());
    };
    let api = crate::packages::EXTENSION_API_PREVIOUS.unwrap_or(crate::packages::EXTENSION_API);
    let older = |reason: String| CallError::OlderApiShape { api, reason };
    let Some(ComponentItem::ComponentFunc(func)) = interface
        .get_export(engine, "results-for")
        .map(|export| export.ty)
    else {
        return Err(older(
            "its root results have no function `results-for`".into(),
        ));
    };
    func.typecheck::<
        (String, root_results::WallTime),
        (Result<Vec<root_results::RootResult>, String>,),
    >(&ty.instance_type())
        .map_err(|error| older(format!("`results-for`: {error:#}")))
}

/// The previous world's twin of `Code::check`: type-checks `component`
/// against the linker and the previous extension world, and against each
/// interface `exports` names too, without instantiating it, so no guest
/// code runs. What checking found out about it is the same [`Checked`]
/// either world answers with.
pub(crate) fn check(
    linker: &Linker<GuestState>,
    engine: &Engine,
    component: &Component,
    exports: Exports,
) -> Result<Checked, CallError> {
    let ty = component.component_type();
    let network = ty
        .imports(engine)
        .any(|(name, _)| name.starts_with("wasi:http/"));
    let programs = ty
        .imports(engine)
        .any(|(name, _)| name.starts_with(crate::programs::PROGRAMS_INTERFACE));
    let interface = |error: wasmtime::Error| CallError::Interface(format!("{error:#}"));
    let pre = linker.instantiate_pre(component).map_err(interface)?;
    if exports.root_results {
        root_bindings::RootResultsProviderPre::new(pre.clone()).map_err(|error| {
            CallError::Interface(format!(
                "its manifest says it computes root results, but it does not export \
                 {ROOT_RESULTS_INTERFACE} with the functions Pane calls: {error:#}"
            ))
        })?;
    }
    if exports.indexed_results {
        indexed_bindings::IndexedResultsProviderPre::new(pre.clone()).map_err(|error| {
            CallError::Interface(format!(
                "its manifest says it supplies indexed results, but it does not export \
                 {INDEXED_RESULTS_INTERFACE} with the functions Pane calls: {error:#}"
            ))
        })?;
    }
    if exports.operations {
        operations_bindings::OperationsProviderPre::new(pre.clone()).map_err(|error| {
            CallError::Interface(format!(
                "its manifest publishes operations it serves, but it does not export \
                 {OPERATIONS_INTERFACE} with the functions Pane calls: {error:#}"
            ))
        })?;
    }
    if exports.service {
        service_bindings::ServiceProviderPre::new(pre.clone()).map_err(|error| {
            CallError::Interface(format!(
                "its manifest says it runs a continuing service, but it does not export \
                 {SERVICE_INTERFACE} with the functions Pane calls: {error:#}"
            ))
        })?;
    }
    if exports.activate {
        lifecycle_bindings::LifecycleProviderPre::new(pre.clone()).map_err(|error| {
            CallError::Interface(format!(
                "its pane.json names it under `activate`, but it does not export \
                 {LIFECYCLE_INTERFACE} with the functions Pane calls: {error:#}"
            ))
        })?;
    }
    // A component opts in to the state handoff (ADR 0041) by exporting
    // the lifecycle interface, whether or not its package's `pane.json`
    // names it under `activate`: installing detects the export here,
    // without running guest code, so a component exporting another
    // shape of it is refused.
    if ty.get_export(engine, LIFECYCLE_INTERFACE).is_some() {
        lifecycle_bindings::LifecycleProviderPre::new(pre.clone()).map_err(|error| {
            CallError::Interface(format!(
                "it does not export {LIFECYCLE_INTERFACE} with the functions Pane calls: \
                 {error:#}"
            ))
        })?;
    }
    bindings::ExtensionWithTypedFolderPre::new(pre).map_err(interface)?;

    Ok(Checked { network, programs })
}

// =============================================================== the linker

/// Registers the previous world's interfaces in the linker, beside the
/// current one's: every host function a component built for the previous
/// version imports, served by the adapter below. WASI 0.3 and
/// `wasi:http` are registered once for both worlds by `Code::new`, whose
/// names neither version carries.
pub(crate) fn add_to_linker(linker: &mut Linker<GuestState>) -> wasmtime::Result<()> {
    use wasmtime::component::HasSelf;
    // The same registrations, in the same order, as the current world's in
    // `Code::new`, over the same store state and the same host code: only
    // the instance names differ, carrying `@0.1.0`.
    bindings::pane::extension::settings::add_to_linker::<_, HasSelf<_>>(linker, |state| state)?;
    bindings::pane::extension::content::add_to_linker::<_, HasSelf<_>>(linker, |state| state)?;
    bindings::pane::extension::cache::add_to_linker::<_, HasSelf<_>>(linker, |state| state)?;
    bindings::pane::extension::credentials::add_to_linker::<_, HasSelf<_>>(linker, |state| state)?;
    bindings::pane::extension::operations::add_to_linker::<_, crate::operations::Calls>(
        linker,
        |state| state,
    )?;
    bindings::pane::extension::applications::add_to_linker::<_, HasSelf<_>>(linker, |state| state)?;
    bindings::pane::extension::clipboard_history::add_to_linker::<_, HasSelf<_>>(
        linker,
        |state| state,
    )?;
    bindings::pane::extension::helpers::add_to_linker::<_, crate::helpers::Runs>(
        linker,
        |state| state,
    )?;
    bindings::pane::extension::programs::add_to_linker::<_, crate::programs::Calls>(
        linker,
        |state| state,
    )?;
    bindings::pane::extension::files::add_to_linker::<_, HasSelf<_>>(linker, |state| state)?;
    bindings::pane::extension::file_index::add_to_linker::<_, HasSelf<_>>(linker, |state| state)?;
    bindings::pane::extension::typed_folder::add_to_linker::<_, HasSelf<_>>(linker, |state| state)?;
    bindings::pane::extension::commands::add_to_linker::<_, HasSelf<_>>(linker, |state| state)?;
    bindings::pane::extension::window::add_to_linker::<_, HasSelf<_>>(linker, |state| state)?;
    // Toasts and HUDs answer at once; `confirm` waits for the user with
    // the store's accessor, as helpers do.
    bindings::pane::extension::feedback::add_to_linker::<_, super::host_functions::Confirms>(
        linker,
        |state| state,
    )?;
    bindings::pane::extension::system::add_to_linker::<_, HasSelf<_>>(linker, |state| state)?;
    bindings::pane::extension::run::add_to_linker::<_, HasSelf<_>>(linker, |state| state)?;
    bindings::pane::extension::system_commands::add_to_linker::<_, HasSelf<_>>(linker, |state| {
        state
    })?;
    bindings::pane::extension::windows::add_to_linker::<_, HasSelf<_>>(linker, |state| state)?;
    bindings::pane::extension::preferences::add_to_linker::<_, HasSelf<_>>(linker, |state| state)?;
    // What a command registers at run time it owns as a resource: the
    // host side of the resources and of the functions that make them.
    bindings::pane::extension::registrations::add_to_linker::<_, HasSelf<_>>(linker, |state| {
        state
    })?;
    Ok(())
}

// =============================================================== the adapter
//
// The previous world's host traits, implemented over the same host code
// the current world's use: each body is the current impl's, with the
// previous bindgen's types at its edges, converted where they differ.
// Only `feedback.show-hud`'s style differs between the two worlds (the
// 0.2 breaking change), and its conversion is the one place the adapter
// translates a shape rather than re-wrapping it.

// The data interfaces, over the same data, with the same macro the
// current world's are: the reading is a short host call, the saving
// waits off the runtime thread.
data_host!(settings, DataKind::Settings);
data_host!(content, DataKind::Content);
data_host!(cache, DataKind::Cache);
data_host!(credentials, DataKind::LocalCredentials);

impl preference_values::Host for GuestState {
    fn values(&mut self, command: Option<String>) -> Result<String, String> {
        // The same read as the current world's: the manifest, the saved
        // values and the command are the host's, not the world's.
        super::preference_values_of(self, command)
    }
}

impl launching::Host for GuestState {
    fn launch(
        &mut self,
        target: launching::CommandRef,
        launch_type: launching::LaunchType,
        arguments: Vec<launching::ArgumentValue>,
        context: Option<String>,
    ) -> Result<(), String> {
        // Stopped code launches nothing: checked once the host call is
        // marked, as applications' are.
        let _host = self.host();
        if let Some(end) = self.stopped() {
            return Err(stopped_code(end));
        }
        let launches = lock(&self.launches)
            .clone()
            .ok_or("this Pane does not launch commands for extensions")?;
        launches(crate::launch::LaunchRequest {
            caller: self.component.clone(),
            source: target.source,
            command: target.command,
            launch_type: match launch_type {
                launching::LaunchType::UserInitiated => crate::launch::LaunchType::UserInitiated,
                launching::LaunchType::Background => crate::launch::LaunchType::Background,
            },
            arguments: arguments
                .into_iter()
                .map(|argument| (argument.name, argument.value))
                .collect(),
            context,
            windowed: self.call.windowed,
        })
    }

    fn set_subtitle(&mut self, subtitle: Option<String>) -> Result<(), String> {
        self.set_command_subtitle(subtitle)
    }
}

impl registering::Host for GuestState {
    fn add_root_item(
        &mut self,
        command: String,
        item: String,
    ) -> wasmtime::Result<Result<Resource<crate::registrations::RootItemHandle>, String>> {
        // The same registrations the current world's impl makes: the
        // resources and the checks are the host's, identical for either
        // world.
        self.add_root_item(command, item)
    }

    fn timer_after(
        &mut self,
        seconds: u64,
        tag: String,
    ) -> wasmtime::Result<Result<Resource<crate::registrations::TimerHandle>, String>> {
        Ok(self.timer(false, seconds, tag))
    }

    fn timer_every(
        &mut self,
        seconds: u64,
        tag: String,
    ) -> wasmtime::Result<Result<Resource<crate::registrations::TimerHandle>, String>> {
        Ok(self.timer(true, seconds, tag))
    }

    fn watch_folder(
        &mut self,
        path: String,
        recursive: bool,
        tag: String,
    ) -> wasmtime::Result<Result<Resource<crate::registrations::WatcherHandle>, String>> {
        self.watch_folder(path, recursive, tag)
    }

    fn provide_capability(
        &mut self,
        capability: String,
    ) -> wasmtime::Result<Result<Resource<crate::registrations::ProvisionHandle>, String>> {
        self.provide_capability(capability)
    }
}

impl registering::HostRootItem for GuestState {
    fn update(
        &mut self,
        own: Resource<crate::registrations::RootItemHandle>,
        item: String,
    ) -> wasmtime::Result<Result<(), String>> {
        self.update_root_item(own, item)
    }

    fn drop(
        &mut self,
        own: Resource<crate::registrations::RootItemHandle>,
    ) -> wasmtime::Result<()> {
        if let Ok(handle) = self.table.delete(own) {
            self.registrations.release(handle.0);
        }
        Ok(())
    }
}

impl registering::HostTimer for GuestState {
    fn drop(&mut self, own: Resource<crate::registrations::TimerHandle>) -> wasmtime::Result<()> {
        if let Ok(handle) = self.table.delete(own) {
            self.registrations.release(handle.0);
        }
        Ok(())
    }
}

impl registering::HostWatcher for GuestState {
    fn drop(&mut self, own: Resource<crate::registrations::WatcherHandle>) -> wasmtime::Result<()> {
        if let Ok(handle) = self.table.delete(own) {
            self.registrations.release(handle.0);
        }
        Ok(())
    }
}

impl registering::HostProvision for GuestState {
    fn drop(
        &mut self,
        own: Resource<crate::registrations::ProvisionHandle>,
    ) -> wasmtime::Result<()> {
        if let Ok(handle) = self.table.delete(own) {
            self.registrations.release(handle.0);
        }
        Ok(())
    }
}

impl applications::Host for GuestState {
    fn installed(&mut self) -> Result<Vec<applications::Application>, String> {
        // Stopped code starts no more work: checked once the host call is
        // marked, from when Pane no longer decides to give up on the
        // thread.
        let _host = self.host();
        if let Some(end) = self.stopped() {
            return Err(stopped_code(end));
        }
        // The host keeps its list while a package that asked can run.
        self.applications.asked_by(
            &self.component,
            self.data
                .as_ref()
                .map(crate::extension_data::PackageData::generation),
        );
        Ok(self
            .applications()
            .installed()?
            .into_iter()
            .map(|application| applications::Application {
                icon: crate::applications::icon_reference(&application.id),
                id: application.id,
                name: application.name,
                location: application.location,
                alternate_titles: application.alternate_titles,
                keywords: application.keywords,
                distinction: application.distinction,
            })
            .collect())
    }

    fn open(&mut self, id: String) -> Result<(), String> {
        let _host = self.host();
        if let Some(end) = self.stopped() {
            return Err(stopped_code(end));
        }
        self.applications().open(&id)
    }
}

impl clipboard_history::Host for GuestState {
    fn status(&mut self) -> Result<clipboard_history::HistoryStatus, String> {
        let status = self.clipboard(|history| history.status())?;
        Ok(clipboard_history::HistoryStatus {
            capture: match status.capture {
                crate::clipboard::CaptureState::Off => clipboard_history::Capture::Off,
                crate::clipboard::CaptureState::On => clipboard_history::Capture::On,
                crate::clipboard::CaptureState::Paused => clipboard_history::Capture::Paused,
            },
            problem: status.problem,
            excluded: status.excluded.into_iter().map(String::from).collect(),
            items: count(status.items),
            retention_seconds: status.retention_seconds,
        })
    }

    fn set_capture(&mut self, wanted: clipboard_history::Capture) -> Result<(), String> {
        let wanted = match wanted {
            clipboard_history::Capture::Off => crate::clipboard::CaptureState::Off,
            clipboard_history::Capture::On => crate::clipboard::CaptureState::On,
            clipboard_history::Capture::Paused => crate::clipboard::CaptureState::Paused,
        };
        self.clipboard(|history| history.set_capture(wanted))
    }

    fn set_excluded(&mut self, programs: Vec<String>) -> Result<(), String> {
        self.clipboard(|history| history.set_excluded(&programs))
    }

    fn set_retention(&mut self, seconds: u64) -> Result<(), String> {
        self.clipboard(|history| history.set_retention(seconds))
    }

    fn entries(&mut self) -> Result<Vec<clipboard_history::Entry>, String> {
        let (items, now) = self.clipboard(|history| history.items())?;
        Ok(items
            .into_iter()
            .map(|item| clipboard_history::Entry {
                id: item.id.to_string(),
                text: item.text,
                copied_at: item.copied_at,
                age_seconds: now.saturating_sub(item.copied_at) / 1000,
                // The program's file name, as the contract says, also where
                // Pane keeps its path (Windows).
                source: item
                    .source
                    .as_deref()
                    .map(|source| crate::clipboard::program_file_name(source).to_owned()),
            })
            .collect())
    }

    fn copy(&mut self, id: String) -> Result<(), String> {
        self.clipboard(|history| history.copy(&id))
    }

    fn clear(&mut self) -> Result<u32, String> {
        Ok(count(self.clipboard(|history| history.clear())?))
    }

    fn delete_items(&mut self, ids: Vec<String>) -> Result<u32, String> {
        Ok(count(self.clipboard(|history| history.delete(&ids))?))
    }

    fn turn_off_and_clear(&mut self) -> Result<u32, String> {
        Ok(count(
            self.clipboard(|history| history.turn_off_and_clear())?,
        ))
    }
}

/// A count for a guest, which cannot exceed `u32` in practice.
fn count(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

impl window_host::Host for GuestState {
    fn close(&mut self, clear_root_search: bool, pop: window_host::PopToRootType) -> bool {
        let _host = self.host();
        let Some(host) = self.host_functions() else {
            return false;
        };
        let pop = match pop {
            window_host::PopToRootType::Default => crate::feedback::PopToRoot::Default,
            window_host::PopToRootType::Immediate => crate::feedback::PopToRoot::Immediate,
            window_host::PopToRootType::Suspended => crate::feedback::PopToRoot::Suspended,
        };
        host.close(&self.caller(), clear_root_search, pop)
    }

    fn pop_to_root(&mut self, clear_search: bool) -> bool {
        let _host = self.host();
        let Some(host) = self.host_functions() else {
            return false;
        };
        host.pop_to_root(&self.caller(), clear_search)
    }

    fn clear_search(&mut self) -> bool {
        let _host = self.host();
        let Some(host) = self.host_functions() else {
            return false;
        };
        host.clear_search(&self.caller())
    }
}

impl feedback_host::Host for GuestState {
    fn show_toast(&mut self, toast: feedback_host::Toast) -> feedback_host::ToastId {
        let _host = self.host();
        let Some(host) = self.host_functions() else {
            return 0;
        };
        host.show_toast(&self.caller(), given(toast))
    }

    fn update_toast(&mut self, id: feedback_host::ToastId, toast: feedback_host::Toast) {
        let _host = self.host();
        if let Some(host) = self.host_functions() {
            host.update_toast(&self.caller(), id, given(toast));
        }
    }

    fn hide_toast(&mut self, id: feedback_host::ToastId) {
        let _host = self.host();
        if let Some(host) = self.host_functions() {
            host.hide_toast(&self.caller(), id);
        }
    }

    // The one shape the 0.2 breaking change retyped: the previous world
    // still styles a HUD with a toast-style, which the host converts as
    // it converts the current world's hud-style — the same drawing,
    // whichever type names it.
    fn show_hud(&mut self, title: String, style: feedback_host::ToastStyle) {
        let _host = self.host();
        if let Some(host) = self.host_functions() {
            host.show_hud(
                &self.caller(),
                crate::feedback::Hud::new(style_of(style), title),
            );
        }
    }
}

/// `toast` as the guest's bindings carry it, for the launcher: the
/// previous world's toast records are the current one's, field for field.
fn given(toast: feedback_host::Toast) -> crate::feedback::GivenToast {
    let action = |action: feedback_host::ToastAction| crate::feedback::GivenAction {
        title: action.title,
        callback: action.callback,
        shortcut: action.shortcut,
    };
    crate::feedback::GivenToast {
        style: style_of(toast.style),
        title: toast.title,
        message: toast.message,
        primary: toast.primary.map(action),
        secondary: toast.secondary.map(action),
    }
}

/// A toast's `toast-style` as the host draws it, in the previous world's
/// bindings.
fn style_of(style: feedback_host::ToastStyle) -> crate::feedback::ToastStyle {
    match style {
        feedback_host::ToastStyle::Animated => crate::feedback::ToastStyle::Animated,
        feedback_host::ToastStyle::Success => crate::feedback::ToastStyle::Success,
        feedback_host::ToastStyle::Failure => crate::feedback::ToastStyle::Failure,
    }
}

impl system_host::Host for GuestState {
    async fn running_on(&mut self) -> system_host::HostSystem {
        match crate::platform::Platform::current() {
            Some(crate::platform::Platform::Windows) => system_host::HostSystem::Windows,
            Some(crate::platform::Platform::Macos) => system_host::HostSystem::Macos,
            Some(crate::platform::Platform::Linux) => system_host::HostSystem::Linux,
            None => system_host::HostSystem::Other,
        }
    }

    async fn copy(&mut self, content: system_host::Clip, concealed: bool) -> Result<(), String> {
        let system = self.system()?;
        let clip = clip_from(content)?;
        self.hosted(super::system_functions::off_thread(
            move || system.copy(&clip, concealed),
            || Err(super::system_functions::failed()),
        ))
        .await
    }

    async fn read_clipboard(&mut self) -> Result<Option<system_host::Clip>, String> {
        let system = self.system()?;
        let read = self
            .hosted(super::system_functions::off_thread(
                move || system.read_clipboard(),
                || Err(super::system_functions::failed()),
            ))
            .await?;
        Ok(read.map(|clip| match clip {
            crate::system::Clip::Text(text) => system_host::Clip::Text(text),
            crate::system::Clip::File(path) => {
                system_host::Clip::File(path.to_string_lossy().into_owned())
            }
        }))
    }

    async fn open(&mut self, target: String, application: Option<String>) -> Result<(), String> {
        let system = self.system()?;
        if target.trim().is_empty() {
            return Err("Nothing to open was named".into());
        }
        let application = application.filter(|application| !application.trim().is_empty());
        let applications = self.applications();
        self.hosted(super::system_functions::off_thread(
            move || {
                // An installed application's id is opened by its source.
                let application = application.map(|application| {
                    crate::applications::opener(applications.as_ref(), &application)
                });
                system.open(&target, application.as_deref())
            },
            || Err(super::system_functions::failed()),
        ))
        .await
    }

    async fn reveal(&mut self, path: String) -> Result<(), String> {
        let system = self.system()?;
        let path = crate::system::absolute(&path)?;
        self.hosted(super::system_functions::off_thread(
            move || system.reveal(&path),
            || Err(super::system_functions::failed()),
        ))
        .await
    }

    async fn trash(&mut self, paths: Vec<String>) -> Result<(), Vec<system_host::NotTrashed>> {
        let wire = |not: crate::system::NotTrashed| system_host::NotTrashed {
            path: not.path.to_string_lossy().into_owned(),
            reason: not.reason,
        };
        let system = match self.system() {
            Ok(system) => system,
            Err(why) => {
                return Err(paths
                    .into_iter()
                    .map(|path| system_host::NotTrashed {
                        path,
                        reason: why.clone(),
                    })
                    .collect());
            }
        };
        let (kept, mut refused) = crate::system::trashable(&paths);
        if !kept.is_empty() {
            let failed_for = kept.clone();
            let not_moved = self
                .hosted(super::system_functions::off_thread(
                    move || system.trash(&kept),
                    move || {
                        failed_for
                            .into_iter()
                            .map(|path| crate::system::NotTrashed {
                                path,
                                reason: super::system_functions::failed(),
                            })
                            .collect()
                    },
                ))
                .await;
            refused.extend(not_moved);
        }
        if refused.is_empty() {
            Ok(())
        } else {
            Err(refused.into_iter().map(wire).collect())
        }
    }

    async fn paste(&mut self, content: system_host::Clip) -> Result<(), system_host::SystemError> {
        let system = self.system().map_err(system_host::SystemError::Failed)?;
        let clip = clip_from(content).map_err(system_host::SystemError::Failed)?;
        // Asked first, so that a paste this system cannot make leaves the
        // window and the clipboard as they were.
        let asked = system.clone();
        self.hosted(super::system_functions::off_thread(
            move || asked.can_paste(),
            || Err(super::system_functions::failed_error()),
        ))
        .await
        .map_err(wire_system_error)?;
        // The application that was in front can only come back once
        // Pane's window has gone.
        self.close_for_paste().map_err(wire_system_error)?;
        self.hosted(super::system_functions::off_thread(
            move || crate::system::paste(system.as_ref(), &clip),
            || Err(super::system_functions::failed_error()),
        ))
        .await
        .map_err(wire_system_error)
    }

    async fn front_application(
        &mut self,
    ) -> Result<Option<system_host::FrontApp>, system_host::SystemError> {
        let system = self.system().map_err(system_host::SystemError::Failed)?;
        let front = self
            .hosted(super::system_functions::off_thread(
                move || system.front_application(),
                || Err(super::system_functions::failed_error()),
            ))
            .await
            .map_err(wire_system_error)?;
        Ok(front.map(|front| system_host::FrontApp {
            name: front.name,
            icon: front.icon,
        }))
    }

    async fn selected_text(&mut self) -> Result<Option<String>, system_host::SystemError> {
        let system = self.system().map_err(system_host::SystemError::Failed)?;
        let selected = self
            .hosted(super::system_functions::off_thread(
                move || system.selected_text(),
                || Err(super::system_functions::failed_error()),
            ))
            .await
            .map_err(wire_system_error)?;
        match selected {
            // An empty selection is no selection.
            Some(text) if text.is_empty() => Ok(None),
            Some(text) if text.len() > crate::system::MAX_CLIPBOARD_TEXT => {
                Err(system_host::SystemError::Failed(format!(
                    "The selected text is longer than the {} MiB Pane gives a command",
                    crate::system::MAX_CLIPBOARD_TEXT / (1024 * 1024)
                )))
            }
            selected => Ok(selected),
        }
    }
}

/// `error` as the previous world's `system` functions carry it.
fn wire_system_error(error: crate::system::SystemError) -> system_host::SystemError {
    match error {
        crate::system::SystemError::NotAvailable(why) => {
            system_host::SystemError::NotAvailable(why)
        }
        crate::system::SystemError::Failed(why) => system_host::SystemError::Failed(why),
    }
}

/// A command's clip as Pane's, in the previous world's bindings: a
/// file's path must be absolute.
fn clip_from(content: system_host::Clip) -> Result<crate::system::Clip, String> {
    Ok(match content {
        system_host::Clip::Text(text) => crate::system::Clip::Text(text),
        system_host::Clip::File(path) => crate::system::Clip::File(crate::system::absolute(&path)?),
    })
}

impl run_host::Host for GuestState {
    async fn run(&mut self, line: String, elevated: bool) -> Result<(), run_host::RunError> {
        let run = self.run_adapter().map_err(run_host::RunError::Failed)?;
        let answer = self
            .hosted(super::run_functions::off_thread(
                move || run.run(&line, elevated),
                || Err(super::run_functions::failed()),
            ))
            .await;
        answer.map_err(wire_run_error)
    }

    async fn history(&mut self) -> Result<Vec<String>, run_host::RunError> {
        let run = self.run_adapter().map_err(run_host::RunError::Failed)?;
        let answer = self
            .hosted(super::run_functions::off_thread(
                move || run.history(),
                || Err(super::run_functions::failed()),
            ))
            .await;
        answer.map_err(wire_run_error)
    }

    async fn delete_from_history(&mut self, line: String) -> Result<(), run_host::RunError> {
        let run = self.run_adapter().map_err(run_host::RunError::Failed)?;
        let answer = self
            .hosted(super::run_functions::off_thread(
                move || run.delete_from_history(&line),
                || Err(super::run_functions::failed()),
            ))
            .await;
        answer.map_err(wire_run_error)
    }

    async fn completions(
        &mut self,
        text: String,
    ) -> Result<Vec<run_host::Completion>, run_host::RunError> {
        let run = self.run_adapter().map_err(run_host::RunError::Failed)?;
        let answer = self
            .hosted(super::run_functions::off_thread(
                move || run.completions(&text),
                || Err(super::run_functions::failed()),
            ))
            .await;
        let completions = answer.map_err(wire_run_error)?;
        Ok(completions.into_iter().map(wire_completion).collect())
    }

    async fn terminal(&mut self) -> Result<Option<String>, run_host::RunError> {
        let run = self.run_adapter().map_err(run_host::RunError::Failed)?;
        let answer = self
            .hosted(super::run_functions::off_thread(
                move || run.terminal(),
                || Err(super::run_functions::failed()),
            ))
            .await;
        answer.map_err(wire_run_error)
    }
}

/// `error` as the previous world's `run` functions carry it.
fn wire_run_error(error: crate::run::RunError) -> run_host::RunError {
    match error {
        crate::run::RunError::NotAvailable(why) => run_host::RunError::NotAvailable(why),
        crate::run::RunError::Failed(why) => run_host::RunError::Failed(why),
        crate::run::RunError::Declined(why) => run_host::RunError::Declined(why),
    }
}

/// `completion` as the previous world's `run` functions carry it.
fn wire_completion(completion: crate::run::Completion) -> run_host::Completion {
    run_host::Completion {
        line: completion.line,
        source: match completion.source {
            crate::run::Source::History => run_host::CompletionSource::History,
            crate::run::Source::AppPath => run_host::CompletionSource::AppPath,
            crate::run::Source::SearchPath => run_host::CompletionSource::SearchPath,
            crate::run::Source::Applet => run_host::CompletionSource::Applet,
            crate::run::Source::Console => run_host::CompletionSource::Console,
            crate::run::Source::Scheme => run_host::CompletionSource::Scheme,
            crate::run::Source::Variable => run_host::CompletionSource::Variable,
        },
    }
}

impl system_commands_host::Host for GuestState {
    async fn lock_screen(&mut self) -> system_commands_host::Outcome {
        wire_outcome(
            self.run_system_command(crate::system_commands::Command::LockScreen)
                .await,
        )
    }

    async fn log_out(&mut self) -> system_commands_host::Outcome {
        wire_outcome(
            self.run_system_command(crate::system_commands::Command::LogOut)
                .await,
        )
    }

    async fn restart(&mut self) -> system_commands_host::Outcome {
        wire_outcome(
            self.run_system_command(crate::system_commands::Command::Restart)
                .await,
        )
    }

    async fn shut_down(&mut self) -> system_commands_host::Outcome {
        wire_outcome(
            self.run_system_command(crate::system_commands::Command::ShutDown)
                .await,
        )
    }

    async fn sleep(&mut self) -> system_commands_host::Outcome {
        wire_outcome(
            self.run_system_command(crate::system_commands::Command::Sleep)
                .await,
        )
    }

    async fn hibernate(&mut self) -> system_commands_host::Outcome {
        wire_outcome(
            self.run_system_command(crate::system_commands::Command::Hibernate)
                .await,
        )
    }

    async fn turn_off_displays(&mut self) -> system_commands_host::Outcome {
        wire_outcome(
            self.run_system_command(crate::system_commands::Command::TurnOffDisplays)
                .await,
        )
    }

    async fn start_screen_saver(&mut self) -> system_commands_host::Outcome {
        wire_outcome(
            self.run_system_command(crate::system_commands::Command::StartScreenSaver)
                .await,
        )
    }

    async fn volume_up(&mut self) -> system_commands_host::Outcome {
        wire_outcome(
            self.run_system_command(crate::system_commands::Command::VolumeUp)
                .await,
        )
    }

    async fn volume_down(&mut self) -> system_commands_host::Outcome {
        wire_outcome(
            self.run_system_command(crate::system_commands::Command::VolumeDown)
                .await,
        )
    }

    async fn toggle_mute(&mut self) -> system_commands_host::Outcome {
        wire_outcome(
            self.run_system_command(crate::system_commands::Command::ToggleMute)
                .await,
        )
    }

    async fn set_volume(&mut self, level: u8) -> system_commands_host::Outcome {
        wire_outcome(
            self.run_system_command(crate::system_commands::Command::SetVolume(level))
                .await,
        )
    }

    async fn toggle_microphone_mute(&mut self) -> system_commands_host::Outcome {
        wire_outcome(
            self.run_system_command(crate::system_commands::Command::ToggleMicrophoneMute)
                .await,
        )
    }

    async fn open_recycle_bin(&mut self) -> system_commands_host::Outcome {
        wire_outcome(
            self.run_system_command(crate::system_commands::Command::OpenRecycleBin)
                .await,
        )
    }

    async fn empty_recycle_bin(&mut self) -> system_commands_host::Outcome {
        wire_outcome(
            self.run_system_command(crate::system_commands::Command::EmptyRecycleBin)
                .await,
        )
    }

    async fn toggle_appearance(&mut self) -> system_commands_host::Outcome {
        wire_outcome(
            self.run_system_command(crate::system_commands::Command::ToggleAppearance)
                .await,
        )
    }

    async fn toggle_hdr(&mut self) -> system_commands_host::Outcome {
        wire_outcome(
            self.run_system_command(crate::system_commands::Command::ToggleHdr)
                .await,
        )
    }

    async fn show_desktop(&mut self) -> system_commands_host::Outcome {
        wire_outcome(
            self.run_system_command(crate::system_commands::Command::ShowDesktop)
                .await,
        )
    }

    async fn toggle_hidden_files(&mut self) -> system_commands_host::Outcome {
        wire_outcome(
            self.run_system_command(crate::system_commands::Command::ToggleHiddenFiles)
                .await,
        )
    }

    async fn eject_removable_drives(&mut self) -> system_commands_host::Outcome {
        wire_outcome(
            self.run_system_command(crate::system_commands::Command::EjectRemovableDrives)
                .await,
        )
    }

    async fn toggle_bluetooth(&mut self) -> system_commands_host::Outcome {
        wire_outcome(
            self.run_system_command(crate::system_commands::Command::ToggleBluetooth)
                .await,
        )
    }
}

/// `outcome` as the previous world's system commands carry it: the host
/// half answers the host's own, which either world wires the same way.
fn wire_outcome(outcome: crate::system_commands::Outcome) -> system_commands_host::Outcome {
    match outcome {
        crate::system_commands::Outcome::Done(text) => system_commands_host::Outcome::Done(text),
        crate::system_commands::Outcome::Explained(text) => {
            system_commands_host::Outcome::Explained(text)
        }
    }
}

impl windows_host::Host for GuestState {
    async fn list_windows(
        &mut self,
    ) -> Result<Vec<windows_host::Window>, windows_host::WindowsError> {
        let windows = self
            .windows_adapter()
            .map_err(windows_host::WindowsError::Failed)?;
        let answer = self
            .hosted(super::windows_functions::off_thread(
                move || windows.list(),
                || Err(super::windows_functions::failed()),
            ))
            .await;
        answer
            .map_err(wire_windows_error)
            .map(|listed| listed.into_iter().map(wire_window).collect())
    }

    async fn activate(&mut self, id: String) -> Result<(), windows_host::WindowsError> {
        let windows = self
            .windows_adapter()
            .map_err(windows_host::WindowsError::Failed)?;
        let answer = self
            .hosted(super::windows_functions::off_thread(
                move || windows.activate(&id),
                || Err(super::windows_functions::failed()),
            ))
            .await;
        answer.map_err(wire_windows_error)
    }
}

/// `window` as the previous world's `windows` functions carry it.
fn wire_window(window: crate::switch_windows::Window) -> windows_host::Window {
    windows_host::Window {
        id: window.id,
        title: window.title,
        application_name: window.application,
        icon: window.icon,
        minimized: window.minimized,
        maximized: window.maximized,
        elsewhere: window.elsewhere,
        elevated: window.elevated,
    }
}

/// `error` as the previous world's `windows` functions carry it.
fn wire_windows_error(error: crate::switch_windows::WindowsError) -> windows_host::WindowsError {
    match error {
        crate::switch_windows::WindowsError::NotAvailable(why) => {
            windows_host::WindowsError::NotAvailable(why)
        }
        crate::switch_windows::WindowsError::Failed(why) => windows_host::WindowsError::Failed(why),
    }
}

impl typed_folder_host::Host for GuestState {
    async fn list_entries(
        &mut self,
        folder: String,
    ) -> Result<typed_folder_host::FolderListing, String> {
        // Answers from what the listing thread found; the runtime thread
        // awaits it, serving other packages' calls meanwhile.
        let _host = self.host();
        // Stopped code starts no more work: checked once the host call is
        // marked, from when Pane no longer decides to give up on the
        // thread.
        if let Some(end) = self.stopped() {
            return Err(stopped_code(end));
        }
        let Some(owner) = self.owner() else {
            return Err("Pane lists a typed folder for installed extensions".into());
        };
        let access = self.file_access();
        let home = crate::typed_folder::home(&access);
        self.hosted(crate::typed_folder::off_thread(
            move || access.typed().list(&owner, folder.trim(), home.as_deref()),
            || Err(crate::typed_folder::failed()),
        ))
        .await
        .map(|listing| typed_folder_host::FolderListing {
            entries: listing
                .entries
                .into_iter()
                .map(|entry| typed_folder_host::FolderEntry {
                    id: entry.id,
                    name: entry.name,
                    folder: entry.folder,
                    program: entry.program,
                })
                .collect(),
            truncated: listing.truncated,
        })
    }
}

impl files::Host for GuestState {
    fn list_folder(&mut self) -> Result<files::FolderState, String> {
        // Answers from what the listing worker found; never waits for it.
        let _host = self.host();
        // Stopped code starts no more work: checked once the host call is
        // marked, from when Pane no longer decides to give up on the
        // thread.
        if let Some(end) = self.stopped() {
            return Err(stopped_code(end));
        }
        let Some(owner) = self.owner() else {
            return Ok(files::FolderState::NotGranted);
        };
        let generation = self.generation().cloned();
        Ok(
            match self
                .file_access()
                .folder_state(&owner, generation.as_ref())?
            {
                crate::files::FolderState::NotGranted => files::FolderState::NotGranted,
                crate::files::FolderState::Listing => files::FolderState::Listing,
                crate::files::FolderState::Ready { files, truncated } => {
                    files::FolderState::Ready(files::FolderListing {
                        files: files
                            .into_iter()
                            .map(|(id, relative)| files::FoundFile { id, relative })
                            .collect(),
                        truncated,
                    })
                }
            },
        )
    }

    fn limits(&mut self) -> files::ScanLimits {
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        files::ScanLimits {
            depth: count(crate::files::MAX_DEPTH),
            files: count(crate::files::MAX_FILES),
            entries: count(crate::files::MAX_ENTRIES),
        }
    }
}

impl file_index::Host for GuestState {
    fn search(
        &mut self,
        query: String,
        options: file_index::SearchOptions,
    ) -> Result<Vec<file_index::FileEntry>, String> {
        // Answers from what the index holds; never waits for a walk.
        let _host = self.host();
        if let Some(end) = self.stopped() {
            return Err(stopped_code(end));
        }
        let Some(owner) = self.owner() else {
            return Err("Pane's file index is for installed extensions".into());
        };
        let found = self.file_access().indexer().search(
            &owner,
            &query,
            crate::file_index::SearchOptions {
                kind: options.kind.map(kind_of),
                category: options.category.map(category_of),
                sort: match options.sort {
                    file_index::Sort::Relevance => crate::file_index::Sort::Relevance,
                    file_index::Sort::Modified => crate::file_index::Sort::Modified,
                },
                limit: options.limit as usize,
                offset: options.offset as usize,
            },
        )?;
        Ok(found
            .into_iter()
            .map(|found| file_index::FileEntry {
                id: found.id,
                path: found.path.to_string_lossy().into_owned(),
                name: found.name,
                folder: found.folder,
                kind: wit_kind(found.kind),
                program: found.program,
                size: found.size,
                modified: found.modified,
                volume: found.volume,
            })
            .collect())
    }

    fn status(&mut self) -> file_index::IndexStatus {
        let status = self.file_access().indexer().status();
        file_index::IndexStatus {
            state: match status.state {
                crate::file_index::IndexState::Off => file_index::IndexState::Off,
                crate::file_index::IndexState::Building => file_index::IndexState::Building,
                crate::file_index::IndexState::Current => file_index::IndexState::Current,
                crate::file_index::IndexState::Stopped => file_index::IndexState::Stopped,
            },
            entries: status.entries,
            found: status.found,
            reason: status.reason,
        }
    }
}

/// `kind` as the host searches by, in the previous world's bindings.
fn kind_of(kind: file_index::EntryKind) -> crate::file_index::EntryKind {
    match kind {
        file_index::EntryKind::File => crate::file_index::EntryKind::File,
        file_index::EntryKind::Folder => crate::file_index::EntryKind::Folder,
        file_index::EntryKind::Link => crate::file_index::EntryKind::Link,
    }
}

/// `kind` as the previous world's file index carries it.
fn wit_kind(kind: crate::file_index::EntryKind) -> file_index::EntryKind {
    match kind {
        crate::file_index::EntryKind::File => file_index::EntryKind::File,
        crate::file_index::EntryKind::Folder => file_index::EntryKind::Folder,
        crate::file_index::EntryKind::Link => file_index::EntryKind::Link,
    }
}

/// `category` as the host searches by, in the previous world's bindings.
fn category_of(category: file_index::Category) -> crate::file_index::Category {
    match category {
        file_index::Category::Documents => crate::file_index::Category::Documents,
        file_index::Category::Images => crate::file_index::Category::Images,
        file_index::Category::Audio => crate::file_index::Category::Audio,
        file_index::Category::Video => crate::file_index::Category::Video,
        file_index::Category::Archives => crate::file_index::Category::Archives,
        file_index::Category::Applications => crate::file_index::Category::Applications,
        file_index::Category::Text => crate::file_index::Category::Text,
        file_index::Category::Other => crate::file_index::Category::Other,
    }
}

impl helpers::Host for GuestState {}

impl<T> helpers::HostWithStore<T> for crate::helpers::Runs {
    async fn run(
        accessor: &Accessor<T, Self>,
        helper: String,
        args: Vec<String>,
        input: String,
    ) -> Result<String, helpers::HelperError> {
        let (started, watch) = accessor.with(|mut view| {
            let state = view.get();
            (state.start_helper(helper, args, input), state.watch())
        });
        // Waiting for the helper is not the guest's computing, nor a
        // hang: the runtime thread only awaits it. Dropped here if the
        // guest cancels the call: the process ends.
        let result =
            super::deadlines::hosted(watch, async move { started.await?.finish().await }).await;
        result.map_err(helpers::HelperError::from)
    }
}

/// Pane's helper error as the previous world's helpers carry it.
impl From<crate::helpers::runner::HelperError> for helpers::HelperError {
    fn from(error: crate::helpers::runner::HelperError) -> helpers::HelperError {
        use crate::helpers::runner::HelperErrorKind;
        use helpers::HelperErrorKind as Wit;
        let kind = match error.kind {
            HelperErrorKind::NotFound => Wit::NotFound,
            HelperErrorKind::Unavailable => Wit::Unavailable,
            HelperErrorKind::Failed => Wit::Failed,
            HelperErrorKind::Refused => Wit::Refused,
        };
        helpers::HelperError {
            kind,
            message: error.message,
        }
    }
}

impl operations::Host for GuestState {}

impl<T> operations::HostWithStore<T> for crate::operations::Calls {
    async fn call(
        accessor: &Accessor<T, Self>,
        source: String,
        operation: String,
        version: u32,
        input: String,
    ) -> Result<String, operations::CallError> {
        let answer = crate::operations::send(
            accessor,
            crate::operations::Addressed::Package { source, version },
            operation,
            input,
        )
        .await
        .map_err(operations::CallError::from)?;
        match answer {
            crate::operations::OperationAnswer::One(result) => {
                result.map_err(operations::CallError::from)
            }
            // Only a call addressed to every provider is answered with
            // every provider's answers.
            crate::operations::OperationAnswer::Every(_) => {
                unreachable!("a call to one target answered with many")
            }
        }
    }

    async fn call_capability(
        accessor: &Accessor<T, Self>,
        capability: String,
        operation: String,
        input: String,
    ) -> Result<String, operations::CallError> {
        let answer = crate::operations::send(
            accessor,
            crate::operations::Addressed::Capability { capability },
            operation,
            input,
        )
        .await
        .map_err(operations::CallError::from)?;
        match answer {
            crate::operations::OperationAnswer::One(result) => {
                result.map_err(operations::CallError::from)
            }
            // Only a call addressed to every provider is answered with
            // every provider's answers.
            crate::operations::OperationAnswer::Every(_) => {
                unreachable!("a call to one target answered with many")
            }
        }
    }

    async fn call_every(
        accessor: &Accessor<T, Self>,
        capability: String,
        operation: String,
        input: String,
    ) -> Result<Vec<operations::ProviderAnswer>, operations::CallError> {
        let answer = crate::operations::send(
            accessor,
            crate::operations::Addressed::Every { capability },
            operation,
            input,
        )
        .await
        .map_err(operations::CallError::from)?;
        match answer {
            crate::operations::OperationAnswer::Every(result) => result
                .map(|answers| {
                    answers
                        .into_iter()
                        .map(|answer| operations::ProviderAnswer {
                            provider: answer.identity.key(),
                            title: answer.title,
                            answer: answer.answer.map_err(operations::CallError::from),
                        })
                        .collect()
                })
                .map_err(operations::CallError::from),
            // Only a call addressed to one target is answered with one
            // target's answer.
            crate::operations::OperationAnswer::One(_) => {
                unreachable!("a fan-out answered with one answer")
            }
        }
    }

    fn providers(mut host: Access<'_, T, Self>, capability: String) -> Vec<operations::Provider> {
        let state = host.get();
        let installed = state.installed();
        let caller = state.component.clone();
        installed
            .capable_providers(&caller, &capability)
            .into_iter()
            .map(|(source, title)| operations::Provider { source, title })
            .collect()
    }
}

/// Pane's operation error as the previous world's operations carry it.
impl From<crate::operations::OperationError> for operations::CallError {
    fn from(error: crate::operations::OperationError) -> operations::CallError {
        use crate::operations::OperationErrorKind;
        use operations::CallErrorKind as Wit;
        let kind = match error.kind {
            OperationErrorKind::NotFound => Wit::NotFound,
            OperationErrorKind::Disabled => Wit::Disabled,
            OperationErrorKind::Incompatible => Wit::Incompatible,
            OperationErrorKind::Unavailable => Wit::Unavailable,
            OperationErrorKind::Failed => Wit::Failed,
            OperationErrorKind::Crashed => Wit::Crashed,
            OperationErrorKind::Refused => Wit::Refused,
        };
        operations::CallError {
            kind,
            message: error.message,
        }
    }
}

impl programs::Host for GuestState {}

impl<T> programs::HostWithStore<T> for crate::programs::Calls {
    async fn run(
        accessor: &Accessor<T, Self>,
        program: String,
        args: Vec<String>,
        input: Vec<u8>,
        options: programs::Options,
    ) -> Result<programs::Output, programs::ProgramError> {
        let request = request(program, args, options);
        let (owner, watch) = accessor.with(|mut view| {
            let state = view.get();
            (
                crate::programs::owner_for(state, &request, &input),
                state.watch(),
            )
        });
        let owner = owner?;
        // Waiting for the program is not the guest's computing, nor a
        // hang. Dropped here if the guest cancels the call: the program
        // ends.
        super::deadlines::hosted(watch, crate::programs::runner::run(request, input, owner))
            .await
            .map(programs::Output::from)
            .map_err(programs::ProgramError::from)
    }

    async fn spawn(
        accessor: &Accessor<T, Self>,
        program: String,
        args: Vec<String>,
        options: programs::Options,
    ) -> Result<u64, programs::ProgramError> {
        let request = request(program, args, options);
        if request.elevated {
            return Err(crate::programs::runner::ProgramError::new(
                crate::programs::runner::ErrorKind::Unavailable,
                format!(
                    "{} cannot be spawned elevated: an elevated program's streams cannot be \
                     reached; run it instead",
                    request.shown()
                ),
            )
            .into());
        }
        let (owner, watch) = accessor.with(|mut view| {
            let state = view.get();
            (
                crate::programs::owner_for(state, &request, &[]),
                state.watch(),
            )
        });
        let owner = owner?;
        let spawned =
            super::deadlines::hosted(watch, crate::programs::runner::spawn(request, owner)).await?;
        Ok(accessor.with(|mut view| view.get().programs.insert(spawned)))
    }

    async fn write_input(
        accessor: &Accessor<T, Self>,
        process: u64,
        bytes: Vec<u8>,
    ) -> Result<(), programs::ProgramError> {
        if bytes.len() > crate::programs::runner::MAX_PROGRAM_INPUT {
            return Err(crate::programs::runner::ProgramError::new(
                crate::programs::runner::ErrorKind::Refused,
                format!(
                    "{} bytes were given at once; at most {} are written",
                    bytes.len(),
                    crate::programs::runner::MAX_PROGRAM_INPUT
                ),
            )
            .into());
        }
        let (writing, watch) = accessor.with(|mut view| {
            let state = view.get();
            let writing = state
                .programs
                .get(process)
                .map(|spawned| spawned.write(bytes));
            (writing, state.watch())
        });
        let writing = writing?;
        super::deadlines::hosted(watch, writing)
            .await
            .map_err(programs::ProgramError::from)
    }

    async fn close_input(
        accessor: &Accessor<T, Self>,
        process: u64,
    ) -> Result<(), programs::ProgramError> {
        accessor.with(|mut view| {
            view.get()
                .programs
                .get(process)
                .map(|spawned| spawned.close_input())
                .map_err(programs::ProgramError::from)
        })
    }

    async fn read_output(
        accessor: &Accessor<T, Self>,
        process: u64,
    ) -> Result<Option<Vec<u8>>, programs::ProgramError> {
        crate::programs::read(accessor, process, crate::programs::runner::Stream::Output)
            .await
            .map_err(programs::ProgramError::from)
    }

    async fn read_error(
        accessor: &Accessor<T, Self>,
        process: u64,
    ) -> Result<Option<Vec<u8>>, programs::ProgramError> {
        crate::programs::read(accessor, process, crate::programs::runner::Stream::Errors)
            .await
            .map_err(programs::ProgramError::from)
    }

    async fn wait(
        accessor: &Accessor<T, Self>,
        process: u64,
    ) -> Result<Option<i32>, programs::ProgramError> {
        let (waiting, watch) = accessor.with(|mut view| {
            let state = view.get();
            (
                state.programs.get(process).map(|spawned| spawned.wait()),
                state.watch(),
            )
        });
        let waiting = waiting?;
        super::deadlines::hosted(watch, waiting)
            .await
            .map_err(programs::ProgramError::from)
    }

    async fn kill(
        accessor: &Accessor<T, Self>,
        process: u64,
    ) -> Result<(), programs::ProgramError> {
        accessor.with(|mut view| {
            view.get()
                .programs
                .get(process)
                .map(|spawned| spawned.kill())
                .map_err(programs::ProgramError::from)
        })
    }
}

/// `program`, `args` and `options` as Pane runs them, in the previous
/// world's bindings: the conversion `crate::programs::request` is for the
/// current world's.
fn request(
    program: String,
    args: Vec<String>,
    options: programs::Options,
) -> crate::programs::runner::Request {
    crate::programs::runner::Request {
        program,
        args,
        folder: options.folder,
        environment: options
            .environment
            .into_iter()
            .map(|variable| (variable.name, variable.value))
            .collect(),
        timeout: options
            .timeout_ms
            .map(|milliseconds| std::time::Duration::from_millis(milliseconds.into())),
        show_window: options.show_window,
        elevated: options.elevated,
    }
}

/// The output of a program the guest ran, in the previous world's
/// bindings.
impl From<crate::programs::runner::Output> for programs::Output {
    fn from(output: crate::programs::runner::Output) -> programs::Output {
        programs::Output {
            exit_code: output.exit_code,
            stdout: output.stdout,
            stderr: output.stderr,
        }
    }
}

/// Pane's program error as the previous world's programs carry it.
impl From<crate::programs::runner::ProgramError> for programs::ProgramError {
    fn from(error: crate::programs::runner::ProgramError) -> programs::ProgramError {
        use crate::programs::runner::ErrorKind;
        use programs::ProgramErrorKind as Wit;
        let kind = match error.kind {
            ErrorKind::NotFound => Wit::NotFound,
            ErrorKind::Unavailable => Wit::Unavailable,
            ErrorKind::Declined => Wit::Declined,
            ErrorKind::TimedOut => Wit::TimedOut,
            ErrorKind::TooMuchOutput => Wit::TooMuchOutput,
            ErrorKind::Refused => Wit::Refused,
            ErrorKind::Failed => Wit::Failed,
        };
        programs::ProgramError {
            kind,
            message: error.message,
        }
    }
}

impl<T> feedback_host::HostWithStore<T> for super::host_functions::Confirms {
    async fn confirm(
        accessor: &Accessor<T, Self>,
        confirmation: feedback_host::Confirmation,
    ) -> Result<bool, String> {
        let (asking, watch) = accessor.with(|mut view| {
            let state = view.get();
            (
                state.ask_to_confirm(given_confirmation(confirmation)),
                state.watch(),
            )
        });
        // Waiting for the user is not the guest's computing, nor a hang:
        // the runtime thread only awaits it.
        super::deadlines::hosted(watch, asking).await
    }
}

/// `confirmation` as the current world's bindings carry it, for the host
/// half of `confirm` both worlds share.
fn given_confirmation(
    confirmation: feedback_host::Confirmation,
) -> super::feedback_host::Confirmation {
    super::feedback_host::Confirmation {
        title: confirmation.title,
        message: confirmation.message,
        primary: confirmation.primary,
        destructive: confirmation.destructive,
        dismiss: confirmation.dismiss,
        remember: confirmation.remember,
    }
}
