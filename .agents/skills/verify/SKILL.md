---
name: verify
description: 'Run the full check loop for bevy-data-explorer: format, build, test, and a headless smoke run that catches runtime-only failures. Use when: finishing a change, before committing, checking whether something actually works, after editing shaders or UI.'
---

# Verify

A clean build proves very little in this repo. Shader compilation, render
pipeline validation and queries that match nothing all fail at runtime, and
several real bugs here built cleanly and silently did nothing.

Run all four steps. Report what actually happened, including the counts.

## Step 1: Format and build

```sh
cargo fmt && cargo build 2>&1 | grep -E "^(error|warning)" -A 6
```

Treat warnings as failures. Dead code here usually means something is wired up
wrong — a system never registered, a component never inserted — rather than
merely unused.

## Step 2: Test

```sh
cargo test 2>&1 | grep -E "^(test result|failures:)" -A 5
```

## Step 3: Smoke run

The app opens a window and never exits on its own, so it must be run under a
timeout:

```sh
timeout 50 cargo run 2>&1 | grep -iE "panic|ERROR|Validation|ambigu" -A 6
```

Look for:
- `Validation Error` — a render pipeline was rejected, e.g. a vertex stride
  that is not a multiple of four, or a binding that collides with one Bevy
  already declares.
- `failed to process shader` — WGSL did not compile. The message names the line.
- `panicked` — usually a plugin added twice, or a missing resource.

Silence here is the pass condition.

## Step 4: Check the startup summary

The same run prints what each source opened with. Confirm the level counts,
point counts and node counts look like the dataset rather than zeros, which is
how a reader that parsed nothing presents itself.

## Reporting

Say what passed and what did not, with numbers: test counts, any warnings left,
and whether the smoke run was clean. Do not describe a change as working on the
strength of a build alone.
