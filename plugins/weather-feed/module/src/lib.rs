//! The Weather Feed plugin: the weather at the place chosen in Settings, from
//! Open-Meteo every quarter of an hour, and the warnings in force there from
//! whichever service issues them, every three minutes (the weather library,
//! crates/ectomatter-weather). It reaches the web only through Ectomatter,
//! which asks only the hosts its manifest names.
//!
//! It tells Ectomatter its Settings section (settings/section) and its
//! readings (readings/changed) whenever they change, so Ectomatter never
//! waits on it while it waits on the network. Ectomatter keeps its settings
//! (the place, and the units the section shows) and hands them over
//! (settings/set); a place typed in Settings is looked up here
//! (settings/search).

use std::cell::RefCell;

use ectomatter_weather::{self as weather, alerts, units, At, Place, Report};
use serde_json::{json, Value};

wit_bindgen::generate!({ path: "../../../wit", world: "core" });

use ectomatter::core::{host, net};

/// The timers: the weather due, and the warnings due.
const WEATHER: u32 = 1;
const WARNINGS: u32 = 2;

#[derive(Default)]
struct State {
    /// this computer's region, for the units to start in and the language places are named in
    locale: String,
    /// the settings as Ectomatter keeps them: place, temperature, wind
    values: serde_json::Map<String, Value>,
    place: Option<Place>,
    report: Option<Report>,
    /// the warning in force as last asked, and the place it was asked for
    warning: Option<(At, i32)>,
}

thread_local! {
    static S: RefCell<State> = RefCell::new(State::default());
}

/// The web, as Ectomatter reaches it for this plugin.
fn get(url: &str) -> Result<String, String> {
    net::fetch(url, 20_000)
}

fn at(p: &Place) -> At {
    At { lat: p.lat, lon: p.lon, country: p.country_code.clone() }
}

/// A clock time in words, as the rest of Ectomatter writes one ("7:28 AM").
fn twelve_hour(hour: u32, minute: u32) -> String {
    let h = if hour % 12 == 0 { 12 } else { hour % 12 };
    format!("{h}:{minute:02} {}", if hour < 12 { "AM" } else { "PM" })
}

const CREDIT: &str = "Weather and air from Open-Meteo (open-meteo.com, CC BY 4.0), every 15 minutes. Places from Open-Meteo, and from Photon over OpenStreetMap (ODbL) for names Open-Meteo cannot match. Warnings every 3 minutes from the National Weather Service, Environment and Climate Change Canada, or MeteoAlarm (meteoalarm.org, CC BY 4.0).";

impl State {
    fn chosen(&self, key: &str) -> Option<String> {
        self.values.get(key).and_then(Value::as_str).map(String::from)
    }

    fn temperature_unit(&self) -> String {
        self.chosen("temperature").unwrap_or_else(|| units::units_for(&self.locale).temperature.to_string())
    }

    fn wind_unit(&self) -> String {
        self.chosen("wind").unwrap_or_else(|| units::units_for(&self.locale).wind.to_string())
    }

    /// The warning in force: the service's, where there is one for the place,
    /// or else read from the weather itself; and whether it was read so.
    fn warning(&self) -> (i32, bool) {
        let Some(p) = &self.place else { return (weather::NO_ALERT, false) };
        if alerts::source(&p.country_code).is_some() {
            let level = self.warning.as_ref().filter(|(w, _)| *w == at(p)).map_or(weather::NO_ALERT, |(_, l)| *l);
            return (level, false);
        }
        let level = self.report.as_ref().map_or(weather::NO_ALERT, weather::infer);
        (level, level != weather::NO_ALERT)
    }

    /// The weather asked for the place now, and again in a quarter of an hour
    /// (in a minute, where it couldn't be had, the last report standing).
    fn ask_weather(&mut self) {
        let Some(p) = self.place.clone() else { return };
        match weather::ask(&get, p.lat, p.lon) {
            Ok(r) => {
                self.report = Some(r);
                host::after((weather::EVERY * 1000) as u32, WEATHER);
            }
            Err(e) => {
                host::log(&format!("the weather for {} wasn’t had: {e}", p.name));
                host::after((weather::RETRY * 1000) as u32, WEATHER);
            }
        }
    }

