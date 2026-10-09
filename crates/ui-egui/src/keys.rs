//! Keyboard: typing, editing keys and command shortcuts (from the registry's `shortcut` fields).

use egui::{Key, Modifiers};
use serde_json::{Value, json};

use crate::WordApp;

/// The registry's name for a key.
pub fn key_name(k: Key) -> Option<&'static str> {
    Some(match k {
        Key::ArrowLeft => "Left",
        Key::ArrowRight => "Right",
        Key::ArrowUp => "Up",
        Key::ArrowDown => "Down",
        Key::Home => "Home",
        Key::End => "End",
        Key::PageUp => "PageUp",
        Key::PageDown => "PageDown",
        Key::Enter => "Enter",
        Key::Tab => "Tab",
        Key::Backspace => "Backspace",
        Key::Delete => "Delete",
        Key::Escape => "Escape",
        Key::Space => "Space",
        Key::Equals | Key::Plus => "=",
        Key::Minus => "-",
        Key::Period => ".",
        Key::Comma => ",",
        Key::OpenBracket => "[",
        Key::CloseBracket => "]",
        Key::Num0 => "0",
        Key::Num1 => "1",
        Key::Num2 => "2",
        Key::Num3 => "3",
        Key::Num4 => "4",
        Key::Num5 => "5",
        Key::Num6 => "6",
        Key::Num7 => "7",
        Key::Num8 => "8",
        Key::Num9 => "9",
        Key::F3 => "F3",
        Key::F5 => "F5",
        Key::F7 => "F7",
        Key::F9 => "F9",
        Key::F12 => "F12",
        Key::A => "A",
        Key::B => "B",
        Key::C => "C",
        Key::D => "D",
        Key::E => "E",
        Key::F => "F",
        Key::G => "G",
        Key::H => "H",
        Key::I => "I",
        Key::J => "J",
        Key::K => "K",
        Key::L => "L",
        Key::M => "M",
        Key::N => "N",
        Key::O => "O",
        Key::P => "P",
        Key::Q => "Q",
        Key::R => "R",
        Key::S => "S",
        Key::T => "T",
        Key::U => "U",
        Key::V => "V",
        Key::W => "W",
        Key::X => "X",
        Key::Y => "Y",
        Key::Z => "Z",
        _ => return None,
    })
}

fn combo(m: Modifiers, key: &str, with_shift: bool) -> String {
    let mut s = String::new();
    if m.command {
        s.push_str("Mod+");
    }
    if m.ctrl && !m.command {
        s.push_str("Ctrl+");
    }
    if m.mac_cmd && m.ctrl {
        s.push_str("Ctrl+");
    }
    if m.alt {
        s.push_str("Alt+");
    }
    if m.shift && with_shift {
        s.push_str("Shift+");
    }
    s.push_str(key);
    s
}

/// Find and run the command bound to this key. Returns true if handled.
fn dispatch(app: &mut WordApp, key: Key, m: Modifiers) -> bool {
    let Some(name) = key_name(key) else { return false };
    let reg = app.session.registry.clone();
    if let Some(spec) = reg.by_shortcut(&combo(m, name, true)) {
        let _ = app.run(spec.id, json!({}));
        return true;
    }
    // Shift extends caret movement.
    if m.shift
        && let Some(spec) = reg.by_shortcut(&combo(m, name, false))
        && spec.id.starts_with("caret.")
    {
        let _ = app.run(spec.id, json!({"extend": true}));
        return true;
    }
    false
}

fn key_to_char(key: Key, shift: bool) -> Option<char> {
    Some(match key {
        Key::Space => ' ',
        Key::Enter => '\n',
        Key::Tab => '\t',
        Key::A => if shift { 'A' } else { 'a' },
        Key::B => if shift { 'B' } else { 'b' },
        Key::C => if shift { 'C' } else { 'c' },
        Key::D => if shift { 'D' } else { 'd' },
        Key::E => if shift { 'E' } else { 'e' },
        Key::F => if shift { 'F' } else { 'f' },
        Key::G => if shift { 'G' } else { 'g' },
        Key::H => if shift { 'H' } else { 'h' },
        Key::I => if shift { 'I' } else { 'i' },
        Key::J => if shift { 'J' } else { 'j' },
        Key::K => if shift { 'K' } else { 'k' },
        Key::L => if shift { 'L' } else { 'l' },
        Key::M => if shift { 'M' } else { 'm' },
        Key::N => if shift { 'N' } else { 'n' },
        Key::O => if shift { 'O' } else { 'o' },
        Key::P => if shift { 'P' } else { 'p' },
        Key::Q => if shift { 'Q' } else { 'q' },
        Key::R => if shift { 'R' } else { 'r' },
        Key::S => if shift { 'S' } else { 's' },
        Key::T => if shift { 'T' } else { 't' },
        Key::U => if shift { 'U' } else { 'u' },
        Key::V => if shift { 'V' } else { 'v' },
        Key::W => if shift { 'W' } else { 'w' },
        Key::X => if shift { 'X' } else { 'x' },
        Key::Y => if shift { 'Y' } else { 'y' },
        Key::Z => if shift { 'Z' } else { 'z' },
        Key::Num0 => if shift { ')' } else { '0' },
        Key::Num1 => if shift { '!' } else { '1' },
        Key::Num2 => if shift { '@' } else { '2' },
        Key::Num3 => if shift { '#' } else { '3' },
        Key::Num4 => if shift { '$' } else { '4' },
        Key::Num5 => if shift { '%' } else { '5' },
        Key::Num6 => if shift { '^' } else { '6' },
        Key::Num7 => if shift { '&' } else { '7' },
        Key::Num8 => if shift { '*' } else { '8' },
        Key::Num9 => if shift { '(' } else { '9' },
        Key::Period => if shift { '>' } else { '.' },
        Key::Comma => if shift { '<' } else { ',' },
        Key::Equals | Key::Plus => if shift { '+' } else { '=' },
        Key::Minus => if shift { '_' } else { '-' },
        Key::OpenBracket => if shift { '{' } else { '[' },
        Key::CloseBracket => if shift { '}' } else { ']' },
        Key::Semicolon => if shift { ':' } else { ';' },
        Key::Quote => if shift { '"' } else { '\'' },
        Key::Slash => if shift { '?' } else { '/' },
        Key::Backslash => if shift { '|' } else { '\\' },
        Key::Backtick => if shift { '~' } else { '`' },
        _ => return None,
    })
}

