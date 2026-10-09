# Security policy

## Reporting a vulnerability

Please report suspected vulnerabilities privately to [ben@spindle.studio](mailto:ben@spindle.studio)
or use GitHub's private vulnerability reporting feature when it is enabled.
Do not disclose exploit details in public issues.

Do not include Matrix passwords, API tokens, access tokens, room content,
`matrix_session.json`, `matrix_store`, or full unredacted logs in a report.

## Operational security

The gateway holds Matrix credentials and E2EE state and is intended only for a
trusted internal network. Restrict access to the gateway, use a strong unique
`API_SECRET_TOKEN`, protect the persistent `/data` volume, and do not expose the
service directly to the public internet.
