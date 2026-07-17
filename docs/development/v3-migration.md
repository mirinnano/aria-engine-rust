# V1/V2 to Aria 3.1 migration

`aria migrate` is a one-way, all-or-nothing source migration. It creates a
backup before changing anything and does not add a C# interop layer.

```sh
cargo run --locked -p aria-cli -- migrate path/to/project
cargo run --locked -p aria-cli -- migrate path/to/project --game-id jp.example.game
```

The migrator:

1. copies scripts, `init.aria`, config, saves, and pak files into
   `.aria-migrate-backup/<timestamp>/`;
2. infers a schema-3 `aria.toml` and preserves existing project metadata;
3. converts the safe legacy subset to `aria 3.1;` scenes with explicit
   `await advance;` and explicit terminal control flow;
4. emits imports for additional scripts and validates the whole import graph;
5. compiles the staged source graph and checks every converted asset reference
   before touching the legacy files;
6. converts recognized save envelopes into `SaveEnvelopeV3` generations;
7. writes a JSON migration report.

Unsupported commands, malformed delimiters, unresolved labels, scene
fall-through, and missing assets are reported with source locations. If any
script cannot be converted, no source or manifest is written; the command
returns exit code 2. Unreadable saves are left untouched and counted in the
report. Running the command again on an already structured project revalidates
it instead of claiming a false conversion.

The converter intentionally does not guess semantics for old implicit waits,
dynamic labels, arbitrary register expressions, or host-only commands. Those
cases require an author decision in Aria 3.1. Keep the legacy project and its
tests outside this Rust-only repository; once a behavior is represented by a
platform-neutral source and replay fixture, the Native/Web determinism test
can make it part of the V3 green corpus.