    /// The warnings asked for the place now, and again in three minutes, where
    /// a service issues them for it; a place's last answer kept when the
    /// service can't be reached.
    fn ask_warnings(&mut self) {
        let Some(p) = self.place.clone() else { return };
        if alerts::source(&p.country_code).is_none() {
            return;
        }
        let a = at(&p);
        let last = self.warning.as_ref().filter(|(w, _)| *w == a).map(|(_, l)| *l);
        let level = alerts::ask(&get, p.lat, p.lon, &p.country_code, weather::now_secs()).or(last).unwrap_or(weather::NO_ALERT);
        self.warning = Some((a, level));
        host::after((weather::ALERTS_EVERY * 1000) as u32, WARNINGS);
    }

    /// The section as Settings draws it now: the place, the weather there,
    /// the warnings and where they come from, and the units.
    fn section(&self) -> Value {
        let place = self.values.get("place").cloned().unwrap_or(Value::Null);
        let has_place = place.is_object();
        let d = units::units_for(&self.locale);
        let (tu, wu) = (self.temperature_unit(), self.wind_unit());
        let where_ = if has_place {
            [place["name"].as_str(), place["region"].as_str(), place["country"].as_str()].into_iter().flatten().filter(|t| !t.is_empty()).collect::<Vec<_>>().join(", ")
        } else {
            "Every device that shows weather uses this place.".into()
        };
        let mut layout = vec![json!({ "show": ["place"], "note": where_ })];
        if has_place {
            layout.push(match &self.report {
                None => json!({ "card": { "label": "Now", "waiting": "Getting the weather…" } }),
                Some(r) => {
                    let t = if tu == "f" { r.temperature * 9.0 / 5.0 + 32.0 } else { r.temperature };
                    let wind = |k: f32| match wu.as_str() {
                        "mph" => format!("{} mph", (k / 1.609).round()),
                        "ms" => format!("{} m/s", (k / 3.6).round()),
                        _ => format!("{} km/h", k.round()),
                    };
                    let clock = |secs: f32| twelve_hour((secs / 3600.0).floor().clamp(0.0, 23.0) as u32, ((secs % 3600.0) / 60.0).floor() as u32);
                    json!({ "card": {
                        "label": "Now",
                        "big": format!("{}°{}", t.round(), tu.to_uppercase()),
                        "beside": weather::sky(r.code),
                        "facts": [["Cloud", format!("{}%", (r.cloud * 100.0).round())], ["Wind", wind(r.wind_speed)], ["Sunrise", clock(r.sunrise)], ["Sunset", clock(r.sunset)]],
                    } })
                }
            });
        }
        let country = place["country_code"].as_str().unwrap_or("");
        let official = alerts::source(country).map(|x| x.name());
        let (tone, words) = match (has_place, official) {
            (false, _) => ("quiet", "Chosen with the place"),
            (true, Some(n)) => ("good", n),
            (true, None) => ("warn", "Read from the weather"),
        };
        let note = if has_place {
            let in_force = weather::warning_name(self.warning().0).map_or(String::new(), |n| format!("{n} {}. ", if official.is_some() { "warning in force" } else { "conditions now" }));
            let why = if official.is_some() {
                "Official warnings for this place, checked every 3 minutes."
            } else {
                "No warning service covers this country yet, so storm-force wind with rain stands in for an official warning. A tornado cannot be told from the weather alone."
            };
            format!("{in_force}{why}")
        } else {
            String::new()
        };
        layout.push(json!({ "status": { "label": "Warnings", "tone": tone, "words": words, "note": note } }));
        let units_note = if self.chosen("temperature").is_none() && self.chosen("wind").is_none() {
            format!("Set from this computer's region ({}). Change either and Ectomatter keeps your choice.", self.locale.replace('-', "_"))
        } else {
            String::new()
        };
        layout.push(json!({ "show": ["temperature", "wind"], "note": units_note }));
        layout.push(json!({ "credit": CREDIT }));
        json!({
            "label": "Weather",
            "settings": [
                { "id": "place", "type": "search", "label": "Place", "text": place["name"].as_str().unwrap_or("") },
                { "id": "temperature", "type": "choice", "label": "Temperature", "default": d.temperature,
                  "options": [{ "value": "c", "label": "Celsius" }, { "value": "f", "label": "Fahrenheit" }] },
                { "id": "wind", "type": "choice", "label": "Wind", "default": d.wind,
                  "options": [{ "value": "kmh", "label": "km/h" }, { "value": "mph", "label": "mph" }, { "value": "ms", "label": "m/s" }] },
            ],
            "layout": layout,
        })
    }

