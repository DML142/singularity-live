# Tauri desktop architecture

## Status

Accepted — 2026-09-21

## Context

Singularity Live needs a lightweight cross-platform desktop shell with a modern UI and
access to security-sensitive operating-system capabilities. Its primary production target
is Windows, while Linux and macOS must remain viable. The webview must not own provider
credentials, persistence, capture, or unrestricted host access.

## Decision

Use Tauri 2 with a React and TypeScript webview. React owns presentation and local UI
interaction. Rust owns operating-system integration, secrets, provider networking,
persistence, capture, and session orchestration. Communication crosses narrowly scoped,
typed Tauri commands. Capabilities begin empty and are expanded only for implemented work.

## Consequences

The application remains small and can share most UI and domain concepts across platforms.
Platform-specific implementations stay behind Rust adapters. Features that require native
packages have a higher development-environment setup cost, especially on Linux. IPC types
must be kept aligned across Rust and TypeScript until a justified code-generation approach
is adopted.
