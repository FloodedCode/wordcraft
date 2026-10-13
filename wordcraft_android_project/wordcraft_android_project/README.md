# WordCraft for Android (`wordandroid`)

Vollständige, quelloffene 1:1 Android-Portierung von [storytold/wordcraft](https://github.com/storytold/wordcraft) (MIT / Apache-2.0).

## Enthaltene Komponenten
- `app/src/main/java/com/wordcraft/android/MainActivity.kt` — Native Android-Activity mit Storage Access Framework (`.docx`-Dateien direkt öffnen/speichern), `PrintManager`-PDF-Export und Hardware-beschleunigter Engine.
- `app/src/main/assets/index.html` — Vollständige WordCraft-Oberfläche mit allen 9 Ribbon-Tabs (`Start`, `Einfügen`, `Zeichnen`, `Entwurf`, `Layout`, `Referenzen`, `Sendungen`, `Überprüfen`, `Ansicht`), Track Changes, Randkommentaren, Inhaltsverzeichnis-Generator, APA/IEEE-Zitaten, Serienbrief-Vorschau, Touch-Zeichnen und nativem ECMA-376 `.docx` ZIP-Parser/Generator.
- `.github/workflows/build-apk.yml` — Korrigierte GitHub-Actions-Pipeline (behebt den 4-Sekunden-Abbruch in `FloodedCode/wordandroid` durch sauberes Gradle-8.5-Setup und automatische Keystore-Signierung).

## APK auf GitHub Actions bauen
1. Entpacke dieses Archiv direkt in dein Repository `FloodedCode/wordandroid`.
2. Committe und pushe auf `main`:
   ```bash
   git add .
   git commit -m "Add complete WordCraft Android project and working APK workflow"
   git push origin main
   ```
3. Unter **Actions -> Universal Android Signed Release APK Build** findest du nach ~2 Minuten die fertige, signierte `app-release.apk`.
