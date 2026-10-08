# Issue tracker: GitHub Issues

Specs, implementation tickets, planning prerequisites and release checklists live in [pane-app/pane](https://github.com/pane-app/pane/issues). Use the `gh` CLI with `--repo pane-app/pane`, or `gh api` for relationships.

## Read and publish

- Read an issue with `gh issue view <number> --repo pane-app/pane --json number,title,body,labels,state,assignees,comments`.
- List work with `gh issue list --repo pane-app/pane --state open --limit 100 --json number,title,labels,assignees` and an appropriate label filter.
- Publish with `gh issue create --repo pane-app/pane --title "..." --body-file <file> --label <role>`. Use a UTF-8 body file for multiline creates, edits and comments.
- Use the five triage labels in [triage-labels.md](triage-labels.md). Tickets produced by `/to-tickets` use `ready-for-agent`; they need no incoming-issue triage. Readiness does not clear open blockers.
- Specs use `specification`; implementation tickets use `implementation`; bounded planning work uses `prerequisite`; release checklists use `release-validation`.
- Add implementation and planning issues as native sub-issues of their specification. Include the parent URL in each body. Keep acceptance checkboxes and a readable `Blocked by` section in each ticket.
- Append discussion as GitHub comments. Record actual results before closing an issue. Close completed blockers as completed; a cancelled blocker needs an explicit dependency decision.
- GitHub issue numbers are authoritative. Use absolute GitHub URLs for issue and repository-document links; local Markdown ticket copies are no longer maintained.

## Dependencies and execution

Use native relationships, with database IDs from `gh api repos/pane-app/pane/issues/<number> --jq .id`:

- Parent: `gh api --method POST repos/pane-app/pane/issues/<parent>/sub_issues -F sub_issue_id=<child-db-id>`.
- Blocker: `gh api --method POST repos/pane-app/pane/issues/<child>/dependencies/blocked_by -F issue_id=<blocker-db-id>`.
- Inspect blockers with `gh api repos/pane-app/pane/issues/<child>/dependencies/blocked_by` and parent children with `gh api --paginate repos/pane-app/pane/issues/<parent>/sub_issues`.

Work open, unassigned implementation issues whose blockers are completed, in parent order. Check prerequisite issues and any platform-specific conditions in the body. Claim with `gh issue edit <number> --repo pane-app/pane --add-assignee @me`. Parentage alone is not a blocking dependency.

## Wayfinding

The map is an issue labelled `wayfinder:map` with Notes, Decisions-so-far and Fog. Its native sub-issues carry `wayfinder:research`, `wayfinder:prototype`, `wayfinder:grilling` or `wayfinder:task`; create those labels when needed. Open/unassigned means available, an assignee means claimed, and closed as completed means resolved. Use native blocker relationships and choose the first open, unassigned, unblocked child in map order. Resolve by posting the answer, closing the child, and adding its gist and URL to the map's Decisions-so-far.

PRs as a request surface: no. GitHub shares numbering between issues and PRs; check the resource type when a reference is ambiguous.
