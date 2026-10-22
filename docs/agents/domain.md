# Domain docs

Pane uses a single-context layout: the glossary is `CONTEXT.md` at the workspace root; architectural decisions live in `docs/adr/`.

## Before design or code exploration

1. Read `CONTEXT.md` and use its domain vocabulary.
2. Read `docs/HANDOFF.md` for where the work stands, then `docs/current-decisions.md` for the decision status it reconciled on 2026-09-28; later decisions are in the later ADRs and the open specifications. Follow references into `docs/launcher-design-interview.md` for the history and rationale.
3. Read ADRs relevant to the area being worked on.

The current user task takes precedence over historical phase notes. Later explicit user decisions supersede earlier proposals and runtime choices; preserve the distinction between accepted decisions, provisional directions and experimental evidence.

If a domain document is absent, proceed with available context. Create domain documents through `/domain-modeling` when actual terminology or decisions need recording.

## Vocabulary and decisions

Use glossary terms in specs, tickets, tests and implementation discussions. Record meaningful terminology gaps through `/domain-modeling`.

When a proposed change conflicts with an ADR, identify the conflict and its reason. If a later user decision already supersedes that ADR, reference that decision rather than silently restoring the historical choice.

## Research and prototypes

Follow relevant references into `docs/research/` for evidence and limitations. A successful prototype is not proof that its production SDK, cross-platform support or runtime lifecycle is complete.
