# Contributing

## Git naming

Git metadata describes the concrete capability or change being delivered. Roadmap phases
are planning concepts documented in `tech.md`; they are not release names or Git naming
conventions.

- Name branches after the work, such as `context-provider-core` or
  `fix/backend-status-validation`.
- Use conventional, outcome-focused commit subjects, such as
  `feat: stream manual assistant responses`.
- Write pull-request titles and descriptions around behavior, architecture, validation,
  and user impact.
- Do not include roadmap phase labels or numbering in branch names, commit subjects,
  pull-request titles, or pull-request descriptions.
- Do not include agent, model, or tool names in Git metadata.

Roadmap documentation may continue to use phases where they help communicate sequencing
and completion status.

## Before opening a pull request

Run the complete validation suite:

```bash
pnpm check
```

Keep each pull request focused, update documentation when behavior or architecture changes,
and include the validation performed in the pull-request description.
