# The launcher borrows Raycast's polish within the accepted UI

Accepted 2026-10-06 by the user's decision, after the [Raycast deep dive](../research/raycast-deep-dive.md#look-and-motion) (its decision 2). It amends [ADR 0029](0029-the-ui-is-accepted-as-built.md) without reopening it. The launcher's accepted design stays: its layout, material, type, theme, pinned home ([ADR 0027](0027-quick-slots-are-an-ordered-list.md)) and background image ([ADR 0028](0028-the-launcher-draws-a-background-image-the-user-chooses.md)). It takes the following details from Raycast. Raycast's proportions and its palette are not taken.

- **Selection without an edge.** A selected row is drawn as a wash of the text colour at a low alpha (Raycast's is 10%). Where hovering does not select, a hovered one gets a fainter wash (5%). Neither has a border or ring, so the selected row's 1px inset edge goes, and so does the computed answer card's accent ring. A control that takes the keyboard, such as a Settings control or a field, keeps its focus ring. Over a background image, the selected row stays frosted as ADR 0028 draws it.
- **Bare application icons.** An application's own icon is drawn without a tile behind it once Pane has that icon, through the applications specification's icon extraction. Built-in commands keep their tiles.
- **A late loading indicator.** The thin loading indicator under the search field appears only after work has run for 300 ms, so an answer that comes quickly shows none.
- **Feedback that leaves.** A toast hides itself after 3 seconds. Today's status line instead keeps a message until the next one replaces it. A HUD shows for 1.2 seconds, or 3 seconds for a failure. An animated, in-progress toast stays until its command updates or hides it.
- **Number hints after a hold.** The numbers show after Ctrl is held alone for 400 ms, as ADR 0027 already has it.
- **Text levels by alpha.** Where it fits, secondary and tertiary text use the primary text colour at a lower alpha (Raycast uses 100, 60, 40 and 20%), so the levels hold over the glass material and a background image alike. Where a level's contrast would suffer, the accepted colour stays.

As ADR 0029 says, the result is judged in the app by the user, and window tests guard its behaviour.

The user is happy with Pane's design (ADR 0029), and these are the details where Raycast, after years of use, reads better. An edge on the selection draws another line each time the selection moves. A tile boxes an icon that already has its own shape. A loading bar that flashes for an instant answer is noise. A message that never leaves keeps old news on screen. Each change is small and keeps the accepted look. Raycast's proportions would replace that look, so they were not taken.
