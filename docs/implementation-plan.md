# Delivery plan

Le Harness is delivered through eighteen sequential pull requests. Each PR is reviewed and merged before dependent work begins.

| ID | Scope | Status |
| --- | --- | --- |
| H00 | Scope, contracts, and repository foundation | Complete ([#1](https://github.com/artemidius/Le-Harness/pull/1)) |
| H01 | Le Rust integration and provider compatibility | Complete ([#2](https://github.com/artemidius/Le-Harness/pull/2); live gate open) |
| H02 | Session controller and bounded agent loop | Complete ([#3](https://github.com/artemidius/Le-Harness/pull/3)) |
| H03 | Repository discovery and read tools | Complete ([#4](https://github.com/artemidius/Le-Harness/pull/4)) |
| H04 | Permission policy and approval flow | Complete ([#5](https://github.com/artemidius/Le-Harness/pull/5)) |
| H05 | Command execution and isolation | Complete ([#6](https://github.com/artemidius/Le-Harness/pull/6)) |
| H06 | Conventional edit transactions | Complete ([#7](https://github.com/artemidius/Le-Harness/pull/7)) |
| H07 | Usable CLI and Git workflow | Complete ([#8](https://github.com/artemidius/Le-Harness/pull/8)) |
| H08 | Codestral FIM editing | Complete ([#9](https://github.com/artemidius/Le-Harness/pull/9)) |
| H09 | Evaluation harness and baseline report | Complete ([#10](https://github.com/artemidius/Le-Harness/pull/10)) |
| H10 | Context management and compaction | Complete ([#11](https://github.com/artemidius/Le-Harness/pull/11)) |
| H11 | Configurable cloud model routing | Planned |
| H12 | MCP client integration | Planned |
| H13 | Local inference adapter | Planned |
| H14 | Session resume and inspection | Planned |
| H15 | TUI | Planned |
| H16 | End-to-end hardening and documentation | Planned |
| H17 | Release candidate | Planned |

The release checklist links each row to its implementation PR and validation evidence. Authenticated service checks and hardware-dependent probes are recorded separately from deterministic tests.

