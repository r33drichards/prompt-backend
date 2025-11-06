use vergen::EmitBuilder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Generate git version information
    EmitBuilder::builder()
        .all_git()
        .all_build()
        .emit()?;

    Ok(())
}
