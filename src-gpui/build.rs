fn main() {
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rerun-if-changed=../src-tauri/icons/icon.ico");
        embed_resource::compile("app.rc", embed_resource::NONE)
            .manifest_required()
            .expect("failed to embed the StructSheet icon");
    }
}
