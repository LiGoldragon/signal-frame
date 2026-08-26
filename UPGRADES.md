# Upgrades

## 0.4.0 — binary channel variants no longer require unique payload heads

`signal_channel!` now admits distinct operation, reply, or event variants that
share a payload type. This is required for contracts such as Orchestrate, where
both `Locked` and `Released` carry the complete `Lock` snapshot. The obsolete
Dotos record-head restriction is removed from binary channel generation.

The macro no longer emits `From<Payload>` for reply variants, because a payload
shared by two variants has no unambiguous conversion. Deploy the frame producer
before consumers. Consumers must construct the intended reply enum variant
explicitly, then regenerate and test their signal contract. No old implicit
conversion or Dotos dispatch fallback remains.
