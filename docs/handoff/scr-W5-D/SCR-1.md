# SCR-1: Supply alert cadence and arbitrary rate/overflow windows

- Raised by: W5-D; Date: 2026-10-11
- Affects: AlertInputs and NowInputs contracts; Docs: 07 §6.1–6.4, wave-5-live W5-D
- Affected tasks: W5-D, W6-A

## Problem

AlertInputs supplies only 15m/1h mount rates and a 15m overflow count, but rules accept arbitrary positive windows. ScanStatus has no schedule/cadence, so `older_than = "auto"` cannot implement twice the effective cadence (with a minimum 24h cadence for windowed roots). Multiple hot-directory rules can request different windows, but a single hot_dirs_window input contains no rule/window identity. Missing phantom input is indistinguishable from a measured empty list in NowInputs. The referenced webhook redaction rule is absent from 07 §6 in this checkout.

## Proposal

For W6 integration, add rule-scoped observations carrying window duration, available/unavailable status, and already-resolved stale age. Alternatively extend AlertInputs with filesystem sample histories, overflow histories, hot entries keyed by window, and `(root, effective_cadence)` values. Add explicit collector availability to NowInputs. Specify webhook path redaction in 07 §6.3 and choose internal state_space hysteresis.

## Impact if accepted

W6-A must supply the extra facts; W5-D must evaluate arbitrary windows and auto staleness. Core DTOs need not change for the first alternative. Caller signatures require a coordinated revision.

## Interim

No frozen signatures changed. Unsupported rate/overflow windows and auto stale ages produce no observation, preventing false alerts. Callers must supply hot entries for the rule window (multiple distinct windows remain unsupported in one evaluate call). Webhook construction exposes redaction explicitly. Missing phantom values remain zero in the frozen report, with the caller responsible for adding collector degradation. Internal state_space fires immediately and resolves after ten minutes.

## Decision (coordinator)

Pending.
