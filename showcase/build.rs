//! Compiles the views in `ui/`.

fn main() {
    let config = slint_build::CompilerConfiguration::new()
        // One style everywhere the showcase runs, so its tests lay out the same on every system.
        .with_style("fluent".into())
        // Tests find elements by their accessible labels, which needs the names Slint keeps only with its debug info. It is on in
        // every build, so tests pass in a release build too, at the cost of the names in the binary. An app that would rather not
        // ship them turns it on only for the builds its tests use.
        .with_debug_info(true);
    slint_build::compile_with_config("ui/app.slint", config).expect("the views compile");
}
