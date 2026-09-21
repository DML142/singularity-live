# Provider-neutral application boundary

## Status

Accepted — 2026-09-21

## Context

The product intends to use multiple speech, text, vision, and multimodal providers. Model
availability, pricing, request formats, and service quality change frequently. UI code and
session behavior must not be coupled to Groq, Gemini, OpenRouter, or any later vendor.

## Decision

Future provider adapters will implement provider-independent Rust capabilities. Application
services and the session orchestrator will use domain request and response types; adapters
alone translate those types into vendor DTOs. React calls application actions through IPC
and never calls providers. Provider selection, retries, timeouts, and fallbacks belong in a
Rust-side router implemented in the provider phase, not in Phase 0.

## Consequences

Changing providers will not require component changes, credentials stay out of the
webview, and fallback policy can remain consistent. The boundary adds mapping code inside
adapters and requires disciplined error classification. No speculative provider interfaces
or dependencies are added until Phase 1 needs them.
