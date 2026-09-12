fn main() {
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=assets/icons/App.ico");

        let mut resource = winres::WindowsResource::new();
        resource.set_icon("assets/icons/App.ico");
        resource
            .compile()
            .expect("failed to compile the Windows executable resources");
    }
}
