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

// ─────────────── soft-keyboard conduit (GameTextInput → egui) ──────────────
//
// winit's Android backend forwards hardware/IME *key* events (which is why Delete
// works) but never reads GameActivity's GameTextInput buffer, where soft keyboards
// put committed and composing text (`commitText`/`setComposingText`). Typed letters
// therefore never became egui events. This conduit polls that buffer every frame
// and translates changes into the same `egui::Event::Ime` events a desktop IME
// integration produces, which the canvas already handles (`keys::canvas_events`).
//
// The buffer is treated as a *conduit*: committed text is emitted exactly once and
// the buffer is then cleared, so the next keystrokes always start from empty and
// the buffer cannot grow without bound. This logic is platform-independent on
// purpose, so it is unit-tested on the host (see the tests at the bottom).

/// One snapshot of the GameTextInput buffer. Span offsets are UTF-16 code units,
/// like Java `String` indices.
#[derive(Clone, Debug, Default)]
pub struct ImeBuffer {
    pub text: String,
    /// Composing span (start inclusive, end exclusive), or `None` when nothing is
    /// being composed.
    pub compose: Option<(usize, usize)>,
}

/// Slice `text` by UTF-16 code-unit offsets, clamped and snapped to char
/// boundaries. Never panics: empty, out-of-range or inverted spans yield `""`.
fn slice_utf16(text: &str, start: usize, end: usize) -> &str {
    if start >= end || text.is_empty() {
        return "";
    }
    let mut byte_start: Option<usize> = None;
    let mut byte_end = text.len();
    let mut u16 = 0usize;
    for (b, c) in text.char_indices() {
        let next = u16.saturating_add(c.len_utf16());
        if byte_start.is_none() && next > start {
            byte_start = Some(b);
        }
        if u16 >= end {
            byte_end = b;
            break;
        }
        u16 = next;
    }
    let Some(s) = byte_start else { return "" };
    if s >= byte_end {
        return "";
    }
    text.get(s..byte_end).unwrap_or("")
}

/// Leading `chars()` shared by both strings.
fn common_char_prefix(a: &str, b: &str) -> usize {
    a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count()
}

fn backspace_pair() -> [egui::Event; 2] {
    let down = egui::Event::Key {
        key: egui::Key::Backspace,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::default(),
    };
    let up = egui::Event::Key {
        key: egui::Key::Backspace,
        physical_key: None,
        pressed: false,
        repeat: false,
        modifiers: egui::Modifiers::default(),
    };
    [down, up]
}

/// Turns GameTextInput buffer snapshots into egui IME events, emitting every
/// change exactly once. See the module docs for the conduit protocol.
#[derive(Debug, Default)]
pub struct ImeConduit {
    /// Buffer text at the previous poll.
    last_text: String,
    /// Composing text reported at the previous poll (what the document shows inline).
    last_preedit: String,
    /// Committed buffer prefix already emitted to the document.
    committed: String,
    /// Set after we asked GameTextInput to clear; the next poll may still show the
    /// old text until the clear propagates, which must not be re-emitted.
    awaiting_clear: bool,
}

