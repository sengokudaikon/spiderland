//! The windows, in Slint's own language, in `ui/`: `library.slint` and `editor.slint`, built on Slint's standard widgets and laid
//! out by Slint. `build.rs` compiles them, and this module brings in what Rust reaches of them: the two windows and their rows.
//! Every control a test uses has an accessible label, by which the test finds it.

#![allow(missing_docs, missing_debug_implementations, reason = "Slint makes the views' types public, with no docs or `Debug`")]

slint::include_modules!();
