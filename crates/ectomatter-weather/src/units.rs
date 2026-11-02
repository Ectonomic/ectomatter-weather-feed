//! What a place's people expect by default, from the computer's locale
//! (such as "en-US", "en_GB.UTF-8" or "nb-NO"): the language for place names,
//! and the units for temperature and wind. Only defaults: Settings can change
//! either.

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Units {
    /// "c" or "f"
    pub temperature: &'static str,
    /// "kmh", "mph" or "ms"
    pub wind: &'static str,
}

/// The region part of a locale, upper case: "US" from "en_US.UTF-8".
fn region(locale: &str) -> String {
    let base = locale.split(['.', '@']).next().unwrap_or("");
    base.split(['-', '_']).skip(1).find(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_alphabetic()))
        .unwrap_or("").to_ascii_uppercase()
}

/// The language part of a locale, lower case, for Open-Meteo's place search:
/// "de" from "de-AT". English when there is none.
pub fn language(locale: &str) -> String {
    let l: String = locale.split(['-', '_', '.', '@']).next().unwrap_or("").to_ascii_lowercase();
    if l.len() >= 2 && l.len() <= 3 && l.chars().all(|c| c.is_ascii_alphabetic()) && l != "c" { l } else { "en".into() }
}

pub fn units_for(locale: &str) -> Units {
    let r = region(locale);
    // Fahrenheit: the United States, its territories and freely associated
    // states, and the few countries that keep it
    const F: [&str; 13] = ["US", "PR", "GU", "VI", "AS", "MP", "PW", "FM", "MH", "LR", "BS", "BZ", "KY"];
    // wind in meters a second, as their weather services give it
    const MS: [&str; 10] = ["NO", "SE", "FI", "IS", "RU", "BY", "UA", "KZ", "EE", "LV"];
    if F.contains(&r.as_str()) {
        Units { temperature: "f", wind: "mph" }
    } else if r == "GB" {
        Units { temperature: "c", wind: "mph" }
    } else if MS.contains(&r.as_str()) {
        Units { temperature: "c", wind: "ms" }
    } else {
        Units { temperature: "c", wind: "kmh" }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_by_locale() {
        assert_eq!(units_for("en-US"), Units { temperature: "f", wind: "mph" });
        assert_eq!(units_for("en_GB.UTF-8"), Units { temperature: "c", wind: "mph" });
        assert_eq!(units_for("nb-NO"), Units { temperature: "c", wind: "ms" });
        assert_eq!(units_for("de-DE"), Units { temperature: "c", wind: "kmh" });
        assert_eq!(units_for("zh-Hans-CN"), Units { temperature: "c", wind: "kmh" });
        assert_eq!(units_for("C"), Units { temperature: "c", wind: "kmh" });
        assert_eq!(language("de-AT"), "de");
        assert_eq!(language("pt_BR.UTF-8"), "pt");
        assert_eq!(language("C"), "en");
        assert_eq!(language(""), "en");
    }
}
