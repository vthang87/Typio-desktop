//! English and Vietnamese text for everything the user sees. The log stays in English.
//!
//! `tr(T::X)` gives a fixed text; `trf(T::X, &[("name", value)])` fills `{name}`
//! placeholders. The language is set once at start-up and again when the user changes it.

use std::cell::Cell;

use serde::{Deserialize, Serialize};

/// The language the user picked in Settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    /// Vietnamese when it is the Mac's preferred language, English otherwise.
    #[default]
    System,
    English,
    Vietnamese,
}

impl Language {
    pub const ALL: [Language; 3] = [Language::System, Language::English, Language::Vietnamese];

    /// The name shown in the language menu; each language names itself.
    pub fn title(self) -> &'static str {
        match self {
            Language::System => tr(T::LanguageSystem),
            Language::English => "English",
            Language::Vietnamese => "Tiếng Việt",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lang {
    En,
    Vi,
}

thread_local! {
    static CURRENT: Cell<Lang> = const { Cell::new(Lang::En) };
}

/// Switches the text to `language`.
pub fn set(language: Language) {
    let lang = match language {
        Language::English => Lang::En,
        Language::Vietnamese => Lang::Vi,
        Language::System if system_prefers_vietnamese() => Lang::Vi,
        Language::System => Lang::En,
    };
    CURRENT.set(lang);
}

#[cfg(target_os = "macos")]
fn system_prefers_vietnamese() -> bool {
    objc2_foundation::NSLocale::preferredLanguages()
        .firstObject()
        .is_some_and(|language| language.to_string().starts_with("vi"))
}

#[cfg(windows)]
fn system_prefers_vietnamese() -> bool {
    // LANG_VIETNAMESE is 0x2a in the low ten bits of the language identifier.
    let language = unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() };
    language & 0x3ff == 0x2a
}

#[cfg(not(any(target_os = "macos", windows)))]
fn system_prefers_vietnamese() -> bool {
    false
}

pub fn tr(text: T) -> &'static str {
    match CURRENT.get() {
        Lang::En => text.en(),
        Lang::Vi => text.vi(),
    }
}

/// `tr` with each `{key}` replaced by its value.
pub fn trf(text: T, values: &[(&str, &str)]) -> String {
    values
        .iter()
        .fold(tr(text).to_string(), |text, (key, value)| {
            text.replace(&format!("{{{key}}}"), value)
        })
}

macro_rules! texts {
    ($($name:ident => $en:expr, $vi:expr;)*) => {
        /// Every text the user sees, in English and Vietnamese.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum T {
            $($name,)*
        }

        impl T {
            #[cfg(test)]
            const ALL: &'static [T] = &[$(T::$name,)*];

            fn en(self) -> &'static str {
                match self {
                    $(T::$name => $en,)*
                }
            }

            fn vi(self) -> &'static str {
                match self {
                    $(T::$name => $vi,)*
                }
            }
        }
    };
}

