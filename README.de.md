# S3 Studio

[简体中文](README.md) · [English](README.en.md) · [한국어](README.ko.md) · [日本語](README.ja.md) · [Français](README.fr.md) · [Deutsch](README.de.md) · [Русский](README.ru.md)

S3 Studio ist ein plattformübergreifender S3-Desktop-Client für die übersichtliche Verwaltung von Cloud-Dateien. Verbinden Sie AWS S3, MinIO oder andere S3-kompatible Dienste, verwalten Sie mehrere Konten und Buckets und laden Sie Dateien hoch oder herunter. Die Anzahl der Konten ist unbegrenzt; Zugangsdaten bleiben im Systemschlüsselbund.

![S3 Studio](artifacts/workspace-preview.png)

## Funktionen

- Mehrere Konten mit benutzerdefiniertem Endpoint, Region, pfadbasierter Adressierung und temporären Zugangsdaten.
- Ordnernavigation, Pfadleiste, Suche, Sortierung, Listen- und Rasteransicht, Mehrfachauswahl und Objektdetails.
- Upload, Download, Ordner und Buckets erstellen, Kopieren innerhalb eines Buckets, Mehrfachlöschung und signierte Freigabelinks.
- Übertragungsfortschritt und Geschwindigkeit, Pause, Fortsetzen, Wiederholen und Wiederaufnahme nach App-Neustarts.
- Gemeinsame Parallelitätsgrenze für alle Konten: standardmäßig 8, einstellbar von 1 bis 32.
- System-, helles und dunkles Design; Chinesisch, Englisch, Koreanisch, Japanisch, Französisch, Deutsch und Russisch.
- Zugangsdaten in macOS Keychain, Windows Credential Manager oder Linux Secret Service.

## Download und Installation

