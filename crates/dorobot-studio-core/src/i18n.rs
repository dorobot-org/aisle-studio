//! Display-only localization. Protocol values and user-authored data stay intact.
use makepad_widgets::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[serde(rename = "zh-CN", alias = "zh", alias = "zh-Hans")]
    Chinese,
    #[default]
    #[serde(rename = "en", other)]
    English,
}

impl Language {
    pub fn code(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Chinese => "zh-CN",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "zh" | "zh-CN" | "zh-Hans" => Self::Chinese,
            _ => Self::English,
        }
    }
}

fn catalog() -> &'static BTreeMap<String, String> {
    static CATALOG: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("../resources/i18n/zh-CN.json"))
            .expect("Validated Chinese UI catalog")
    })
}

fn patterns() -> &'static Vec<(&'static str, &'static str)> {
    static PATTERNS: OnceLock<Vec<(&str, &str)>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        let mut patterns: Vec<_> = catalog()
            .iter()
            .filter(|(k, _)| k.contains("{0}"))
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        patterns.sort_by_key(|(source, _)| std::cmp::Reverse(source.len()));
        patterns
    })
}

/// Match a complete application message. Captured parameters are opaque: paths,
/// identifiers and user text must never be translated recursively.
fn parameters<'a>(pattern: &str, source: &'a str) -> Option<Vec<&'a str>> {
    let mut values = Vec::new();
    let mut rest = source;
    let mut pattern = pattern;
    while let Some(open) = pattern.find('{') {
        rest = rest.strip_prefix(&pattern[..open])?;
        let close = pattern[open..].find('}')? + open;
        if pattern[open + 1..close].parse::<usize>().ok()? != values.len() {
            return None;
        }
        pattern = &pattern[close + 1..];
        let separator = pattern.split('{').next()?;
        if separator.is_empty() {
            if !pattern.is_empty() {
                return None;
            }
            values.push(rest);
            rest = "";
        } else {
            let at = if !pattern.contains('{') {
                rest.strip_suffix(separator)?.len()
            } else {
                rest.find(separator)?
            };
            values.push(&rest[..at]);
            rest = &rest[at..];
        }
    }
    (rest == pattern).then_some(values)
}

fn substitute(template: &str, values: &[&str]) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('}').map(|n| n + open) else {
            break;
        };
        let value = rest[open + 1..close]
            .parse::<usize>()
            .ok()
            .and_then(|i| values.get(i));
        if let Some(value) = value {
            out.push_str(value);
        } else {
            out.push_str(&rest[open..=close]);
        }
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    out
}

pub fn text(language: Language, source: &str) -> String {
    if language == Language::English || source.is_empty() {
        return source.to_owned();
    }
    if let Some(value) = catalog().get(source) {
        return value.clone();
    }
    // These are structured, app-owned controller/status lines, never dataset
    // task strings. Other unmatched text (including native diagnostics) survives.
    if [
        "LIVE · ",
        "STALE · ",
        "ENDED · ",
        "Simulation · ",
        "Preview only · ",
        "Recording · Gamepad · ",
        "Connecting · ",
        "Disconnected · ",
        "Reconnecting · ",
        "Ended · ",
        "Left · J1",
        "Right · J1",
    ]
    .iter()
    .any(|prefix| source.starts_with(prefix))
    {
        return source
            .split(" · ")
            .map(|part| text(language, part))
            .collect::<Vec<_>>()
            .join(" · ");
    }
    let rig_hint = "Rig loaded. Continue starts LeRobot calibration; follow the prompts and press Continue when asked.";
    if let Some(prefix) = source
        .strip_suffix(rig_hint)
        .and_then(|s| s.strip_suffix('\n'))
    {
        return format!("{}\n{}", text(language, prefix), text(language, rig_hint));
    }
    for (pattern, translated) in patterns() {
        if let Some(values) = parameters(pattern, source) {
            return substitute(translated, &values);
        }
    }
    source.to_owned()
}

#[derive(Default)]
struct Entry {
    source: String,
    rendered: String,
    language: Language,
}

impl Entry {
    fn render(&mut self, current: &str, language: Language) -> Option<String> {
        if current != self.rendered {
            self.source = current.to_owned();
        }
        if current == self.rendered && language == self.language {
            return None;
        }
        self.rendered = text(language, &self.source);
        self.language = language;
        (current != self.rendered).then(|| self.rendered.clone())
    }
}

/// Keeps source captions alongside their rendered form, so switching back never
/// relies on reverse translation. Widgets, focus and all application state stay.
#[derive(Default)]
pub struct Localizer {
    entries: HashMap<WidgetUid, Entry>,
}

