fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=../app/assets/icon/app.rc");
        println!("cargo:rerun-if-changed=../app/assets/icon/teams-fast.ico");
        embed_resource::compile(
            "../app/assets/icon/app.rc",
            embed_resource::ParamsIncludeDirs(&["../app/assets/icon"]),
        )
        .manifest_optional()
        .unwrap();
    }
}