Laden Sie die aktuelle Version von [GitHub Releases](https://github.com/eyotang/s3studio/releases/latest) herunter. Prüfen Sie die Dateien vor der Installation anhand der zugehörigen `SHA256SUMS.txt`.

| Plattform                   | Paket            | Installation                                        |
| --------------------------- | ---------------- | --------------------------------------------------- |
| macOS Apple Silicon / Intel | Universal `.dmg` | S3 Studio in Programme ziehen                       |
| Windows x64                 | `.exe` / `.msi`  | Installationsprogramm starten                       |
| Ubuntu 22.04+ amd64         | `.deb`           | Mit der Systempaketinstallation öffnen              |
| Linux x86_64                | `.AppImage`      | Ausführbar machen und starten                       |
| Omarchy / Arch Linux x86_64 | `.pkg.tar.zst`   | Mit `sudo pacman -U` und dem Paketpfad installieren |

### Wenn macOS die App nicht öffnen kann

Die Installationspakete sind derzeit nicht mit einer System-Codesignatur oder Apple-Notarisierung versehen. Verschieben Sie die App nach Programme, prüfen Sie die offizielle Quelle und SHA-256 und führen Sie dann Folgendes aus:

```bash
sudo xattr -d com.apple.quarantine "/Applications/S3 Studio.app"
```

Starten und entsperren Sie unter Linux einen Secret-Service-Schlüsselbund wie GNOME Keyring. Das Dateiauswahl-Portal des Desktops muss verfügbar sein. Erlauben Sie die Ausführung der AppImage in den Dateieigenschaften. Omarchy-Pakete sind nicht in AUR; laden Sie neue Pakete herunter und aktualisieren Sie mit `pacman -U`. `sudo pacman -R s3-studio` entfernt die App, behält persönliche Einstellungen aber bei.

## Erste Schritte

### Konto verbinden

Wählen Sie Konto hinzufügen und geben Sie Name, Access Key ID, Secret Access Key und Region ein. Bei AWS ist Endpoint normalerweise nicht erforderlich; die Bucket-Region wird automatisch erkannt. Temporäre Zugangsdaten benötigen zusätzlich ein Session Token. MinIO-Beispiel: Endpoint `http://localhost:9000`, Region `us-east-1`, pfadbasierte Adressierung aktiviert. Verwenden Sie im Produktivbetrieb HTTPS.

Das Konto benötigt die passenden S3-Berechtigungen. Doppelklicken Sie Ordner zum Öffnen oder Dateien für Details. Wählen Sie Objekte zum Herunterladen, Kopieren, Teilen oder Löschen. Ordner werden nach Bestätigung mit ihrem gesamten Inhalt gelöscht. Uploads überschreiben gleichnamige Objekte. Freigabelinks gelten 1 Stunde und können durch ablaufende temporäre Zugangsdaten früher ungültig werden.

### Sprache, Design und Übertragungen

Öffnen Sie Einstellungen über die Seitenleiste oder unter macOS über S3 Studio → Einstellungen… (⌘,). Sprache und Design werden sofort angewendet und lokal gespeichert. Systemeinstellung erkennt unterstützte Systemsprachen und verwendet sonst Englisch. Stellen Sie die Parallelität auf 1–32 und speichern Sie. Die native macOS-Titelleiste folgt dem ausgewählten Design.

Unter Übertragungen können Sie pausieren, fortsetzen, wiederholen oder abbrechen. Abgeschlossene Teile bleiben beim Pausieren erhalten. Nach einem Neustart bleiben unvollständige Übertragungen bis zur manuellen Fortsetzung pausiert. Abbrechen bereinigt Prüfpunkte und temporäre Daten. Änderungen an Quelle, Remote-Objekt oder Ziel können eine unsichere Fortsetzung oder ein Überschreiben verhindern.

### Updates und Grenzen

Suchen Sie über das macOS-App-Menü oder die Versionsanzeige nach Updates. Veröffentlichte Versionen prüfen 20 Sekunden nach dem Start und danach alle 24 Stunden. Windows NSIS/MSI und Linux AppImage unterstützen geprüfte Updates in der App; pausieren oder beenden Sie zuvor alle Übertragungen. macOS und deb öffnen die Release-Seite zur manuellen Installation. Konten und Prüfpunkte bleiben erhalten.

Uploads sind auf 5 TiB je Datei begrenzt; serverseitiges Kopieren eines Objekts folgt der 5-GB-Grenze von S3 CopyObject. Löschen entfernt keinen Versionsverlauf. SSO/AssumeRole, ACL- und Richtlinienbearbeitung, Versionsverwaltung, kontenübergreifendes Kopieren, Ordnersynchronisierung, Glacier-Wiederherstellung, Ordnerdownloads und CloudFront-Verwaltung sind nicht enthalten. Die S3-Kompatibilität hängt von der Dienstimplementierung ab.

## Entwicklung und Beiträge

Die App nutzt GPUI, GPUI Kit, Rust und AWS SDK for Rust. `src-gpui` enthält die native Oberfläche, `src-core` das gemeinsame Backend und `src`/`src-tauri` die ursprüngliche React/Tauri-Referenz. Installieren Sie Node.js 22.12+ und stabiles Rust. macOS benötigt Xcode Command Line Tools; Windows benötigt MSVC C++ Build Tools und WebView2. Linux-Bibliotheken stehen in der [Entwicklungsreferenz](README.md#开发与贡献).

```bash
npm ci
npm run dev:native
```

`npm run preview:native` startet eine Desktop-Vorschau mit Beispieldaten. `npm run dev` startet die Browser-Referenz unter `http://127.0.0.1:1420`; sie verbindet keinen Cloud-Speicher und speichert keine Zugangsdaten.

Führen Sie die für Ihre Änderungen relevanten Prüfungen aus:

```bash
npm test
npm run test:ui
npm run build
cargo test --locked --manifest-path src-core/Cargo.toml
cargo test --locked --manifest-path src-gpui/Cargo.toml
```

Erstellen Sie die ausführbare Datei mit `npm run build:native` und Pakete mit `npm run tauri bundle -- --ci`, jeweils auf dem Zielbetriebssystem. Die [Entwicklungsreferenz](README.md#开发与贡献) beschreibt Linux-Abhängigkeiten, Universal-Builds, Datenpfade und Signaturen. Ändern Sie Übersetzungen in `src-core/locales`, führen Sie `node scripts/sync-i18n.mjs` aus, formatieren Sie Rust und testen Sie anschließend.

Geben Sie in Issues App-Version, Betriebssystem, Schritte zur Reproduktion und Fehler an. Entfernen Sie Zugangsdaten und private Informationen aus Logs und Screenshots. Pull Requests sollen Problem, resultierendes Verhalten und Validierung beschreiben; fügen Sie bei UI-Änderungen Screenshots hinzu. Tests verwenden einen lokalen S3-Simulator. Beachten Sie bei Veröffentlichungen [AGENTS.md](AGENTS.md): erst englische, dann chinesische Versionshinweise, beide mit der macOS-xattr-Anleitung.

Dieses unabhängig entwickelte Projekt ist kein offizielles AWS-Produkt. Siehe die [chinesische technische Referenz](README.md), [Versionshinweise](docs/releases) und [Markenressourcen](assets/brand).
