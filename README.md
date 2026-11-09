# Typio Desktop

Cross-platform (macOS, Windows, Linux X11) desktop version of Typio: suggestions and shortcuts from your own datasets, in any app, stored only on your machine.

## Download

- macOS (Apple silicon and Intel): [Typio-macos.zip](https://github.com/vthang87/Typio-desktop/releases/latest/download/Typio-macos.zip)
- Windows 10/11: [Typio-windows.zip](https://github.com/vthang87/Typio-desktop/releases/latest/download/Typio-windows.zip)

The builds are not notarized by Apple or signed for Windows. On macOS, open System Settings → Privacy & Security → Open Anyway the first time; on Windows, choose More info → Run anyway.

License: [MIT](LICENSE).

## Layout

- `crates/typio-core`: Vietnamese normalization, matching and ranking, dataset and backup import. Ported from the Typio extension and kept behaviour-compatible with it.
