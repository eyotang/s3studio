fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=../src-tauri/icons/icon.ico");
    #[cfg(windows)]
    winresource::WindowsResource::new()
        .set_icon("../src-tauri/icons/icon.ico")
        .set("ProductName", "S3 Studio")
        .set("FileDescription", "S3 Studio")
        .compile()?;
    Ok(())
}
