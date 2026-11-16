# RenameX

**A batch-rename *planning* engine for Orbit — it never touches your files.**

> Orbit companion module — phase 2/4 of the [Orbit ecosystem](https://github.com/Can-5/orbit).
> Standalone crate: usable without Orbit, without other modules, and without network.

## Implemented now

- RenameRules: token pattern ({name} {counter} {date} {ext}), regex find/replace pass, zero-padded counter
- build_plan() — dry-run rename plan with warnings; would_overwrite() and detect_duplicate_newnames() refuse unsafe plans
- Case/in-place rename detection, lossy-case warnings, civil-from-days date formatting

## Not implemented yet (explicitly out of scope for this crate)

- No filesystem writes — application is host-side, per-file, only after explicit confirmation
- No EXIF/metadata-based naming (future module)

## Build & test

```sh
cargo test  # 10 tests
```

Dependencies: serde, serde_json, thiserror, regex. No cross-repo path dependencies, no Git dependencies, no downloads at build time beyond crates.io.

## Privacy

Read-only by design. Nothing is renamed without your explicit confirmation in the host.

## License

MIT © The Orbit contributors. See [LICENSE](./LICENSE).
