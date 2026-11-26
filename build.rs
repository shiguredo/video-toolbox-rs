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
        // 定義は実際のバインディングと同じ形 (ポインタ型・型エイリアス・列挙値) に揃える。
        // 空の構造体で代用すると、実際のバインディングでは満たされる性質 (Copy など) が
        // 失われてドキュメント生成が失敗する。
        //
        // See also: https://docs.rs/about/builds
        std::fs::write(
            output_bindings_path,
            concat!(
                "pub type CFIndex = ::std::os::raw::c_long;",
                "pub type CFTypeRef = *const ::std::ffi::c_void;",
                "pub type CFTypeID = ::std::os::raw::c_ulong;",
                "pub type CFStringRef = *const ::std::ffi::c_void;",
                "pub type CFArrayRef = *const ::std::ffi::c_void;",
                "pub type CFDictionaryRef = *const ::std::ffi::c_void;",
                "pub type CFBooleanRef = *const ::std::ffi::c_void;",
                "pub type CFAllocatorRef = *const ::std::ffi::c_void;",
                "pub type Boolean = ::std::os::raw::c_uchar;",
                "pub type UInt32 = ::std::os::raw::c_uint;",
                "pub struct __CVBuffer {",
                "    _unused: [u8; 0],",
                "}",
                "pub struct OpaqueCMBlockBuffer {",
                "    _unused: [u8; 0],",
                "}",
                "pub struct opaqueCMSampleBuffer {",
                "    _unused: [u8; 0],",
                "}",
                "pub type CVBufferRef = *mut __CVBuffer;",
                "pub type CVImageBufferRef = CVBufferRef;",
                "pub type CVPixelBufferRef = CVImageBufferRef;",
                "pub type CMItemCount = CFIndex;",
                "pub type CMTimeValue = i64;",
                "pub type CMTimeScale = i32;",
                "pub type CMTimeFlags = u32;",
                "pub type CMTimeEpoch = i64;",
                "pub type CMBlockBufferRef = *mut OpaqueCMBlockBuffer;",
                "pub type CMFormatDescriptionRef = *const ::std::ffi::c_void;",
                "pub type CMVideoFormatDescriptionRef = CMFormatDescriptionRef;",
                "pub type CMSampleBufferRef = *mut opaqueCMSampleBuffer;",
                "pub type VTDecodeInfoFlags = UInt32;",
                "pub type VTEncodeInfoFlags = UInt32;",
                "pub type VTDecompressionSessionRef = *mut ::std::ffi::c_void;",
                "pub type VTCompressionSessionRef = *mut ::std::ffi::c_void;",
                "pub type VTCompressionSessionCreate = *mut ::std::ffi::c_void;",
                "pub const kCMTimeFlags_Valid: CMTimeFlags = 1;",
                "pub const kCMTimeFlags_ImpliedValueFlagsMask: CMTimeFlags = 28;",
                "#[repr(C)]",
                "#[derive(Debug, Clone, Copy, PartialEq)]",
                "pub struct CMTime {",
                "    pub value: CMTimeValue,",
                "    pub timescale: CMTimeScale,",
                "    pub flags: CMTimeFlags,",
                "    pub epoch: CMTimeEpoch,",
                "}",
                "#[repr(C)]",
                "#[derive(Debug, Clone, Copy)]",
                "pub struct CMBlockBufferCustomBlockSource {",
                "    pub version: u32,",
                "    pub AllocateBlock: ::std::option::Option<",
                "        unsafe extern \"C\" fn(",
                "            refcon: *mut ::std::ffi::c_void,",
                "            sizeInBytes: usize,",
                "        ) -> *mut ::std::ffi::c_void,",
                "    >,",
                "    pub FreeBlock: ::std::option::Option<",
                "        unsafe extern \"C\" fn(",
                "            refcon: *mut ::std::ffi::c_void,",
                "            doomedMemoryBlock: *mut ::std::ffi::c_void,",
                "            sizeInBytes: usize,",
                "        ),",
                "    >,",
                "    pub refCon: *mut ::std::ffi::c_void,",
                "}",
                "#[repr(C)]",
                "#[derive(Debug, Clone, Copy)]",
                "pub struct CMSampleTimingInfo {",
                "    pub duration: CMTime,",
                "    pub presentationTimeStamp: CMTime,",
                "    pub decodeTimeStamp: CMTime,",
                "}",
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
