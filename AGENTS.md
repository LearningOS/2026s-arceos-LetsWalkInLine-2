# Agent Notes

## Where to work
- Main code and build system are under `arceos/` (Rust workspace + `Makefile`). Run `cargo`/`make` there, not at repo root.
- Root `scripts/*.sh` are the grading entrypoints; CI uses `./scripts/total-test.sh` from repo root.
- `crates/kernel_guard` is wired into `arceos` via `[patch.crates-io]` in `arceos/Cargo.toml`.

## Environment assumptions
- Prefer running code/commands in the dev container (`docker compose exec dev bash`).
- Before execution, run `docker compose ps -q dev`; if empty, try `docker compose up -d` and re-check.
- If still unavailable, ask whether to continue on host/local, and do not run any code/commands until explicit user approval.
- Toolchain is pinned in `arceos/rust-toolchain.toml`: `nightly-2024-09-04` with cross targets.
- Grading/build scripts require Linux tooling (`bash`, `dd`, `mkfs.fat`, `mount`, `stat -c`, `sudo`) and QEMU. On Windows, use WSL/container.
- `make disk_img` uses loop mount with `sudo`; containerized runs need privileged mode (`docker-compose.yml` already sets `privileged: true`).

## Verified commands
- Full local grading (same path CI scores): `./scripts/total-test.sh`.
- Single grading script: `./scripts/test-alt_alloc.sh` (or the matching `test-*.sh` script).
- Manual app run from `arceos/`: `make run A=exercises/<name>/ [BLK=y]`.
- Manual run order matters because `make run` defaults to `PFLASH=y`:
  1. `make pflash_img`
  2. `make disk_img`
  3. `make run A=...`

## Grading-sensitive gotchas
- `scripts/total-test.sh` must print numeric score on the last line; CI reads `tail -n1`.
- Scripts write failure details to `test.output`; CI prints this file.
- Preserve tested output strings:
  - `Hello, Arceos!` with ANSI color escapes (`test-print.sh`)
  - `[Ramfs-Rename]: ok!`
  - `Bump tests run OK!`
  - `Memory tests run OK!`
  - `Read back content: hello, arceos!`
  - `Shutdown vm normally!`

## Exercise-specific wiring
- `sys_map` and `simple_hv` need payload injection before run (handled by scripts): `make payload` + `./update_disk.sh ...`.
- `arceos/modules/bump_allocator/src/lib.rs` and `arceos/modules/alt_axalloc/src/lib.rs` are intentionally incomplete and are exercised by `exercises/alt_alloc`.
- `arceos/exercises/sys_map/src/syscall.rs` contains the `sys_mmap` stub used by `test-sys_map.sh`.
