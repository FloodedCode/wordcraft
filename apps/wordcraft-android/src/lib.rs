//! WordCraft on Android via `eframe` and GameActivity (IME / soft keyboard).
//!
//! Provides native ARM64/x86_64 rendering with wgpu (Vulkan / OpenGL ES),
//! maintaining full desktop UI feature parity on tablets and large displays.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use eframe::egui;
use std::sync::{Mutex, OnceLock};
use wordcraft_engine::Session;
use wordcraft_ui_egui::{Inbox, Services, WordApp};

/// Shared with the Kotlin GameActivity host for async SAF open results.
static INBOX: OnceLock<Inbox> = OnceLock::new();
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

pub struct AndroidWordApp(pub WordApp);

impl eframe::App for AndroidWordApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Ok(mut slot) = egui_ctx_slot().lock() {
            *slot = Some(ctx.clone());
        }
        self.0.logic(ctx);
        if self.0.quit_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.0.raw_input_hook(raw);
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.0.ui(ui);
    }
}

pub fn create_android_app() -> AndroidWordApp {
    let doc = wordcraft_engine::sample::sample_document();
    let mut word_app = WordApp::new(Session::new(doc), android_services());
    // Tablets use the full desktop ribbon / rulers / status bar (no integrated titlebar chrome).
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
        // Like the web host: a suggested name; `download` writes the bytes via SAF.
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

/// Called from Kotlin when the user picked a document or picture via SAF.
pub fn push_opened_file(name: String, bytes: Vec<u8>) {
    inbox().lock().unwrap_or_else(|e| e.into_inner()).push((name, bytes));
    request_repaint();
}

#[cfg(target_os = "android")]
mod bridge {
    use jni::objects::{JByteArray, JClass, JObject, JString, JValue};
    use jni::{JNIEnv, JavaVM};

    const ACTIVITY: &str = "ai/storyteller/wordcraft/MainActivity";

    fn with_env<F>(f: F)
    where
        F: FnOnce(&mut JNIEnv<'_>) -> jni::errors::Result<()>,
    {
        let ctx = ndk_context::android_context();
        if ctx.vm().is_null() || ctx.context().is_null() {
            log::error!("android context not ready for SAF");
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
        if let Err(e) = f(&mut env) {
            log::error!("JNI SAF call failed: {e}");
        }
    }

    pub fn request_open(purpose: &str) {
        let purpose = purpose.to_string();
        with_env(|env| {
            let class = env.find_class(ACTIVITY)?;
            let jpurpose = env.new_string(&purpose)?;
            env.call_static_method(class, "requestOpen", "(Ljava/lang/String;)V", &[JValue::Object(&jpurpose)])?;
            Ok(())
        });
    }

    pub fn request_save(name: &str, bytes: &[u8]) {
        let name = name.to_string();
        let bytes = bytes.to_vec();
        with_env(|env| {
            let class = env.find_class(ACTIVITY)?;
            let jname = env.new_string(&name)?;
            let jbytes = env.byte_array_from_slice(&bytes)?;
            let jbytes_obj = JObject::from(jbytes);
            env.call_static_method(
                class,
                "requestSave",
                "(Ljava/lang/String;[B)V",
                &[JValue::Object(&jname), JValue::Object(&jbytes_obj)],
            )?;
            Ok(())
        });
    }

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
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    use std::sync::Once;
    static LOG_INIT: Once = Once::new();
    LOG_INIT.call_once(|| {
        android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info));
    });

    let mut options = eframe::NativeOptions::default();
    options.android_app = Some(app);
    options.viewport = egui::ViewportBuilder::default()
        .with_title("WordCraft")
        .with_app_id("ai.storyteller.wordcraft")
        .with_fullscreen(true);

    if let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup {
        // On Android / Android Emulator, Vulkan drivers (e.g. Goldfish GFXStream/Mesa) often
        // crash inside driver indirect validation or create_bind_group_layout.
        // Backends::GL (OpenGL ES) is universally supported and rock-solid.
        create.instance_descriptor.backends = eframe::wgpu::Backends::from_env().unwrap_or(eframe::wgpu::Backends::GL);
    }

    let _ = eframe::run_native(
        "WordCraft",
        options,
        Box::new(|_cc| Ok(Box::new(create_android_app()))),
    );
}

/// Host-side compile check (non-Android CI): the app factory must build.
#[cfg(not(target_os = "android"))]
#[allow(dead_code)]
fn _ensure_factory_builds() {
    let _ = create_android_app();
}
