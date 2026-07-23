# signal-frame — Agent Instructions

Read this repository's `INTENT.md`, `ARCHITECTURE.md`, and `skills.md` before
editing.

`signal-frame` is the frame-mechanics contract repo, renamed from the
former `signal-core`. Keep runtime actors, reducers, stores, terminal
adapters, and CLI parsing out of this crate. The six Sema verbs
(`Assert` / `Mutate` / `Retract` / `Match` / `Subscribe` / `Validate`)
do **not** live here — they live in the sibling crate `signal-sema`.

This repository is under fast development and constantly breaking.
