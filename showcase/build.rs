//! Compiles the views in `ui/`.

fn main() {
    let config = slint_build::CompilerConfiguration::new()
        // One style everywhere the showcase runs, so its tests lay out the same on every system.
        .with_style("fluent".into())
        // Tests find elements by their accessible labels, which needs the names Slint keeps only with its debug info. A release
        // build leaves them out.
        .with_debug_info(std::env::var("PROFILE").as_deref() != Ok("release"));
    slint_build::compile_with_config("ui/app.slint", config).expect("the views compile");
}
