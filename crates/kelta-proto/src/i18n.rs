//! A tiny fr/de table for the text the core and the desktop shell emit outside the webview (OS
//! notification titles, the native macOS menu), #344. The webview translates the rest with its own catalogs.
//! shortcut: toast and error messages built by the core are not covered; upgrade by sending message ids.

use std::sync::OnceLock;

use crate::settings::Language;

static OS_LANGUAGE: OnceLock<String> = OnceLock::new();

/// The desktop shell records the OS language tag ("fr-FR", "de_DE.UTF-8") once at startup; `System` is
/// English until then (and in tests).
pub fn set_os_language(tag: String) {
    let _ = OS_LANGUAGE.set(tag);
}

impl Language {
    /// `System` replaced by the OS language when Kelta has it, else English.
    pub fn resolve(self) -> Language {
        if self != Language::System {
            return self;
        }
        match OS_LANGUAGE.get().and_then(|t| t.get(..2)).map(str::to_ascii_lowercase).as_deref() {
            Some("fr") => Language::Fr,
            Some("de") => Language::De,
            _ => Language::En,
        }
    }
}

/// (English, French, German); `{}` is filled by the caller.
const TABLE: &[(&str, &str, &str)] = &[
    ("{} needs input", "{} attend une réponse", "{} wartet auf Eingabe"),
    ("{} finished", "{} a terminé", "{} ist fertig"),
    ("Bell in {}", "Alerte dans {}", "Signalton in {}"),
    ("{} ready to review", "{} prêt pour la relecture", "{} bereit zur Prüfung"),
    ("{}: Claude replied", "{} : Claude a répondu", "{}: Claude hat geantwortet"),
    ("{}: Claude finished", "{} : Claude a terminé", "{}: Claude ist fertig"),
    ("Review requested", "Relecture demandée", "Review angefragt"),
    ("CI failed", "CI en échec", "CI fehlgeschlagen"),
    ("PR approved", "PR approuvée", "PR genehmigt"),
    ("Changes requested", "Modifications demandées", "Änderungen angefordert"),
    ("Quit Kelta", "Quitter Kelta", "Kelta beenden"),
    ("Close Window", "Fermer la fenêtre", "Fenster schließen"),
    ("Edit", "Édition", "Bearbeiten"),
    ("Window", "Fenêtre", "Fenster"),
];

/// The `en` message in `lang` (English when `lang` is `System`/`En` or the message is unknown).
pub fn tr(lang: Language, en: &'static str) -> &'static str {
    let Some(row) = TABLE.iter().find(|r| r.0 == en) else { return en };
    match lang.resolve() {
        Language::Fr => row.1,
        Language::De => row.2,
        _ => en,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_keep_their_placeholder() {
        for (en, fr, de) in TABLE {
            assert_eq!(en.matches("{}").count(), fr.matches("{}").count(), "{en}");
            assert_eq!(en.matches("{}").count(), de.matches("{}").count(), "{en}");
        }
    }

    #[test]
    fn picks_the_requested_language() {
        assert_eq!(tr(Language::Fr, "Quit Kelta"), "Quitter Kelta");
        assert_eq!(tr(Language::De, "CI failed"), "CI fehlgeschlagen");
        assert_eq!(tr(Language::System, "CI failed"), "CI failed");
        assert_eq!(tr(Language::Fr, "unlisted"), "unlisted");
    }
}
