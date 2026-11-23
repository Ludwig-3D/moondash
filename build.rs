fn main() {
    slint_build::compile_with_config(
        "ui/moondash.slint",
        slint_build::CompilerConfiguration::new()
            .with_style("fluent".into())
            .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles),
    )
    .expect("compile Moondash UI");
}