impl ImeConduit {
    /// Diff one buffer snapshot. Returns the egui events to inject plus whether the
    /// caller should reset the GameTextInput buffer to empty afterwards.
    pub fn poll(&mut self, buf: &ImeBuffer) -> (Vec<egui::Event>, bool) {
        let mut events = Vec::new();
        let mut clear = false;
        let composing = buf.compose.map(|(s, e)| slice_utf16(&buf.text, s, e)).unwrap_or("").to_string();
        // 1. Composing region changed → Preedit. The canvas replaces the old inline
        //    composition, so typing, backspace-in-compose, autocorrect rewrites and
        //    composition cancels all work through this one path.
        let had_live_preedit = !self.last_preedit.is_empty();
        if composing != self.last_preedit {
            self.last_preedit = composing.clone();
            events.push(egui::Event::Ime(egui::ImeEvent::Preedit { text: composing.clone(), active_range_chars: None }));
        }
        // 2. Committed text (nothing being composed).
        if composing.is_empty() && !buf.text.is_empty() && !(self.awaiting_clear && buf.text == self.last_text) {
            if had_live_preedit {
                // The commit finalizes previously composed text: emit it whole. The
                // Preedit("") above already removed the inline composition, and a
                // suffix would lose the composed prefix.
                events.push(egui::Event::Ime(egui::ImeEvent::Commit(buf.text.clone())));
                self.committed = buf.text.clone();
            } else if let Some(suffix) = buf.text.strip_prefix(&self.committed) {
                if !suffix.is_empty() {
                    // Pure append (fast direct commits without a composing phase).
                    events.push(egui::Event::Ime(egui::ImeEvent::Commit(suffix.to_string())));
                    self.committed = buf.text.clone();
                }
            } else if self.committed.starts_with(&buf.text) {
                // The IME shortened the buffer (rare; backspace normally arrives as a
                // key event): mirror with one backspace per removed char.
                let removed = self.committed.get(buf.text.len()..).map(|t| t.chars().count()).unwrap_or(0);
                for _ in 0..removed {
                    events.extend(backspace_pair());
                }
                self.committed = buf.text.clone();
            } else {
                // The IME rewrote committed text (e.g. autocorrect replaced a word):
                // delete back to the common prefix, then insert the new tail.
                let common = common_char_prefix(&self.committed, &buf.text);
                for _ in common..self.committed.chars().count() {
                    events.extend(backspace_pair());
                }
                let tail: String = buf.text.chars().skip(common).collect();
                if !tail.is_empty() {
                    events.push(egui::Event::Ime(egui::ImeEvent::Commit(tail)));
                }
                self.committed = buf.text.clone();
            }
            // The buffer is a conduit: reset it so later keystrokes start fresh.
            self.last_preedit.clear();
            clear = true;
            self.awaiting_clear = true;
        }
        if buf.text.is_empty() {
            self.awaiting_clear = false;
            self.committed.clear();
        }
        self.last_text = buf.text.clone();
        (events, clear)
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
        // to show or hide the soft keyboard accordingly. Then poll GameTextInput for
        // soft-keyboard text, which winit never forwards on its own.
        #[cfg(target_os = "android")]
        {
            let wants_ime = ctx.output(|o| o.ime.is_some());
            bridge::sync_keyboard(ctx, wants_ime);
            if wants_ime || ctx.egui_wants_keyboard_input() {
                bridge::poll_ime(raw);
            }
        }
        #[cfg(not(target_os = "android"))]
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
    use std::sync::{Mutex, OnceLock};

    use super::{ImeBuffer, ImeConduit};

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

    /// The `AndroidApp` handle (stored at startup): the only way to reach
    /// GameActivity's GameTextInput buffer from the render thread.
    static ANDROID_APP: OnceLock<Mutex<Option<android_activity::AndroidApp>>> = OnceLock::new();

    fn android_app_slot() -> &'static Mutex<Option<android_activity::AndroidApp>> {
        ANDROID_APP.get_or_init(|| Mutex::new(None))
    }

    pub fn store_android_app(app: &android_activity::AndroidApp) {
        *android_app_slot().lock().unwrap_or_else(|e| e.into_inner()) = Some(app.clone());
    }

    /// Conduit diff state across frames.
    static IME_CONDUIT: OnceLock<Mutex<ImeConduit>> = OnceLock::new();

