# Use Raycast-style search with extension-provided features

Accepted in Q17, 2026-09-27. Main search shows applications, extension commands, quicklinks, calculator answers and enabled file results. Online service content is searched inside the selected integration command, with aliases, hotkeys and fallback actions providing quick access; automatic queries across every online integration are deferred from the first version.

The core owns the search interface, command registry, matching/ranking, aggregation, navigation and action dispatch. Disableable default extensions supply feature-specific entries and results, so appearing directly in root search does not make a feature mandatory core functionality. This combines Raycast's familiar search behavior with the accepted Pi-style small-core extension architecture.

The exact provider API and ranking algorithm remain implementation work. Leave room for additional root-search provider modes without committing to a global online-search API at launch. This decision does not imply Raycast extension compatibility.
