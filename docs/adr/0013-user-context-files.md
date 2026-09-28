# Persistent user context files

## Status

Accepted — 2026-09-28

## Context

Creators need project or style notes that accompany every assistant request. Existing context
packs use keyword relevance to avoid sending unrelated documents, so they do not cover notes
that must always be considered. File access and persistence must remain on the Rust side of
the webview boundary.

## Decision

- Add a Settings → Context area that asks Rust to open a native multi-file picker filtered to
  `.md` and `.txt`, lists imported file names, and removes documents by opaque ID.
- Rust validates regular UTF-8 files and stores their contents in a versioned
  `user-context.json` file under Tauri application data. The webview receives names and IDs,
  never file contents or host paths.
- Keep at most 8 files and 12 KiB of combined UTF-8 content. Reject unsupported extensions,
  empty files, invalid UTF-8, and oversized content.
- Prepend every saved user document to the selected static context for every manual text or
  screenshot request. Preserve the existing context-pack selector and prompt-size bound;
  user documents fit before context-pack documents are added.
- Keep imported content out of logs and include it only in requests sent through the existing
  Rust provider router.

## Alternatives considered

1. Ask React to read file paths or contents. This would move native file access and context
   handling into the untrusted webview.
2. Add user files to the keyword-selected context pack. That would make always-included notes
   depend on keyword matching and mix user preferences with pack-managed documents.
3. Copy source files to unmanaged paths. This would leave context availability dependent on
   later renames, moves, and deletions outside the application.

## Consequences

Imported files remain available after restarts and their contents are sent with every manual
text or screenshot request. Users can remove files from Settings. The fixed 12 KiB combined
limit ensures these files fit in the bounded system prompt, but leaves less prompt space for
keyword-selected context-pack documents.
