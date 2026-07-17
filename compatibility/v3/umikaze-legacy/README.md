# umikaze legacy source snapshot

The `.aria` files in this directory are a byte-for-byte copy of the original
`src/AriaEngine` authoring sources, including `init.aria`, `main.aria`, the UI
helpers, and every localized `scenario_01` through `scenario_08` source.

They are retained as migration input and provenance. They are deliberately
outside `examples/umikaze`, so the V3 compiler cannot accidentally treat the
V1/V2 source as deployable Aria 3.1 input. The executable V3 opening and its
real umikaze art assets live in `examples/umikaze`.

Use `aria migrate` on a working copy of this tree only after resolving the
reported legacy commands and scene boundaries; migration never changes this
checked-in snapshot.