    fn conduit_slot() -> &'static Mutex<ImeConduit> {
        IME_CONDUIT.get_or_init(|| Mutex::new(ImeConduit::default()))
    }

    /// Poll GameTextInput for soft-keyboard text and append the resulting egui
    /// events to `raw`. Call once per frame while an IME is wanted. winit never
    /// forwards this buffer on its own, so without this nothing typed on a soft
    /// keyboard reaches the app (only key events like Delete do).
    pub fn poll_ime(raw: &mut egui::RawInput) {
        let app = android_app_slot().lock().map(|g| g.clone()).unwrap_or(None);
        let Some(app) = app else { return };
        let state = app.text_input_state();
        let buf = ImeBuffer { text: state.text, compose: state.compose_region.map(|s| (s.start, s.end)) };
        let (events, clear) = conduit_slot().lock().map(|mut c| c.poll(&buf)).unwrap_or_default();
        if !events.is_empty() {
            log::debug!("ime: {} event(s) for buffer {buf:?}", events.len());
            raw.events.extend(events);
        }
        if clear {
            app.set_text_input_state(android_activity::input::TextInputState::default());
        }
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
    // Keep a handle: the per-frame IME poll reads GameTextInput through it.
    bridge::store_android_app(&app);
    static LOG_INIT: Once = Once::new();
    LOG_INIT.call_once(|| {
        android_logger::init_once(
            android_logger::Config::default()
                .with_max_level(log::LevelFilter::Info)
                .with_tag("WordCraft"),
        );
    });

    let mut options = eframe::NativeOptions {
        android_app: Some(app),
        viewport: egui::ViewportBuilder::default()
            .with_title("WordCraft")
            .with_app_id("ai.storyteller.wordcraft")
            .with_fullscreen(true),
        ..Default::default()
    };

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

#[cfg(test)]
mod tests {
    use super::*;

    fn buf(text: &str, compose: Option<(usize, usize)>) -> ImeBuffer {
        ImeBuffer { text: text.to_string(), compose }
    }

    fn preedit_text(e: &egui::Event) -> Option<&str> {
        if let egui::Event::Ime(egui::ImeEvent::Preedit { text, .. }) = e { Some(text) } else { None }
    }

    fn commit_text(e: &egui::Event) -> Option<&str> {
        if let egui::Event::Ime(egui::ImeEvent::Commit(text)) = e { Some(text) } else { None }
    }

    #[test]
    fn composing_typing_then_space_commits_once() {
        // Gboard sequence for "hi" + space: composing updates, then a commit.
        let mut c = ImeConduit::default();
        let (e, clear) = c.poll(&buf("h", Some((0, 1))));
        assert!(!clear);
        assert_eq!(e.iter().filter_map(preedit_text).collect::<Vec<_>>(), ["h"]);
        let (e, clear) = c.poll(&buf("hi", Some((0, 2))));
        assert!(!clear);
        assert_eq!(e.iter().filter_map(preedit_text).collect::<Vec<_>>(), ["hi"]);
        // Space commits: composition vanishes and the buffer holds "hi ".
        let (e, clear) = c.poll(&buf("hi ", None));
        assert!(clear, "committed text must reset the conduit buffer");
        // Same-frame Preedit("") clears the inline composition, then the commit lands.
        let texts: Vec<&str> = e.iter().filter_map(|x| preedit_text(x).or_else(|| commit_text(x))).collect();
        assert_eq!(texts, ["", "hi "]);
        // The clear propagates: the empty buffer emits nothing.
        let (e, clear) = c.poll(&buf("", None));
        assert!(!clear && e.is_empty());
        // …and the echo of the old text before the clear lands is swallowed.
        let mut c2 = ImeConduit::default();
        let (_, clear) = c2.poll(&buf("hi ", None));
        assert!(clear);
        let (e, _) = c2.poll(&buf("hi ", None));
        assert!(e.is_empty(), "stale buffer echo must not re-emit");
    }

    #[test]
    fn direct_commit_without_composing_phase() {
        let mut c = ImeConduit::default();
        let (e, clear) = c.poll(&buf("Hallo", None));
        assert!(clear);
        assert_eq!(e.iter().filter_map(commit_text).collect::<Vec<_>>(), ["Hallo"]);
    }

    #[test]
    fn composing_backspace_updates_preedit() {
        let mut c = ImeConduit::default();
        let (e, _) = c.poll(&buf("hi", Some((0, 2))));
        assert_eq!(e.iter().filter_map(preedit_text).collect::<Vec<_>>(), ["hi"]);
        let (e, clear) = c.poll(&buf("h", Some((0, 1))));
        assert!(!clear);
        assert_eq!(e.iter().filter_map(preedit_text).collect::<Vec<_>>(), ["h"]);
    }

    #[test]
    fn utf16_spans_cover_umlauts_and_emoji() {
        // "ä" is one UTF-16 unit but two UTF-8 bytes; "😀" is a surrogate pair (two units).
        assert_eq!(slice_utf16("äpfel", 0, 1), "ä");
        assert_eq!(slice_utf16("äpfel", 1, 5), "pfel");
        assert_eq!(slice_utf16("a😀b", 1, 3), "😀");
        assert_eq!(slice_utf16("a😀b", 0, 99), "a😀b");
        assert_eq!(slice_utf16("hi", 5, 9), "");
        assert_eq!(slice_utf16("hi", 2, 1), "");
        let mut c = ImeConduit::default();
        let (e, _) = c.poll(&buf("Bär", Some((0, 3))));
        assert_eq!(e.iter().filter_map(preedit_text).collect::<Vec<_>>(), ["Bär"]);
    }

    #[test]
    fn buffer_shrink_mirrors_backspaces() {
        let mut c = ImeConduit::default();
        let (_, _) = c.poll(&buf("hi", None));
        let (e, _) = c.poll(&buf("h", None));
        assert_eq!(e.len(), 2, "one backspace press + release, got {e:?}");
        assert!(matches!(&e[0], egui::Event::Key { key: egui::Key::Backspace, pressed: true, .. }));
    }

    #[test]
    fn rewritten_commit_deletes_to_common_prefix() {
        let mut c = ImeConduit::default();
        let (_, _) = c.poll(&buf("teh ", None));
        // Autocorrect rewrites the committed word in place.
        let (e, _) = c.poll(&buf("the ", None));
        let backs = e.iter().filter(|x| matches!(x, egui::Event::Key { key: egui::Key::Backspace, pressed: true, .. })).count();
        assert_eq!(backs, 3, "teh␣ → the␣ deletes back to the common prefix, got {e:?}");
        assert_eq!(e.iter().filter_map(commit_text).collect::<Vec<_>>(), ["he "]);
    }
}
