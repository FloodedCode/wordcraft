//! WordCraft on Android via `eframe` and GameActivity (IME / soft keyboard).
//!
//! Provides native ARM64/x86_64 rendering with OpenGL ES (Backends::GL) for
//! maximum driver compatibility on real devices and emulators. Vulkan is
//! tried first only if overridden via the WGPU_BACKEND env var.
//!
//! Android integration points:
//! - File I/O via SAF (Storage Access Framework) — Kotlin ↔ Rust JNI bridge.
//! - Soft keyboard / IME: Rust requests show/hide via JNI; GameActivity's
//!   InputConnection delivers key and IME events to wgpu/egui.
//! - Share-to: plain text shared from other apps is pushed via `nativePushText`.
//! - Immersive fullscreen is configured in Kotlin; the egui viewport fills the
//!   full display including the display notch (shortEdges cutout mode).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use eframe::egui;
use std::sync::{Mutex, OnceLock};
use wordcraft_engine::Session;
use wordcraft_ui_egui::{Inbox, Services, WordApp};

// ─────────────────────────── shared state ──────────────────────────────────

/// Byte-level inbox for async SAF / share-to file data.
static INBOX: OnceLock<Inbox> = OnceLock::new();
/// egui context slot — written every frame so JNI bridge callbacks can request repaint.
static EGUI_CTX: OnceLock<Mutex<Option<egui::Context>>> = OnceLock::new();

fn inbox() -> Inbox {
    INBOX.get_or_init(Inbox::default).clone()
}

fn egui_ctx_slot() -> &'static Mutex<Option<egui::Context>> {
    EGUI_CTX.get_or_init(|| Mutex::new(None))
}

fn request_repaint() {
    if let Ok(guard) = egui_ctx_slot().lock()
        && let Some(ctx) = guard.as_ref()
    {
        ctx.request_repaint();
    }
}

// ─────────────────────────── eframe app wrapper ─────────────────────────────

pub struct AndroidWordApp(pub WordApp);

impl eframe::App for AndroidWordApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Keep the context slot fresh every frame so JNI callbacks can repaint.
        if let Ok(mut slot) = egui_ctx_slot().lock() {
            *slot = Some(ctx.clone());
        }
        // Optimize button touch targets and spacing for Android touchscreens.
        ctx.global_style_mut(|s| {
            if s.spacing.interact_size.y < 30.0 {
                s.spacing.interact_size = egui::vec2(36.0, 32.0);
                s.spacing.button_padding = egui::vec2(10.0, 6.0);
                s.spacing.item_spacing = egui::vec2(8.0, 6.0);
            }
        });
        self.0.logic(ctx);
        if self.0.quit_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.0.raw_input_hook(raw);
        // Synchronise IME keyboard visibility with Android: read the previous frame's
        // egui IME output (set by canvas.rs when the canvas is focused) and tell Kotlin
        // to show or hide the soft keyboard accordingly.
        #[cfg(target_os = "android")]
        {
            let wants_ime = ctx.output(|o| o.ime.is_some());
            bridge::sync_keyboard(ctx, wants_ime);
        }
        let _ = ctx;
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.0.ui(ui);
    }
}

// ──────────────────────────── app factory ───────────────────────────────────

pub fn create_android_app() -> AndroidWordApp {
    let doc = wordcraft_engine::sample::sample_document();
    let mut word_app = WordApp::new(Session::new(doc), android_services());
    // On tablets / large displays use the full desktop chrome (ribbon, rulers, status bar).
    word_app.integrated_titlebar = false;
    word_app.autosave = false;
    AndroidWordApp(word_app)
}

fn android_services() -> Services {
    Services {
        open_async: Some(Box::new(|purpose: &str| {
            #[cfg(target_os = "android")]
            bridge::request_open(purpose);
            #[cfg(not(target_os = "android"))]
            let _ = purpose;
        })),
        // Android SAF: suggest a file name; actual save goes through the `download` hook.
        pick_save: Some(Box::new(|name: &str| Some(name.to_string()))),
        download: Some(Box::new(|name: &str, bytes: &[u8]| {
            #[cfg(target_os = "android")]
            bridge::request_save(name, bytes);
            #[cfg(not(target_os = "android"))]
            {
                let _ = (name, bytes);
            }
        })),
        inbox: Some(inbox()),
        ..Default::default()
    }
}

