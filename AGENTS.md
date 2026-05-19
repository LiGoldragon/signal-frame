# signal-frame — Agent Instructions

This repo follows the primary workspace contract. Read:

1. `/home/li/primary/ESSENCE.md`
2. `/home/li/primary/lore/AGENTS.md`
3. `/home/li/primary/skills/rust-discipline.md`
4. `/home/li/primary/skills/contract-repo.md`
5. `ARCHITECTURE.md`
6. `skills.md`

`signal-frame` is the frame-mechanics contract repo, renamed from the
former `signal-core`. Keep runtime actors, reducers, stores, terminal
adapters, and CLI parsing out of this crate. The six Sema verbs
(`Assert` / `Mutate` / `Retract` / `Match` / `Subscribe` / `Validate`)
do **not** live here — they live in the sibling crate `signal-sema`.

See `reports/designer/238` and `reports/designer/239` in
`/home/li/primary/` for the architectural redirection that produced
this crate.
