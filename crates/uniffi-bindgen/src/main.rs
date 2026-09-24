//! Gerador dos bindings do `isper-mobile` (Kotlin agora, Swift depois).
//!
//! O app Android chama isto sozinho no build (tarefa
//! `generateUniffiBindings`); à mão:
//!
//! ```text
//! cargo run -p uniffi-bindgen -- generate \
//!     --library <libisper_mobile.so> --language kotlin --out-dir <pasta>
//! ```

fn main() {
    uniffi::uniffi_bindgen_main()
}
