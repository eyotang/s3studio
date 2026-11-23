fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=icons/icon.ico");
    println!("cargo:rerun-if-changed=windows.manifest");
    #[cfg(windows)]
    winresource::WindowsResource::new()
        .set_icon("icons/icon.ico")
        .set_manifest(include_str!("windows.manifest"))
        .set("ProductName", "S3 Studio")
        .set("FileDescription", "S3 Studio")
        .compile()?;
    Ok(())
}
