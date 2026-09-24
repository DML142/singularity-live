# Architecture decision records

Architecture decision records capture decisions whose context and consequences should
survive individual implementation tasks. Accepted records are immutable; a later decision
supersedes an earlier record rather than silently rewriting it.

Use [`template.md`](template.md) only when a decision materially changes an architectural
boundary, security posture, persistence strategy, or platform approach.

## Accepted decisions

- [0001 — Tauri desktop architecture](0001-tauri-desktop-architecture.md)
- [0002 — Provider-neutral application boundary](0002-provider-neutral-application-boundary.md)
- [0003 — OpenRouter and the manual-assistance trust boundary](0003-openrouter-manual-assistance-boundary.md)
