//! On Windows: puts the icon and the product name into the .exe.

fn main() {
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/icon.ico");
        res.set("ProductName", "Light Studio");
        res.set("FileDescription", "Light Studio");
        res.set("LegalCopyright", "Luis Nieto");
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed the Windows icon: {e}");
        }
    }
}
