use std::{path::PathBuf, process::Command};

fn main() {
    // build.rs が更新されたら、依存ライブラリを再ビルドする
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-env-changed=DOCS_RS");

    // 各種変数やビルドディレクトリのセットアップ
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("infallible"));
    let out_include_dir = out_dir.join("include/");
    let output_bindings_path = out_dir.join("bindings.rs");

    // macOS 以外ではビルドを停止する (docs.rs は Linux 上で動作するためスキップ)
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS is not set");
    if target_os != "macos" && std::env::var("DOCS_RS").is_err() {
        panic!("this crate only supports macOS (target_os = \"macos\")");
    }

    if std::env::var("DOCS_RS").is_ok() {
        // Docs.rs 向けのビルドでは Video Toolbox は参照できないので build.rs の処理はスキップして、
        // 代わりに、ドキュメント生成時に最低限必要な定義だけをダミーで出力している。
        //
        // See also: https://docs.rs/about/builds
        std::fs::write(
            output_bindings_path,
            concat!(
                "pub struct CFDictionaryRef;",
                "pub struct CFStringRef;",
                "pub struct CFArrayRef;",
                "pub struct CFIndex;",
                "pub struct __CVBuffer;",
                "pub struct CMTime;",
                "pub struct CVImageBufferRef;",
                "pub struct CVPixelBufferRef;",
                "pub struct VTDecodeInfoFlags;",
                "pub struct VTDecompressionSessionRef;",
                "pub struct CMVideoFormatDescriptionRef;",
                "pub struct CMSampleBufferRef;",
                "pub struct VTEncodeInfoFlags;",
                "pub struct VTCompressionSessionRef;",
                "pub struct VTCompressionSessionCreate;",
                "pub struct OpaqueCMBlockBuffer;",
                "pub struct opaqueCMSampleBuffer;",
            ),
        )
        .expect("write file error");
        return;
    }

    let _ = std::fs::remove_dir_all(&out_include_dir);
    std::fs::create_dir(&out_include_dir).expect("failed to create include directory");

    // Video Toolbox の SDK のパスを取得する
    let output = Command::new("xcrun")
        .arg("--show-sdk-path")
        .output()
        .expect("failed to execute `xcrun` command");
    if !output.status.success() {
        panic!(
            "xcrun --show-sdk-path failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let sdk_dir = PathBuf::from(
        String::from_utf8(output.stdout)
            .expect("invalid path")
            .trim(),
    );

    // bindgen が解釈可能な構成にヘッダファイルを配置し直す
    let frameworks = [
        "IOKit",
        "OpenGL",
        "CoreFoundation",
        "CoreMedia",
        "CoreGraphics",
        "CoreAudio",
        "CoreAudioTypes",
        "CoreVideo",
        "VideoToolbox",
    ];
    for framework in &frameworks {
        let framework_headers_dir = sdk_dir.join(format!(
            "System/Library/Frameworks/{framework}.framework/Versions/A/Headers/"
        ));
        std::os::unix::fs::symlink(framework_headers_dir, out_include_dir.join(framework))
            .expect("failed to create a symlink");
    }

    // バインディングを生成する
    bindgen::Builder::default()
        .clang_arg(format!("-I{}", out_include_dir.display()))
        .header(
            out_include_dir
                .join("VideoToolbox/VideoToolbox.h")
                .display()
                .to_string(),
        )
        // ターゲット判定がうまくいかないことがあるので、明示的に指定する
        // ちゃんとやるなら TargetConditionals.h をインクルードするようにした方がいいかもしれない
        .clang_arg("-DTARGET_OS_OSX=1")
        // Clang が memcpy / strlen 等を builtin 署名に差し替えると、bindgen が size_t を
        // c_ulong として出力し、Rust 1.98 以降の suspicious_runtime_symbol_definitions に引っかかる。
        // -fno-builtin で差し替えを止め、size_t → usize の正しい対応を保つ。
        .clang_arg("-fno-builtin")
        // Video Toolbox 側のコメントが誤ってテスト対象と認識されてしまいエラーとなることがあるので、
        // コメントは生成しないようにしている。
        .generate_comments(false)
        // 以下の構造体はビルド時にエラーになることがあって、Hisui では不要なのでブラックリストに登録する
        // "error[E0588]: packed type cannot transitively contain a `#[repr(align)]` type"
        .blocklist_type("HFSCatalogFolder")
        .blocklist_type("HFSPlusCatalogFolder")
        .blocklist_type("HFSCatalogFile")
        .blocklist_type("HFSPlusCatalogFile")
        .blocklist_type("FndrOpaqueInfo")
        .generate()
        .expect("failed to generate bindings")
        .write_to_file(output_bindings_path)
        .expect("failed to write bindings");

    println!("cargo::rustc-link-lib=framework=CoreFoundation");
    println!("cargo::rustc-link-lib=framework=CoreMedia");
    println!("cargo::rustc-link-lib=framework=CoreVideo");
    println!("cargo::rustc-link-lib=framework=VideoToolbox");
}
