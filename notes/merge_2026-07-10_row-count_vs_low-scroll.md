# Merge: default-row-count vs Low Scroll (2026-07-10)

Merging `623488f6` ("perf: cache Low Scroll row heights for large result sets")
into `mymain2` (which already contains `origin/default-row-count`) produces 12
conflicted files — 6 Rust, 6 insta snapshots.

## It is NOT a formatting conflict

First guess was that one side had rustfmt applied and the other hadn't.
Verified otherwise: running both merge stages through `rustfmt` and diffing
(`git show :2:$f | rustfmt` vs `git show :3:$f | rustfmt`) still shows real
semantic differences in every file.

The truth: two independent features landed on the same settings surfaces.

- **ours (`:2`, mymain2)** — Default Row Count + paged mode setting
  (`DefaultRowCount` enum variant, `default_row_count` / `paged_mode` config
  keys, pagination logic `can_paginate_at_end`).
- **theirs (`:3`)** — Low Scroll setting (`LowScroll` variant,
  `LowScrollSettings`, `low_scroll_*` config keys, variable row-height
  `max_scroll` in scroll.rs).

## Resolution strategy

Union merge: keep both features. Conflicts are almost entirely additive
(each side adds its own enum variant, struct fields, `Action::Settings*`
arms, config keys). Files:

- `src/app/model/shared/settings.rs`
- `src/app/ports/outbound/settings_store.rs`
- `src/app/update/browse/result/scroll.rs`  ← the one needing real thought:
  default-row-count's `can_paginate_at_end` and low-scroll's line-based
  `max_scroll` both rework the same function.
- `src/app/update/modal/settings.rs`
- `src/infra/adapters/settings_store.rs`
- `src/infra/config/connection_config.rs`

## Snapshot (.snap) conflicts

Never hand-merge these. The merged UI renders differently from either parent
anyway (settings overlay now shows both features). Resolve arbitrarily, then
regenerate once the code compiles:

```sh
notes/resolve_snap_conflicts.sh          # classify + auto-resolve snaps
cargo insta test --accept                # regenerate from merged code
```
