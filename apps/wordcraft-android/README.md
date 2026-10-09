# WordCraft for Android

Native Android port of WordCraft built with **Rust + `eframe`/`egui` + Android NDK** (GameActivity).  
IME / Soft-Keyboard works via GameActivity. Full desktop UI on tablets — Ribbon, Rulers, Panes, Backstage, Status bar.

---

## Voraussetzungen

### 1. Android Studio installieren
Lade [Android Studio](https://developer.android.com/studio) herunter und installiere es.

Im **SDK Manager** (Zahnrad → Settings → Android SDK → SDK Tools) sicherstellen dass folgendes installiert ist:
- **Android SDK Platform 34** (oder höher)
- **NDK (Side by side)** — empfohlen: aktuelle Stable-Version
- **CMake**

### 2. Umgebungsvariablen setzen (PowerShell)

```powershell
# In PowerShell-Profil ($PROFILE) eintragen oder vor dem Build ausführen:
$env:ANDROID_HOME = "$env:LOCALAPPDATA\Android\Sdk"
$env:ANDROID_NDK_HOME = (Get-ChildItem "$env:ANDROID_HOME\ndk" | Sort-Object Name -Descending | Select-Object -First 1).FullName
```

### 3. Rust Android-Targets & `cargo-ndk`

```powershell
rustup target add aarch64-linux-android x86_64-linux-android
cargo install cargo-ndk
```

---

## Schritt 1: Native Libraries bauen

Aus dem **Workspace-Root** (`wordcraft/`) ausführen:

```powershell
# Debug (schneller, größere .so)
.\android\build-native.ps1

# Release (optimiert — für APK-Test empfohlen)
.\android\build-native.ps1 --release
```

Alternativ manuell:
```powershell
cargo ndk -t arm64-v8a -t x86_64 -o android/app/src/main/jniLibs build --release -p wordcraft-android
```

Die Dateien landen in:
```
android/app/src/main/jniLibs/
  arm64-v8a/libwordcraft.so   ← Tablets / Smartphones (ARM64)
  x86_64/libwordcraft.so      ← Emulator (x86_64)
```

---

## Schritt 2: In Android Studio öffnen

1. Android Studio starten
2. **File → Open…** → Ordner `android/` innerhalb des WordCraft-Repos auswählen
3. Gradle-Sync abwarten (Internet nötig für erstes `games-activity`-Dependency-Download)

---

## Schritt 3: APK bauen und installieren

### Debug-APK (direkt auf Gerät/Emulator):
```
Build → Build APK(s)
```
APK liegt dann unter:
```
android/app/build/outputs/apk/debug/app-debug.apk
```

### Auf angeschlossenem Tablet / Emulator ausführen:
```
Run → Run 'app'   (▶)
```

### Per Kommandozeile (aus `android/`):
```powershell
cd android
.\gradlew assembleDebug       # nur bauen
.\gradlew installDebug        # bauen + installieren
```

---

## Tablet-UI

Auf Tablets (≥ ~600 dp, Landscape) sieht die UI identisch zur Desktop-App aus:
- Vollständige Ribbon-Leiste (Start, Einfügen, Layout, Überprüfen …)
- Horizontale & vertikale Lineale
- Seiten-Canvas mit echtem Seitenumbruch
- Backstage (Datei-Menü), Panes, Statusleiste

Soft-Keyboard (IME) ist über **GameActivity** integriert und taucht beim Antippen des Textbereichs auf.

---

## Datei öffnen / speichern (SAF)

Open/Save nutzt Androids **Storage Access Framework** — der Datei-Picker des Systems erscheint.  
Unterstützte Formate: `.docx`, `.odt`, `.rtf`, `.txt`, `.md`, `.html` (öffnen/lesen); `.docx` (speichern).  
Dateien die per Share-Intent an WordCraft gesendet werden (z. B. aus Files-App) werden direkt geöffnet.

---

## Rechtliches (CLAUDE.md)

- Launcher-Icon: Original-Asset aus `assets/app-icon/` — Open Source, in `ATTRIBUTION.md` verzeichnet.
- Kein Microsoft-/Office-Branding, keine proprietären Schriften.
- Package-ID: `ai.storyteller.wordcraft`, App-Label: **WordCraft**.
- `.so`-Dateien sind gitignored (`android/app/src/main/jniLibs/**/*.so`), werden per Build erzeugt.

---

## Nicht in diesem Milestone

| Feature | Notiz |
|---|---|
| Phone Compact-UI | Desktop-Ribbon auch auf kleinen Screens (kein Collapse) |
| Play Store / AAB / Signing | Debug-APK reicht; Release-Keystore separat dokumentieren |
| Control-Channel / MCP auf Gerät | Desktop/CLI bleiben unberührt |
| Drucken | Android Print Framework — späterer Milestone |
