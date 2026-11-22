fn main() {
    println!("cargo:rerun-if-changed=assets/uac2-icon.ico");
    println!("cargo:rerun-if-changed=THIRD-PARTY-NOTICES.txt");
    let notices =
        std::fs::read_to_string("THIRD-PARTY-NOTICES.txt").expect("read distribution notices");
    let required = notices
        .lines()
        .filter(|line| line.starts_with("Required Notice:"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!required.is_empty(), "missing Required Notice lines");
    winresource::WindowsResource::new()
        .set_icon_with_id("assets/uac2-icon.ico", "101")
        .set("ProductName", "UAC2 Control")
        .set("LegalCopyright", &required)
        .set("Comments", "PolyForm Noncommercial License 1.0.0 — https://polyformproject.org/licenses/noncommercial/1.0.0")
        .set(
            "FileDescription",
            "Microsoft USB Audio 2.0 shared playback period control",
        )
        .set("OriginalFilename", "UAC2-Control.exe")
        .compile()
        .expect("compile embedded Windows resources");
    println!("cargo:rustc-link-arg-bin=UAC2-Control=/MANIFEST:EMBED");
    println!(
        "cargo:rustc-link-arg-bin=UAC2-Control=/MANIFESTUAC:level='asInvoker' uiAccess='false'"
    );
}
