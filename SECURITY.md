# Security

Report vulnerabilities through [GitHub's private reporting form](https://github.com/spindle-studio/n8n-matrix-e2ee/security/advisories/new)
or email [ben@spindle.studio](mailto:ben@spindle.studio). Please don't post exploit
details in public issues.

Describe the problem and how to reproduce it using made-up data. Remove
passwords, tokens, private messages, and other sensitive details from logs.
Never attach Matrix session files or encryption stores.

## Running the gateway

Keep the gateway on a trusted private network. Anyone with its API token and
network access can use the bot, so use a strong, unique token and don't expose
the service directly to the internet.

Protect `.env` and the `/data` volume: they hold the bot's credentials, session,
and encryption keys.