// ──────────────────────── JNI entry points called from Kotlin ───────────────

/// Called from Kotlin (SAF result callback) when the user picked a file.
pub fn push_opened_file(name: String, bytes: Vec<u8>) {
    inbox().lock().unwrap_or_else(|e| e.into_inner()).push((name, bytes));
    request_repaint();
}

/// Called from Kotlin when text is shared from another app (ACTION_SEND).
pub fn push_shared_text(text: String) {
    // Convert the shared plain text into a document and open it in the inbox.
    let bytes = text.into_bytes();
    inbox().lock().unwrap_or_else(|e| e.into_inner()).push(("shared.txt".to_string(), bytes));
    request_repaint();
}

// ────────────────────────── Android JNI bridge ──────────────────────────────

#[cfg(target_os = "android")]
mod bridge {
    use egui::Context;
    use jni::objects::{JByteArray, JClass, JObject, JString, JValue};
    use jni::{JNIEnv, JavaVM};
    use std::sync::atomic::{AtomicBool, Ordering};

    /// Last known IME-wanted state — prevents redundant JNI calls every frame.
    static IME_SHOWN: AtomicBool = AtomicBool::new(false);

    fn with_activity<F>(f: F)
    where
        F: FnOnce(&mut JNIEnv<'_>, &JObject<'_>, &JClass<'_>) -> jni::errors::Result<()>,
    {
        let ctx = ndk_context::android_context();
        if ctx.vm().is_null() || ctx.context().is_null() {
            log::error!("android context not ready");
            return;
        }
        // SAFETY: VM pointer comes from the Android runtime via ndk-context.
        let Ok(vm) = (unsafe { JavaVM::from_raw(ctx.vm().cast()) }) else {
            log::error!("invalid JavaVM");
            return;
        };
        let Ok(mut env) = vm.attach_current_thread() else {
            log::error!("attach_current_thread failed");
            return;
        };
        // SAFETY: context pointer is the jobject of the GameActivity (MainActivity)
        let act_obj = unsafe { JObject::from_raw(ctx.context().cast()) };
        let Ok(act_class) = env.get_object_class(&act_obj) else {
            log::error!("failed to get MainActivity class");
            return;
        };
        if let Err(e) = f(&mut env, &act_obj, &act_class) {
            log::error!("JNI call failed: {e}");
            let _ = env.exception_clear();
        }
    }

    /// Tell Android to show or hide the soft keyboard, ONLY when state changes.
    pub fn sync_keyboard(_ctx: &Context, wants_ime: bool) {
        let was_shown = IME_SHOWN.swap(wants_ime, Ordering::Relaxed);
        if wants_ime == was_shown {
            return; // state did not change — do NOT spam JNI or restartInput every frame!
        }
        let method = if wants_ime { "requestShowKeyboard" } else { "requestHideKeyboard" };
        with_activity(|env, _obj, class| {
            let _ = env.call_static_method(class, method, "()V", &[]);
            Ok(())
        });
    }

    pub fn request_open(purpose: &str) {
        let purpose = purpose.to_string();
        with_activity(|env, _obj, class| {
            let jpurpose = env.new_string(&purpose)?;
            env.call_static_method(class, "requestOpen", "(Ljava/lang/String;)V", &[JValue::Object(&jpurpose)])?;
            Ok(())
        });
    }

    pub fn request_save(name: &str, bytes: &[u8]) {
        let name = name.to_string();
        let bytes = bytes.to_vec();
        with_activity(|env, _obj, class| {
            let jname = env.new_string(&name)?;
            let jbytes = env.byte_array_from_slice(&bytes)?;
            let jbytes_obj = jni::objects::JObject::from(jbytes);
            env.call_static_method(
                class,
                "requestSave",
                "(Ljava/lang/String;[B)V",
                &[JValue::Object(&jname), JValue::Object(&jbytes_obj)],
            )?;
            Ok(())
        });
    }

    // ─── JNI symbols callable from Kotlin ─────────────────────────────────────

    /// `MainActivity.nativeOnFileOpened(String name, byte[] bytes)`
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_ai_storyteller_wordcraft_MainActivity_nativeOnFileOpened<'local>(
        mut env: JNIEnv<'local>,
        _class: JClass<'local>,
        name: JString<'local>,
        bytes: JByteArray<'local>,
    ) {
        let Ok(jname) = env.get_string(&name) else {
            log::error!("nativeOnFileOpened: bad name");
            return;
        };
        let name: String = jname.into();
        let Ok(data) = env.convert_byte_array(&bytes) else {
            log::error!("nativeOnFileOpened: bad bytes");
            return;
        };
        super::push_opened_file(name, data);
    }

