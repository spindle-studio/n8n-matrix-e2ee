# Contributing

For a substantial change, open an issue first so we can agree on the approach.
Small fixes can go straight to a pull request.

The [development guide](AGENTS.md) covers local setup and build and test commands.
For node changes, also read [nodes/AGENTS.md](nodes/AGENTS.md).

Keep pull requests focused. Explain what changed and how you tested it, and
update the documentation if users need to do anything differently. Run the
relevant lint, build, and test commands before submitting.

Use made-up examples in code and reports. Keep credentials, private messages,
Matrix session files, and encryption stores out of commits and logs you share.

## Releases

Use Conventional Commit PR titles, such as `fix(nodes): handle missing metadata`
or `feat(gateway): add a message type`. Add `!` for a breaking change.

Release-please manages versions and changelogs for each component. Don't bump
versions manually; see the [release guide](AGENTS.md#independent-releases-with-release-please).
