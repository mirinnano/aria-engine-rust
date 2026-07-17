# Vendored build dependencies

`wayland-scanner` is copied from the published 0.31.10 crate and patched only
to use `quick-xml` 0.41.0. The published scanner still requests quick-xml
0.39.x, which is affected by the RustSec 2026 XML parser advisories. The
scanner parses trusted, compile-time Wayland protocol XML; keeping this small
patch local avoids a moving git dependency while retaining Wayland support.

When a crates.io release of wayland-scanner with the quick-xml 0.41 dependency
is available, remove this directory and the `[patch.crates-io]` entry in the
workspace manifest.
