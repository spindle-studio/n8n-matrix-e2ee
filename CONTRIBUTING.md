# Contributing

Thanks for contributing. Start by opening an issue for significant changes so
the intended behavior and compatibility impact are clear.

## Development

Follow the root [AGENTS.md](AGENTS.md) for architecture, local setup, and
validation commands. For changes under the n8n package, also follow its scoped
[`AGENTS.md`](nodes/AGENTS.md).

Keep changes focused, update user-facing documentation when behavior changes,
and run the relevant existing build, lint, and test commands before opening a
pull request.

## Pull requests

Describe the problem, implementation, validation, and any deployment or
compatibility considerations. Do not include Matrix access tokens, passwords,
room content, session files, encryption stores, or unredacted logs. Use
synthetic examples and redact identifiers where possible.

## Release notes

Use Conventional Commit titles when squash-merging pull requests, such as
`fix(nodes): handle missing metadata` or `feat(gateway): add a message type`.
Add `!` for breaking changes. Release-please maintains separate release PRs,
versions, and changelogs for `nodes/` and `gateway/`; see
[the release guide](AGENTS.md#independent-releases-with-release-please).
