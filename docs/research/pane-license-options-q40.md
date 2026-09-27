# Pane license options — Q40

Researched 2026-09-28. Follow-up: user selects "like zed for now". Record Zed-style licensing as the provisional direction: primarily GPL-3.0-or-later, with explicitly marked Apache-2.0 components considered separately. The SDK/component split remains undecided; no license files have been applied. The comparison below explains that this direction permits compliant paid forks.

## What the user wants

Forks and modifications should be allowed and remain open source. The user also does not want someone to fork Pane and sell a paid version. The remaining question is whether the objection is to **charging at all**, or to **closing the source of a redistributed fork**.

These are different requirements. Open-source licenses must allow commercial use and selling copies. Copyleft can preserve recipients' source and redistribution rights, but cannot prohibit paid forks while satisfying the Open Source Definition. A restriction on commercial sale would need a different, source-available licensing direction. [OSI definition](https://opensource.org/osd), [OSI FAQ](https://opensource.org/faq), [OSI rejection criteria](https://opensource.org/licenses/common-reasons-for-rejection-of-licenses).

## Examples checked against their current primary sources

| Project | Current source-license statement | Relevance to Pane |
| --- | --- | --- |
| Pi | MIT. | Permissive; not a keep-forks-open policy. [Pi LICENSE](https://github.com/earendil-works/pi/blob/main/LICENSE). |
| VS Code / Code-OSS | Code-OSS repository is MIT; Microsoft's VS Code distribution has a separate Microsoft product license. | Do not describe the branded binary and repository as identically licensed. [Microsoft FAQ](https://code.visualstudio.com/docs/supporting/faq#what-is-the-difference-between-the-vscode-repository-and-the-microsoft-visual-studio-code-distribution). |
| OBS Studio | GPL v2 or any later version. | An established desktop app using copyleft. [OBS README](https://github.com/obsproject/obs-studio#what-is-obs-studio). |
| Zed | Primarily GPL-3.0-or-later; Apache-2.0 components where marked. | Current mixed licensing, verified from the repository; do not substitute an older description of its license split. [Zed licensing](https://github.com/zed-industries/zed#licensing). |

These examples are alternatives, not a popularity ranking or a completed dependency-license audit.

## GPL and AGPL: what they would protect

**GPLv3** is a candidate if the desired rule is that distributed modifications to Pane remain under copyleft. Sellers must comply with the source-access and redistribution terms; recipients may redistribute their copies, including without charging. Private modifications do not automatically have to be published. The obligation is not a blanket requirement to upload every change to GitHub or contribute it upstream. [GNU FAQ on commercial distribution](https://www.gnu.org/licenses/gpl-faq.en.html#GPLCommercially), [private modifications](https://www.gnu.org/licenses/gpl-faq.html.en#UnreleasedMods), [public availability](https://www.gnu.org/licenses/gpl-faq.en.html#DoesTheGPLRequireAvailabilityToPublic).

**AGPLv3** adds a condition for modified versions that support users interacting with them remotely over a network: those users must be offered the corresponding source. This addresses modified services that are operated without distributing program copies. It does not ban charging, and is not triggered simply because a desktop app downloads updates or makes outgoing API requests. [AGPL section 13](https://www.gnu.org/licenses/agpl.html#section13).

For Pane's current desktop-launcher scope, GPLv3 is the simpler starting recommendation **if paid forks that preserve the required source rights are acceptable**. Consider AGPLv3 if source access for users of modified network-serving versions is also a concrete objective. If all paid forks must be forbidden, neither meets the user's requirement; resolve the open-source versus commercial-restriction tradeoff before choosing a license.

## SDK, extensions, and branding remain separate decisions

Do not silently apply the launcher license to every extension or promise that every extension can have any license. The SDK's license and any explicit extension exception remain undecided. A WIT/WASI boundary by itself does not settle whether components form a combined work; GNU's guidance considers the nature of the integration and communication. [GNU plugin guidance](https://www.gnu.org/licenses/gpl-faq.en.html#GPLPlugins).

The user selected **Pane** as the product name. Name availability, any trademark policy, and dependency-license compatibility have not been cleared in this note. No LICENSE file was added.

## Next grilling question

Would a paid fork be acceptable if its recipients receive the source rights, can modify it, and can redistribute it freely—or must charging for forks itself be prohibited?