texts! {
    // Menu
    NeedsAccess => "Typio needs Accessibility access", "Typio cần quyền Accessibility";
    OpenAccessibilitySettings => "Open Accessibility Settings…", "Mở cài đặt Accessibility…";
    StatusOn => "Typio is on", "Typio đang bật";
    StatusOff => "Typio is off", "Typio đang tắt";
    EnableTypio => "Enable Typio", "Bật Typio";
    QuickSearch => "Quick Search", "Tìm nhanh";
    SkipApp => "Skip {name}", "Bỏ qua {name}";
    StopSkippingApp => "Stop Skipping {name}", "Thôi bỏ qua {name}";
    RunInApp => "Run in {name}", "Chạy trong {name}";
    StopRunningInApp => "Stop Running in {name}", "Ngừng chạy trong {name}";
    NoDatasets => "No datasets yet", "Chưa có bộ dữ liệu";
    ImportDataset => "Import Dataset…", "Nhập bộ dữ liệu…";
    SettingsItem => "Settings…", "Cài đặt…";
    Quit => "Quit Typio", "Thoát Typio";
    ChooseImportFiles => "Choose TXT, CSV or JSON files, or a Typio extension backup.",
        "Chọn file TXT, CSV, JSON hoặc bản sao lưu của extension Typio.";
    ImportBusy => "{name}: Typio is busy, try again.", "{name}: Typio đang bận, hãy thử lại.";
    ImportFinished => "Import finished", "Đã nhập xong";

    // Settings
    SettingsTitle => "Typio Settings", "Cài đặt Typio";
    TabGeneral => "General", "Chung";
    TabDatasets => "Datasets", "Bộ dữ liệu";
    TabApps => "Apps", "Ứng dụng";
    EnableEverywhere => "Enable Typio   ⌃⌥T", "Bật Typio   ⌃⌥T";
    TriggerCharacter => "Trigger character", "Ký tự kích hoạt";
    TriggerHint => "Type {trigger}xtt to see suggestions for xtt",
        "Gõ {trigger}xtt để xem gợi ý cho xtt";
    LanguageLabel => "Language", "Ngôn ngữ";
    LanguageSystem => "Same as system", "Theo hệ thống";
    QuickSearchHint => "Quick Search: press ⌃⌥Space in a text field to search every enabled dataset.",
        "Tìm nhanh: nhấn ⌃⌥Space trong ô nhập để tìm trong mọi bộ dữ liệu đang bật.";
    AccessGranted => "Accessibility access: granted", "Quyền Accessibility: đã cấp";
    AccessMissing => "Accessibility access: not granted", "Quyền Accessibility: chưa cấp";
    DatasetsHint => "Only checked datasets give suggestions.",
        "Chỉ bộ dữ liệu được đánh dấu mới đưa ra gợi ý.";
    NoDatasetsHelp => "No datasets yet. Click Import… to add a TXT, CSV or JSON file.",
        "Chưa có bộ dữ liệu. Bấm Nhập… để thêm file TXT, CSV hoặc JSON.";
    EditButton => "Edit…", "Sửa…";
    DeleteButton => "Delete", "Xoá";
    NewButton => "New…", "Tạo mới…";
    ImportButton => "Import…", "Nhập…";
    ExportBackup => "Export Backup…", "Xuất sao lưu…";
    OpenDataFolder => "Open Data Folder", "Mở thư mục dữ liệu";
    RunsIn => "Typio runs in", "Typio chạy trong";
    ModeAll => "All apps except skipped ones", "Mọi app, trừ app bỏ qua";
    ModeSelected => "Only the selected apps", "Chỉ các app đã chọn";
    SkippedAbout => "Skipped apps. Browsers start here so the Typio extension handles them. You can also skip the app in use from the Typio menu.",
        "App bị bỏ qua. Trình duyệt có sẵn ở đây để extension Typio lo phần gợi ý. Bạn cũng có thể bỏ qua app đang dùng từ menu Typio.";
    SkippedEmpty => "Typio runs in every app.", "Typio chạy trong mọi app.";
    SelectedAbout => "Selected apps. Typio runs only in these. You can also add the app in use from the Typio menu.",
        "App đã chọn. Typio chỉ chạy trong các app này. Bạn cũng có thể thêm app đang dùng từ menu Typio.";
    SelectedEmpty => "No apps selected yet, so Typio runs nowhere. Click Add App…",
        "Chưa chọn app nào nên Typio chưa chạy ở đâu. Bấm Thêm app…";
    RemoveButton => "Remove", "Gỡ";
    AddApp => "Add App…", "Thêm app…";
    ChooseApps => "Choose apps.", "Chọn app.";
    BackupFailed => "Typio could not save the backup", "Typio không lưu được bản sao lưu";
    DeleteTitle => "Delete “{name}”?", "Xoá “{name}”?";
    DeleteInfo => "Its values and shortcuts are removed from Typio. This cannot be undone.",
        "Các giá trị và gõ tắt của bộ dữ liệu này sẽ bị xoá khỏi Typio. Không thể hoàn tác.";
    CancelButton => "Cancel", "Huỷ";
    TabExtension => "Extension", "Extension";
    InstallExtension => "Install Chrome Extension…", "Cài extension Chrome…";
    ExtensionAbout => "In Chrome, Edge, Brave and Arc the Typio extension gives the suggestions, so Typio Desktop skips those browsers. Install the extension to use Typio there.",
        "Trong Chrome, Edge, Brave và Arc, extension Typio lo phần gợi ý nên Typio Desktop bỏ qua các trình duyệt này. Hãy cài extension để dùng Typio ở đó.";
    ExtensionStep1 => "Click Open Chrome Web Store, or open the Typio website.",
        "Bấm Mở Chrome Web Store, hoặc mở trang web Typio.";
    ExtensionStep2 => "Click Add to Chrome, then Add extension.",
        "Bấm Thêm vào Chrome, rồi chọn Thêm tiện ích.";
    ExtensionStep3 => "Click the puzzle icon in the toolbar and pin Typio.",
        "Bấm biểu tượng mảnh ghép trên thanh công cụ rồi ghim Typio.";
    ExtensionStep4 => "To use the same data, click Export Backup… here, then import that file in the extension's settings.",
        "Để dùng chung dữ liệu, bấm Xuất sao lưu… ở đây rồi nhập file đó trong phần cài đặt của extension.";
    OpenWebStore => "Open Chrome Web Store", "Mở Chrome Web Store";
    OpenWebsite => "Open Typio Website", "Mở trang web Typio";
    ShortcutCount => "{count} shortcuts", "{count} gõ tắt";
    ValueCount => "{count} values", "{count} giá trị";

    // Editor
    EditTitle => "Edit “{name}”", "Sửa “{name}”";
    NewDataset => "New Dataset", "Bộ dữ liệu mới";
    NameLabel => "Name", "Tên";
    TypeLabel => "Type", "Loại";
    KindShortcuts => "Shortcuts", "Gõ tắt";
    KindValues => "Values", "Giá trị";
    EditorHelp => "Shortcuts: one per line as trigger: text, for example xtt: Xã Tam Tiến.\nValues: one value per line.",
        "Gõ tắt: mỗi dòng một mục dạng trigger: nội dung, ví dụ xtt: Xã Tam Tiến.\nGiá trị: mỗi dòng một giá trị.";
    SaveButton => "Save", "Lưu";
    SaveHint => "⌘S saves", "⌘S để lưu";
    AndMore => "…and {count} more.", "…và {count} mục khác.";
    SavedWithWarnings => "Saved. Some triggers are used twice", "Đã lưu. Một số trigger bị dùng hai lần";
    CannotSaveDataset => "Typio could not save this dataset", "Typio không lưu được bộ dữ liệu này";
    Busy => "Typio is busy", "Typio đang bận";
    TrySavingAgain => "Try saving again.", "Hãy lưu lại lần nữa.";

    // Popup, Quick Search and notices
    InitialsHint => "initials", "chữ đầu";
    SearchPlaceholder => "Search values and shortcuts", "Tìm giá trị và gõ tắt";
    OffInField => "Typio is off in this field", "Typio đang tắt ở ô này";
    CannotTypeHere => "Typio can’t type here", "Typio không gõ được ở đây";
    ClickWhereText => "Click where the text should go, then try again",
        "Bấm vào chỗ cần chèn chữ rồi thử lại";
    CannotTypeField => "Typio can’t type into this field", "Typio không gõ được vào ô này";
    OnInApp => "Typio on in {name}", "Typio bật trong {name}";
    OffInApp => "Typio off in {name}", "Typio tắt trong {name}";
    TurnedOnImport => "Typio on. Import a dataset from the Typio menu.",
        "Typio đã bật. Hãy nhập bộ dữ liệu từ menu Typio.";
    TurnedOn => "Typio on", "Typio đã bật";
    TurnedOff => "Typio off", "Typio đã tắt";
    CannotReadData => "Typio could not read its data", "Typio không đọc được dữ liệu";
    DataFileIn => "{error}\n\nThe file is in {path}.", "{error}\n\nFile nằm ở {path}.";

    // Saving and importing datasets
    NeedName => "Give the dataset a name.", "Hãy đặt tên cho bộ dữ liệu.";
    DatasetGone => "This dataset no longer exists.", "Bộ dữ liệu này không còn nữa.";
    BadLines => "Line {lines} is not “trigger: text”.", "Dòng {lines} không đúng dạng “trigger: nội dung”.";
    DuplicateTrigger => "The trigger “{trigger}” is used more than once.",
        "Trigger “{trigger}” bị dùng nhiều lần.";
    TriggerElsewhere => "“{trigger}” is also a trigger in “{name}”.",
        "“{trigger}” cũng là trigger trong “{name}”.";
    AddLine => "Add at least one line.", "Hãy thêm ít nhất một dòng.";
    FileTooLarge => "This file is larger than 10 MB.", "File này lớn hơn 10 MB.";
    NotUtf8 => "This file is not UTF-8 text.", "File này không phải văn bản UTF-8.";
    NotJson => "This file is not valid JSON.", "File này không phải JSON hợp lệ.";
    NothingToImport => "This file has no values to import.", "File này không có giá trị nào để nhập.";
    ImportedBackup => "Imported {count} datasets from the Typio backup.",
        "Đã nhập {count} bộ dữ liệu từ bản sao lưu Typio.";
    ImportedShortcuts => "Imported {count} shortcuts into “{name}”.",
        "Đã nhập {count} gõ tắt vào “{name}”.";
    SkippedDuplicates => " Skipped {count} duplicate triggers.", " Bỏ qua {count} trigger trùng.";
    ImportedValues => "Imported {count} values into “{name}”.",
        "Đã nhập {count} giá trị vào “{name}”.";
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placeholders(text: &str) -> Vec<&str> {
        let mut found: Vec<&str> = text
            .split('{')
            .skip(1)
            .filter_map(|rest| rest.split_once('}').map(|(key, _)| key))
            .collect();
        found.sort_unstable();
        found
    }

    #[test]
    fn every_text_has_both_languages_with_the_same_placeholders() {
        for &text in T::ALL {
            assert!(!text.en().is_empty() && !text.vi().is_empty(), "{text:?}");
            assert_eq!(placeholders(text.en()), placeholders(text.vi()), "{text:?}");
        }
    }

    #[test]
    fn switches_language_and_fills_placeholders() {
        set(Language::Vietnamese);
        assert_eq!(trf(T::SkipApp, &[("name", "Notes")]), "Bỏ qua Notes");
        set(Language::English);
        assert_eq!(trf(T::SkipApp, &[("name", "Notes")]), "Skip Notes");
        assert_eq!(tr(T::Quit), "Quit Typio");
    }
}
