//! Built-in UI translations; protocol values, paths and phone content remain unchanged.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[serde(rename = "en")]
    #[default]
    English,
    #[serde(rename = "zh-CN")]
    Chinese,
    #[serde(rename = "fr")]
    French,
    #[serde(rename = "es")]
    Spanish,
    #[serde(rename = "system", other)]
    System,
}

impl Language {
    pub fn index(self) -> u32 {
        match self {
            Self::System => 0,
            Self::Chinese => 1,
            Self::English => 2,
            Self::French => 3,
            Self::Spanish => 4,
        }
    }

    pub fn from_index(index: u32) -> Self {
        match index {
            1 => Self::Chinese,
            2 => Self::English,
            3 => Self::French,
            4 => Self::Spanish,
            _ => Self::System,
        }
    }

    fn resolve(self, get: impl Fn(&str) -> Option<String>) -> Self {
        if self != Self::System {
            return self;
        }
        let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .into_iter()
            .filter_map(get)
            .find(|v| !v.is_empty())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let primary = locale.split(['_', '-', '.']).next().unwrap_or("");
        match primary {
            "zh" => Self::Chinese,
            "fr" => Self::French,
            "es" => Self::Spanish,
            _ => Self::English,
        }
    }
    #[cfg(test)]
    fn chinese(self, get: impl Fn(&str) -> Option<String>) -> bool {
        self.resolve(get) == Self::Chinese
    }
}
static RESOLVED: AtomicU32 = AtomicU32::new(2);
static ZH: OnceLock<HashMap<String, String>> = OnceLock::new();
static FR: OnceLock<HashMap<String, String>> = OnceLock::new();
static ES: OnceLock<HashMap<String, String>> = OnceLock::new();
pub fn initialize(language: Language) {
    RESOLVED.store(
        language.resolve(|k| std::env::var(k).ok()).index(),
        Ordering::Relaxed,
    );
}
fn resolved() -> Language {
    Language::from_index(RESOLVED.load(Ordering::Relaxed))
}
pub fn is_chinese() -> bool {
    resolved() == Language::Chinese
}
fn catalog(language: Language) -> Option<&'static HashMap<String, String>> {
    let (cell, text) = match language {
        Language::Chinese => (&ZH, include_str!("zh_cn.json")),
        Language::French => (&FR, include_str!("fr.json")),
        Language::Spanish => (&ES, include_str!("es.json")),
        _ => return None,
    };
    Some(cell.get_or_init(|| {
        serde_json::from_str(text).expect("built-in translation catalog must be valid")
    }))
}
fn translate_to(text: &str, language: Language) -> &str {
    catalog(language)
        .and_then(|c| c.get(text))
        .map(String::as_str)
        .unwrap_or(text)
}
pub fn tr(text: &str) -> &str {
    translate_to(text, resolved())
}
#[cfg(test)]
fn translate(text: &str, chinese: bool) -> &str {
    translate_to(
        text,
        if chinese {
            Language::Chinese
        } else {
            Language::English
        },
    )
}
/// Render catalog {0}/{1} fields using values already formatted and type-checked by Rust.
/// Private indexed delimiters keep user-supplied braces, separators and Unicode intact.
pub fn format_message(key: &str, encoded: String) -> String {
    format_to(key, &encoded, resolved())
}
fn format_to(key: &str, encoded: &str, language: Language) -> String {
    let mut rest = encoded;
    let mut values = Vec::new();
    let mut english = String::new();
    loop {
        let start = format!("\u{e000}{}\u{e001}", values.len());
        let Some(at) = rest.find(&start) else {
            english.push_str(rest);
            break;
        };
        english.push_str(&rest[..at]);
        rest = &rest[at + start.len()..];
        let end = format!("\u{e002}{}\u{e003}", values.len());
        let Some(at) = rest.find(&end) else {
            return key.to_owned();
        };
        let value = &rest[..at];
        english.push_str(value);
        values.push(value);
        rest = &rest[at + end.len()..];
    }
    let Some(template) = catalog(language).and_then(|c| c.get(key)) else {
        return english;
    };
    let mut out = String::new();
    let mut text = template.as_str();
    while let Some(at) = text.find('{') {
        out.push_str(&text[..at]);
        let tail = &text[at + 1..];
        let Some(end) = tail.find('}') else {
            return english;
        };
        let Some(value) = tail[..end]
            .parse::<usize>()
            .ok()
            .and_then(|i| values.get(i))
        else {
            return english;
        };
        out.push_str(value);
        text = &tail[end + 1..];
    }
    out.push_str(text);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_language_overrides_system_and_unknown_settings_remain_loadable() {
        assert_eq!(Language::default(), Language::English);
        assert!(!Language::English.chinese(|_| Some("zh_CN.UTF-8".into())));
        assert!(Language::Chinese.chinese(|_| Some("en_US.UTF-8".into())));
        assert_eq!(
            serde_json::from_str::<Language>("\"future-language\"").unwrap(),
            Language::System
        );
        assert_eq!(
            serde_json::to_string(&Language::Chinese).unwrap(),
            "\"zh-CN\""
        );
    }

    #[test]
    fn system_locale_obeys_precedence_and_ignores_empty_overrides() {
        assert!(!Language::System.chinese(|key| {
            Some(
                match key {
                    "LC_ALL" => "C",
                    _ => "zh_CN.UTF-8",
                }
                .into(),
            )
        }));
        assert!(Language::System.chinese(|key| {
            Some(
                match key {
                    "LC_ALL" => "",
                    "LC_MESSAGES" => "zh_CN.UTF-8",
                    _ => "en_US.UTF-8",
                }
                .into(),
            )
        }));
        assert!(!Language::System.chinese(|_| None));
    }

    #[test]
    fn catalog_contains_language_controls_and_no_empty_translations() {
        let catalog: HashMap<String, String> =
            serde_json::from_str(include_str!("zh_cn.json")).unwrap();
        assert_eq!(catalog.get("Preferences").unwrap(), "偏好设置");
        assert_eq!(catalog.get("Language").unwrap(), "语言");
        assert!(catalog.values().all(|value| !value.trim().is_empty()));
    }

    #[test]
    fn chinese_and_english_translate_ui_but_preserve_unknown_content() {
        assert_eq!(translate("Preferences", true), "偏好设置");
        assert_eq!(translate("Preferences", false), "Preferences");
        assert_eq!(
            translate("A phone supplied app name", true),
            "A phone supplied app name"
        );
        assert_eq!(translate("", true), "");
        for language in [
            Language::System,
            Language::Chinese,
            Language::English,
            Language::French,
            Language::Spanish,
        ] {
            assert_eq!(Language::from_index(language.index()), language);
        }
    }
}