    /// The section and the readings, told as they are now.
    fn tell(&self) {
        host::notify("settings/section", &json!({ "section": self.section() }).to_string());
        let r = self.report.as_ref();
        host::notify("readings/changed", &json!({ "values": { "temperature": r.map(|r| r.temperature), "sky": r.map(|r| weather::sky(r.code)) } }).to_string());
    }
}

fn answer(method: &str, p: &Value) -> Result<Value, String> {
    match method {
        "initialize" => {
            let met = ectomatter_protocol::meet(p, "Weather Feed", env!("CARGO_PKG_VERSION"))?;
            S.with(|s| {
                let mut s = s.borrow_mut();
                s.locale = p["locale"].as_str().unwrap_or("en-US").to_string();
                s.tell();
            });
            host::notify("readings/list", &json!({ "readings": [
                { "id": "temperature", "label": "Outside temperature", "unit": "°C", "kind": "outside-temperature" },
                { "id": "sky", "label": "Sky", "kind": "sky" },
            ] }).to_string());
            Ok(met)
        }
        // the settings as Ectomatter keeps them; a new place is asked for at once
        "settings/set" => {
            S.with(|s| {
                let mut s = s.borrow_mut();
                s.values = p["values"].as_object().cloned().unwrap_or_default();
                let place: Option<Place> = s.values.get("place").cloned().and_then(|v| serde_json::from_value(v).ok());
                if place != s.place {
                    s.place = place;
                    s.report = None;
                    s.tell();
                    s.ask_weather();
                    s.ask_warnings();
                }
                s.tell();
            });
            Ok(Value::Null)
        }
        // the places matching what's typed into the place's box, named in this computer's language
        "settings/search" => {
            let text = p["text"].as_str().unwrap_or("").to_string();
            let lang = S.with(|s| units::language(&s.borrow().locale));
            let found = weather::search(&get, &text, &lang)?;
            let rows: Vec<Value> = found
                .iter()
                .map(|f| {
                    let label = if !f.latin.is_empty() && f.latin != f.name { format!("{} ({})", f.name, f.latin) } else { f.name.clone() };
                    let sub = [f.region.as_str(), f.country.as_str()].iter().filter(|x| !x.is_empty()).cloned().collect::<Vec<_>>().join(", ");
                    json!({ "value": f, "label": label, "sub": sub })
                })
                .collect();
            Ok(json!({ "found": rows, "none": "No place by that name" }))
        }
        "shutdown" => Ok(Value::Null),
        _ => Err(format!("The Weather Feed doesn’t answer {method}.")),
    }
}

struct Plugin;

impl Guest for Plugin {
    fn call(method: String, params: String) -> Result<String, String> {
        let p: Value = if params.is_empty() { Value::Null } else { serde_json::from_str(&params).map_err(|e| format!("{method}: {e}"))? };
        answer(&method, &p).map(|v| v.to_string())
    }

    fn timer(tag: u32) {
        S.with(|s| {
            let mut s = s.borrow_mut();
            match tag {
                WEATHER => s.ask_weather(),
                WARNINGS => s.ask_warnings(),
                _ => return,
            }
            s.tell();
        });
    }
}

export!(Plugin);
