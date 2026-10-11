# SCR-2: Resolve historical query targets that no longer exist live

- Raised by: W3-A    Date: 2026-10-11
- Affects: CLI path resolution; Docs: 08-interfaces §1.1; wave-3 W3-A
- Affected tasks: W3-A

## Problem
Global CLI realpath requires an existing live path, while W3-A explicitly requires querying a target removed from the new snapshot. Such a path normally no longer exists live.

## Proposal
For diff only, on ENOENT canonicalize the longest existing ancestor, then append validated missing name bytes. Do not lexically normalize before realpath. Reject ambiguous missing paths whose unresolved suffix includes parent components rather than borrow another path's facts.

## Impact if accepted
Clarifies diff's historical-path semantics without adding a flag or changing core types. Existing live paths still use realpath directly.

## Interim
Implements this narrow fallback so removed targets are usable; existing symlink ancestors are resolved first. A regression tests `link/../gone` with a symlink to `real/deep`.

## Decision (coordinator)
Pending.