#[cfg(test)]
mod multilingual_tests {
    use super::*;
    #[test]
    fn all_languages_cover_labels_and_keep_format_fields() {
        let zh = catalog(Language::Chinese).unwrap();
        for lang in [Language::French, Language::Spanish] {
            let c = catalog(lang).unwrap();
            for key in zh.keys() {
                assert!(c.contains_key(key), "missing {key} in {lang:?}");
            }
            assert!(c.values().all(|s| !s.trim().is_empty()));
            let fields=regex::Regex::new(r"\{([^{}]*)\}").unwrap();
            for (key,value) in c {
                let count=fields.find_iter(key).count();
                let actual:std::collections::BTreeSet<usize>=fields.captures_iter(value).map(|m|m[1].parse().expect("translated fields must be numbered")).collect();
                assert_eq!(actual,(0..count).collect(),"format fields differ: {key}");
            }
        }
        assert_eq!(
            Language::System.resolve(|_| Some("fr_FR.UTF-8".into())),
            Language::French
        );
        assert_eq!(
            Language::System.resolve(|_| Some("es-ES".into())),
            Language::Spanish
        );
        assert_eq!(
            translate_to("A phone app {name}", Language::French),
            "A phone app {name}"
        );
    }
    #[test]
    fn formatted_values_keep_braces_and_unicode_and_are_evaluated_once() {
        let value = "user {data}: \u{e000} icon";
        let encoded = format!("Opening \u{e000}0\u{e001}{value}\u{e002}0\u{e003}…");
        assert_eq!(
            format_to("Opening {}…", &encoded, Language::French),
            format!("Ouverture de {value}…")
        );
        assert_eq!(
            format_to("Opening {}…", &encoded, Language::Spanish),
            format!("Abriendo {value}…")
        );
        let n = std::cell::Cell::new(0);
        let rendered = crate::tr_format!("Count {}", "数量 {}", {
            n.set(n.get() + 1);
            n.get()
        });
        assert_eq!(rendered, "Count 1");
        assert_eq!(n.get(), 1);
        let number = 1.25;
        let rendered = crate::tr_format!("Number {number:.1}", "数值 {number:.1}");
        assert_eq!(rendered, "Number 1.2");
    }
}
