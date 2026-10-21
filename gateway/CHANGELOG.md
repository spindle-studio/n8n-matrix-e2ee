# Changelog

## [0.5.0](https://github.com/spindle-studio/n8n-matrix-e2ee/compare/gateway-v0.4.0...gateway-v0.5.0) (2026-10-21)


### ⚠ BREAKING CHANGES

* **gateway:** support multiple Matrix accounts via TOML config ([#27](https://github.com/spindle-studio/n8n-matrix-e2ee/issues/27))

### Features

* **gateway:** cache uploaded images per room in SQLite ([#25](https://github.com/spindle-studio/n8n-matrix-e2ee/issues/25)) ([8eebe80](https://github.com/spindle-studio/n8n-matrix-e2ee/commit/8eebe808dca01b290a1148ca95d57d00f7016c2c))
* **gateway:** support multiple Matrix accounts via TOML config ([#27](https://github.com/spindle-studio/n8n-matrix-e2ee/issues/27)) ([1deaebd](https://github.com/spindle-studio/n8n-matrix-e2ee/commit/1deaebd6904067766a0ed9c53c68efb04eb3afb2))

## [0.4.0](https://github.com/spindle-studio/n8n-matrix-e2ee/compare/gateway-v0.3.0...gateway-v0.4.0) (2026-10-21)


### ⚠ BREAKING CHANGES

* flattened camelCase API and event trigger modes ([#21](https://github.com/spindle-studio/n8n-matrix-e2ee/issues/21))
* add room management and stream room invitations ([#20](https://github.com/spindle-studio/n8n-matrix-e2ee/issues/20))
* support outgoing Matrix mentions and multipart media uploads ([#15](https://github.com/spindle-studio/n8n-matrix-e2ee/issues/15))
* support Markdown messages and simplify node parameters ([#11](https://github.com/spindle-studio/n8n-matrix-e2ee/issues/11))

### Features

* add room management and stream room invitations ([#20](https://github.com/spindle-studio/n8n-matrix-e2ee/issues/20)) ([59592e8](https://github.com/spindle-studio/n8n-matrix-e2ee/commit/59592e816150fe954ae2818a37780420ccce2011))
* flattened camelCase API and event trigger modes ([#21](https://github.com/spindle-studio/n8n-matrix-e2ee/issues/21)) ([cf5c2f9](https://github.com/spindle-studio/n8n-matrix-e2ee/commit/cf5c2f91c3fc1063a427b96b43bc310ea4f25775))
* support Markdown messages and simplify node parameters ([#11](https://github.com/spindle-studio/n8n-matrix-e2ee/issues/11)) ([aa81533](https://github.com/spindle-studio/n8n-matrix-e2ee/commit/aa81533ecc12cfc0adf39b22122cd52a83cc3810))
* support Matrix emote messages across gateway and nodes ([#17](https://github.com/spindle-studio/n8n-matrix-e2ee/issues/17)) ([f2cf9d1](https://github.com/spindle-studio/n8n-matrix-e2ee/commit/f2cf9d1b0591a0dc220cb74643938afddd4a5db0))
* support Matrix event redaction ([#19](https://github.com/spindle-studio/n8n-matrix-e2ee/issues/19)) ([8329cee](https://github.com/spindle-studio/n8n-matrix-e2ee/commit/8329cee13d60e1ce0cca629be17f9beba664a807))
* support Matrix message reactions ([#18](https://github.com/spindle-studio/n8n-matrix-e2ee/issues/18)) ([b4c90bc](https://github.com/spindle-studio/n8n-matrix-e2ee/commit/b4c90bc682f1832408284321412d775b080fcd1b))
* support Matrix notice messages across gateway and nodes ([#14](https://github.com/spindle-studio/n8n-matrix-e2ee/issues/14)) ([4a2a951](https://github.com/spindle-studio/n8n-matrix-e2ee/commit/4a2a951b41543a470b5ecccbdf217a0fa4325210))
* support Matrix threads, replies, and message edits ([#16](https://github.com/spindle-studio/n8n-matrix-e2ee/issues/16)) ([f05ce8f](https://github.com/spindle-studio/n8n-matrix-e2ee/commit/f05ce8f9ad01445e9f61508109ceaeb9e666bab0))
* support outgoing Matrix mentions and multipart media uploads ([#15](https://github.com/spindle-studio/n8n-matrix-e2ee/issues/15)) ([8966904](https://github.com/spindle-studio/n8n-matrix-e2ee/commit/8966904e60e91daa92e2c84dcfb1f7ef80673a5c))

## [0.3.0](https://github.com/spindle-studio/n8n-matrix-e2ee/compare/gateway-v0.2.0...gateway-v0.3.0) (2026-10-11)


### Features

* forward Matrix mentions through gateway and trigger ([f82cf16](https://github.com/spindle-studio/n8n-matrix-e2ee/commit/f82cf16c2af122ed0f2faf373c698926cac0be37))

## [0.2.0](https://github.com/spindle-studio/n8n-matrix-e2ee/compare/gateway-v0.1.0...gateway-v0.2.0) (2026-10-11)


### ⚠ BREAKING CHANGES

* **gateway:** authenticate native WebSocket clients with an initial message

### Features

* **gateway:** authenticate native WebSocket clients with an initial message ([5cfd755](https://github.com/spindle-studio/n8n-matrix-e2ee/commit/5cfd755640aac2f35e0486a0fae3d5d8f9ca2ab9))

## 0.1.0 (2026-10-10)


### Features

* initial release of Matrix E2EE nodes and gateway ([dbf123f](https://github.com/spindle-studio/n8n-matrix-e2ee/commit/dbf123f5e00dc6e47a2aab4f7ec470694defc173))


### Bug Fixes

* **gateway:** handle WebSocket pings and disconnects ([947ff6a](https://github.com/spindle-studio/n8n-matrix-e2ee/commit/947ff6a739b1358db3bf5ee0e4069679f8392abc))
