# HobbyCAM user manual (mdBook)

View: `cargo install mdbook --locked`, then `mdbook serve docs --open` (live reload) or `mdbook build docs` (output in `docs/book/`, not committed).
Pages are in `docs/src/`; `cargo test` checks rule messages, shortcuts, links and images against the code (`app/src/docs_check.rs`).
Screenshots: `HOBBYCAM_SHOT_DIR=$PWD/docs/src/images cargo test -p hobbycam shot_docs -- --ignored` from the repository root
(an absolute path: cargo runs the test in `app/`), then look at every PNG and commit them.
Licence: GPL-3.0-or-later, like the code.