    /// `MainActivity.nativePushText(String text)` — plain text shared via ACTION_SEND.
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_ai_storyteller_wordcraft_MainActivity_nativePushText<'local>(
        mut env: JNIEnv<'local>,
        _class: JClass<'local>,
        text: JString<'local>,
    ) {
        let Ok(jtext) = env.get_string(&text) else {
            log::error!("nativePushText: bad string");
            return;
        };
        super::push_shared_text(jtext.into());
    }

    /// `MainActivity.nativeOnKeyboardVisibilityChanged(boolean visible)`
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_ai_storyteller_wordcraft_MainActivity_nativeOnKeyboardVisibilityChanged<'local>(
        _env: JNIEnv<'local>,
        _class: JClass<'local>,
        visible: jni::sys::jboolean,
    ) {
        IME_SHOWN.store(visible != 0, Ordering::Relaxed);
    }

    /// `MainActivity.nativeTriggerAutoSave()`
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_ai_storyteller_wordcraft_MainActivity_nativeTriggerAutoSave<'local>(
        _env: JNIEnv<'local>,
        _class: JClass<'local>,
    ) {
        with_activity(|env, _obj, class| {
            let empty: [u8; 0] = [];
            let jbytes = env.byte_array_from_slice(&empty)?;
            let jbytes_obj = jni::objects::JObject::from(jbytes);
            let _ = env.call_static_method(
                class,
                "requestAutoSave",
                "([B)V",
                &[JValue::Object(&jbytes_obj)],
            );
            Ok(())
        });
    }
}

// ──────────────────────────── android_main ──────────────────────────────────

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    use std::sync::Once;
    static LOG_INIT: Once = Once::new();
    LOG_INIT.call_once(|| {
        android_logger::init_once(
            android_logger::Config::default()
                .with_max_level(log::LevelFilter::Info)
                .with_tag("WordCraft"),
        );
    });

    let mut options = eframe::NativeOptions::default();
    options.android_app = Some(app);
    options.viewport = egui::ViewportBuilder::default()
        .with_title("WordCraft")
        .with_app_id("ai.storyteller.wordcraft")
        .with_fullscreen(true);

    // Prefer OpenGL ES for maximum compatibility (real Mali/Adreno/Tegra GPUs and emulators).
    // Vulkan is used only when WGPU_BACKEND=vulkan is set explicitly.
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup {
        create.instance_descriptor.backends =
            eframe::wgpu::Backends::from_env().unwrap_or(eframe::wgpu::Backends::GL);
    }

    let _ = eframe::run_native(
        "WordCraft",
        options,
        Box::new(|_cc| Ok(Box::new(create_android_app()))),
    );
}

// ──────────────────────────── host compile check ─────────────────────────────

/// Compile check on non-Android CI: the app factory and service wiring must build.
#[cfg(not(target_os = "android"))]
#[allow(dead_code)]
fn _ensure_factory_builds() {
    let _ = create_android_app();
}