impl Localizer {
    pub fn untranslated(&self) -> Vec<String> {
        self.entries
            .values()
            .filter(|e| {
                e.language == Language::Chinese
                    && e.source == e.rendered
                    && e.source.bytes().filter(u8::is_ascii_alphabetic).count() > 2
            })
            .map(|e| e.source.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }
    pub fn apply(
        &mut self,
        cx: &mut Cx,
        root: &WidgetRef,
        language: Language,
        protected: &HashSet<WidgetUid>,
    ) {
        let mut seen = HashSet::new();
        self.visit(cx, root, language, protected, &mut seen);
        self.entries.retain(|id, _| seen.contains(id));
    }

    pub fn apply_panel(&mut self, cx: &mut Cx, panel: &WidgetRef, language: Language) {
        self.visit(cx, panel, language, &HashSet::new(), &mut HashSet::new());
    }

    fn visit(
        &mut self,
        cx: &mut Cx,
        widget: &WidgetRef,
        language: Language,
        protected: &HashSet<WidgetUid>,
        seen: &mut HashSet<WidgetUid>,
    ) {
        let id = widget.widget_uid();
        if !seen.insert(id) {
            return;
        }
        if protected.contains(&id) {
            self.entries.remove(&id);
            return;
        }
        if let Some(mut input) = widget.borrow_mut::<TextInput>() {
            // Only the placeholder is UI text. The editable value, selection and
            // IME composition are never read or changed by localization.
            let current = input.empty_text().to_owned();
            if let Some(value) = self
                .entries
                .entry(id)
                .or_default()
                .render(&current, language)
            {
                input.set_empty_text(cx, value);
            }
        } else if widget.borrow::<Label>().is_some() || widget.borrow::<Button>().is_some() {
            let current = widget.text();
            if let Some(value) = self
                .entries
                .entry(id)
                .or_default()
                .render(&current, language)
            {
                widget.set_text(cx, &value);
            }
        }
        let mut children = Vec::new();
        widget.children(&mut |name, child| {
            // Reusable rows carry raw dataset/device/joint identifiers.
            if ![
                live_id!(name),
                live_id!(jname),
                live_id!(jn),
                live_id!(dev_name),
                live_id!(dev_detail),
                live_id!(name_0),
                live_id!(name_1),
                live_id!(name_2),
                live_id!(name_3),
                live_id!(name_4),
                live_id!(name_5),
            ]
            .contains(&name)
            {
                children.push(child);
            }
        });
        for child in children {
            self.visit(cx, &child, language, protected, seen);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn switching_back_and_dynamic_updates_preserve_original_sources() {
        let mut entry = Entry::default();
        assert_eq!(
            entry.render("Record", Language::Chinese).as_deref(),
            Some("录制")
        );
        assert_eq!(entry.render("录制", Language::Chinese), None);
        assert_eq!(
            entry.render("录制", Language::English).as_deref(),
            Some("Record")
        );
        assert_eq!(
            entry.render("Stop", Language::Chinese).as_deref(),
            Some("停止")
        );
        assert_eq!(
            entry.render("停止", Language::English).as_deref(),
            Some("Stop")
        );
    }
    #[test]
    fn parameters_are_opaque_and_not_recursive() {
        assert_eq!(
            parameters("Saved {0} · Discarded {1}", "Saved 2 · Discarded 1"),
            Some(vec!["2", "1"])
        );
        assert_eq!(
            parameters("Saved {0} frames", "Saved 2 frames trailing"),
            None
        );
        assert_eq!(
            substitute("已打开 {0}", &["Record/{1}/中文"]),
            "已打开 Record/{1}/中文"
        );
        assert_eq!(text(Language::Chinese, "Speed 0.5×"), "速度 0.5×");
        assert_eq!(
            text(Language::Chinese, "/tmp/Record/机器人"),
            "/tmp/Record/机器人"
        );
        assert_eq!(
            text(Language::Chinese, "unknown native diagnostic"),
            "unknown native diagnostic"
        );
        assert_eq!(text(Language::Chinese, "LIVE · Simulation · Drive · Controller disconnected · ● Recording · 2.8 fps preview"),
            "实时 · 仿真 · 底盘 · 手柄未连接 · ● 录制中 · 预览 2.8 fps");
        assert_eq!(text(Language::Chinese, "Found a port: /dev/Record\nRig loaded. Continue starts LeRobot calibration; follow the prompts and press Continue when asked."),
            "发现串口：/dev/Record\n设备配置已加载。点击“继续”开始校准，随后按提示操作。");
    }
    #[test]
    fn catalog_parameters_are_complete_and_unambiguous() {
        fn slots(s: &str) -> Vec<String> {
            s.split('{')
                .skip(1)
                .map(|p| p.split('}').next().unwrap().to_owned())
                .collect()
        }
        for (source, target) in catalog() {
            assert!(!target.is_empty(), "{source}");
            let mut a = slots(source);
            let mut b = slots(target);
            a.sort();
            b.sort();
            assert_eq!(a, b, "{source}");
            if source.contains("{0}") {
                assert!(
                    source.chars().any(|c| c.is_ascii_alphabetic()),
                    "Unanchored template: {source}"
                );
                let sample = substitute(source, &["0", "1", "2", "3", "4", "5"]);
                assert!(parameters(source, &sample).is_some(), "{source}");
            }
        }
    }
    #[test]
    fn old_and_unknown_locale_preferences_fall_back_to_english() {
        let old: crate::preferences::Session = serde_json::from_str("{}").unwrap();
        assert_eq!(old.language, Language::English);
        let unknown: crate::preferences::Session =
            serde_json::from_str(r#"{"language":"xx"}"#).unwrap();
        assert_eq!(unknown.language, Language::English);
        let chinese: crate::preferences::Session =
            serde_json::from_str(r#"{"language":"zh-CN"}"#).unwrap();
        assert_eq!(chinese.language, Language::Chinese);
    }
}