fn handle_preedit_and_commit(app: &mut WordApp, new_text: &str, is_commit: bool) {
    let old_len = app.canvas.ime_preedit.chars().count();
    if old_len > 0 {
        for _ in 0..old_len {
            let _ = app.run("text.backspace", json!({}));
        }
    }
    if is_commit {
        app.canvas.ime_preedit.clear();
        if !new_text.is_empty() {
            insert_typed_lines(app, new_text);
        }
    } else {
        app.canvas.ime_preedit = new_text.to_string();
        if !new_text.is_empty() {
            let _ = app.run("text.insert", json!({"text": new_text}));
        }
    }
}

/// Insert committed text the way the user typed it: line breaks split paragraphs (like
/// Enter) instead of landing as literal control characters inside a paragraph.
fn insert_typed_lines(app: &mut WordApp, text: &str) {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut first = true;
    for chunk in normalized.split('\n') {
        if !first {
            let _ = app.run("text.newParagraph", json!({}));
        }
        first = false;
        if chunk == "\t" {
            let _ = app.run("text.tab", json!({}));
        } else if !chunk.is_empty() {
            let _ = app.run("text.insert", json!({"text": chunk}));
        }
    }
}

/// Events for the focused canvas: text, editing keys, clipboard, IME.
pub fn canvas_events(app: &mut WordApp, ctx: &egui::Context) {
    let events = ctx.input(|i| i.events.clone());
    for e in events {
        match e {
            egui::Event::Text(t) => {
                let m = ctx.input(|i| i.modifiers);
                if m.command || (m.ctrl && !cfg!(target_os = "macos")) {
                    continue;
                }
                handle_preedit_and_commit(app, &t, true);
            }
            egui::Event::Ime(egui::ImeEvent::Preedit { text, .. }) => {
                handle_preedit_and_commit(app, &text, false);
            }
            egui::Event::Ime(egui::ImeEvent::Commit(text)) => {
                handle_preedit_and_commit(app, &text, true);
            }
            egui::Event::Paste(t) => {
                let id = if ctx.input(|i| i.modifiers.shift && i.modifiers.alt) { "edit.pasteText" } else { "edit.paste" };
                let _ = app.run(id, json!({"text": t}));
            }
            egui::Event::Copy | egui::Event::Cut => {
                let id = if matches!(e, egui::Event::Cut) { "edit.cut" } else { "edit.copy" };
                if let Ok(r) = app.run(id, json!({}))
                    && let Some(t) = r.get("text").and_then(Value::as_str)
                    && !t.is_empty()
                {
                    ctx.copy_text(t.to_string());
                }
            }
            egui::Event::Key { key, pressed: true, modifiers, .. } => {
                if key == Key::Escape && app.session.painter.is_some() {
                    app.session.painter = None;
                    continue;
                }
                if key == Key::Escape && matches!(app.session.sel.focus.story, wordcraft_doc::StoryRef::Part(_)) && app.session.sel.is_collapsed() {
                    let _ = app.run("insert.closeHeader", json!({}));
                    continue;
                }
                if !dispatch(app, key, modifiers)
                    && !modifiers.command
                    && !modifiers.ctrl
                    && !modifiers.alt
                    && let Some(ch) = key_to_char(key, modifiers.shift)
                {
                    let _ = app.run("text.insert", json!({"text": ch.to_string()}));
                }
            }
            _ => {}
        }
    }
}

/// Command shortcuts when the canvas isn't focused but no text field is either.
pub fn global_shortcuts(app: &mut WordApp, ctx: &egui::Context) {
    if app.canvas.focused || ctx.egui_wants_keyboard_input() {
        return;
    }
    let events = ctx.input(|i| i.events.clone());
    for e in events {
        if let egui::Event::Key { key, pressed: true, modifiers, .. } = e
            && (modifiers.command || matches!(key, Key::F3 | Key::F5 | Key::F7 | Key::F9 | Key::F12))
        {
            dispatch(app, key, modifiers);
        }
    }
}
