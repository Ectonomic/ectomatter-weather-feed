//! The weather now in one place, for every device Ectomatter drives: the M3's
//! backgrounds, the Z10's screen, and whatever comes next. From Open-Meteo
//! (free, no account; CC BY 4.0), the weather and the air, asked every
//! quarter of an hour, and the warnings in force from the service that
//! issues them for the place (alerts.rs), asked every three minutes, as a
//! tornado warning lasts under an hour. Offline, the last report stands.
//!
//! The Weather Feed plugin's library (plugins/weather-feed): it asks the web through
//! whatever `get` it's handed (in the plugin, Ectomatter's, which reaches
//! only the hosts its manifest names), so nothing here opens a connection
//! or a thread of its own. Brought over from the M3 plugin's weather module,
//! so one feed serves every device instead of each one asking for itself.

pub mod alerts;
mod regions;
pub mod units;

/// How the web is asked: a web address's answer, as text.
pub type Get<'a> = &'a dyn Fn(&str) -> Result<String, String>;

/// How often the weather is asked again for the same place, in seconds; and
/// how soon after it couldn't be reached.
pub const EVERY: i64 = 15 * 60;
pub const RETRY: i64 = 60;
/// How often the warnings are asked again, in seconds.
pub const ALERTS_EVERY: i64 = 3 * 60;

/// The most serious warning in force at the place, as the National Weather
/// Service issues them.
pub const NO_ALERT: i32 = 0;
pub const TROPICAL_STORM_WARNING: i32 = 1;
pub const HURRICANE_WARNING: i32 = 2;
pub const TORNADO_WARNING: i32 = 3;

/// One report, in the place's own time.
#[derive(Clone, Copy, Debug, PartialEq, Default, serde::Serialize)]
pub struct Report {
    /// The WMO weather code Open-Meteo gives (0 clear .. 99 thunderstorm with hail).
    pub code: i32,
    /// The wind or its gusts, whichever is stronger, in km/h.
    pub wind: f32,
    /// Sunrise and sunset, in seconds after the place's midnight.
    pub sunrise: f32,
    pub sunset: f32,
    /// The place's offset from UTC, in seconds.
    pub offset: i32,
    /// How much of the sky is cloud, 0 to 1.
    pub cloud: f32,
    /// Rain, showers and snow falling now, each in mm an hour of water (snow as
    /// the rain it would melt to).
    pub rain: f32,
    pub showers: f32,
    pub snow: f32,
    /// The steady wind in km/h, and the way it blows from, in degrees from north.
    pub wind_speed: f32,
    pub wind_from: f32,
    /// How far one can see, in km, and the temperature, in degrees Celsius.
    pub visibility: f32,
    pub temperature: f32,
    /// The air: fine particles (PM2.5) and dust in micrograms a cubic meter, and
    /// the aerosol optical depth (haze or smoke), with whether it has come.
    pub pm25: f32,
    pub dust: f32,
    pub haze: f32,
    pub air_known: bool,
    /// The most serious warning in force, one of the constants above.
    pub alert: i32,
    /// Whether that warning was read from the weather itself, where no
    /// service issues warnings for the place (see [infer]).
    pub alert_inferred: bool,
    /// When this report was fetched, in seconds since 1970.
    pub fetched: i64,
}

/// A warning read from the weather alone, for places no warning service
/// covers: a sustained wind of hurricane force (Beaufort 12, 118 km/h) as the
/// hurricane, and a sustained storm-force wind (63 km/h, Beaufort 8) with rain
/// or thunder falling as the tropical storm. A tornado cannot be told from a
/// forecast model, so it is never inferred.
pub fn infer(r: &Report) -> i32 {
    if r.wind_speed >= 118.0 {
        HURRICANE_WARNING
    } else if r.wind_speed >= 63.0 && r.code >= 51 {
        TROPICAL_STORM_WARNING
    } else {
        NO_ALERT
    }
}

const CURRENT: &str = "weather_code,cloud_cover,rain,showers,snowfall,wind_speed_10m,wind_direction_10m,wind_gusts_10m,visibility,temperature_2m";

/// The report in an Open-Meteo forecast reply asked for `current=` [CURRENT]
/// `&daily=sunrise,sunset&timezone=auto`.
pub fn parse(reply: &serde_json::Value) -> Option<Report> {
    let current = &reply["current"];
    let code = current["weather_code"].as_i64()? as i32;
    let n = |k: &str| current[k].as_f64().unwrap_or(0.0) as f32;
    let wind = n("wind_speed_10m").max(n("wind_gusts_10m"));
    // the current amounts cover the fifteen minutes before, so four times them
    // is the rate an hour; snowfall comes in cm, about a tenth of that in water
    // local times as "2026-09-25T07:12"
    let clock = |v: &serde_json::Value| -> Option<f32> {
        let time = v.as_str()?.split('T').nth(1)?;
        let mut parts = time.split(':').map(|p| p.parse::<f32>().ok());
        let (h, m) = (parts.next()??, parts.next()??);
        Some(h * 3600.0 + m * 60.0)
    };
    Some(Report {
        code,
        wind,
        sunrise: clock(&reply["daily"]["sunrise"][0]).unwrap_or(6.0 * 3600.0),
        sunset: clock(&reply["daily"]["sunset"][0]).unwrap_or(18.0 * 3600.0),
        offset: reply["utc_offset_seconds"].as_i64().unwrap_or(0) as i32,
        cloud: n("cloud_cover") / 100.0,
        rain: n("rain") * 4.0,
        showers: n("showers") * 4.0,
        snow: n("snowfall") * 4.0 * 10.0 * 0.1,
        wind_speed: n("wind_speed_10m"),
        wind_from: n("wind_direction_10m"),
        visibility: current["visibility"].as_f64().map_or(20.0, |v| v as f32 / 1000.0),
        temperature: n("temperature_2m"),
        ..Report::default()
    })
}

/// The air in an Open-Meteo air quality reply asked for
/// `current=pm2_5,dust,aerosol_optical_depth`: PM2.5, dust, optical depth.
pub fn parse_air(reply: &serde_json::Value) -> Option<(f32, f32, f32)> {
    let c = &reply["current"];
    Some((c["pm2_5"].as_f64()? as f32, c["dust"].as_f64().unwrap_or(0.0) as f32, c["aerosol_optical_depth"].as_f64().unwrap_or(0.0) as f32))
}

/// The most serious warning in a National Weather Service alerts reply.
pub fn parse_alerts(reply: &serde_json::Value) -> i32 {
    let mut worst = NO_ALERT;
    for f in reply["features"].as_array().into_iter().flatten() {
        let rank = match f["properties"]["event"].as_str().unwrap_or("") {
            "Tornado Warning" => TORNADO_WARNING,
            "Hurricane Warning" | "Extreme Wind Warning" => HURRICANE_WARNING,
            "Tropical Storm Warning" => TROPICAL_STORM_WARNING,
            _ => NO_ALERT,
        };
        worst = worst.max(rank);
    }
    worst
}

pub(crate) fn get_json(get: Get, url: &str) -> Result<serde_json::Value, String> {
    serde_json::from_str(&get(url)?).map_err(|e| e.to_string())
}

/// The weather and the air at a place (the air left unknown if it cannot be had).
pub fn ask(get: Get, lat: f32, lon: f32) -> Result<Report, String> {
    let reply = get_json(get, &format!("https://api.open-meteo.com/v1/forecast?current={CURRENT}&daily=sunrise,sunset&timezone=auto&forecast_days=1&latitude={lat}&longitude={lon}"))?;
    let mut report = parse(&reply).ok_or("Open-Meteo's reply had no current weather")?;
    report.fetched = now_secs();
    if let Some((pm25, dust, haze)) = get_json(get, &format!("https://air-quality-api.open-meteo.com/v1/air-quality?current=pm2_5,dust,aerosol_optical_depth&latitude={lat}&longitude={lon}")).ok().as_ref().and_then(parse_air) {
        (report.pm25, report.dust, report.haze, report.air_known) = (pm25, dust, haze, true);
    }
    Ok(report)
}

/// A place a search found: its name, where it is, and where it lies (region
/// and country) to tell same-named places apart.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Place {
    /// Open-Meteo's id, or minus OpenStreetMap's for a place found by Photon.
    pub id: i64,
    /// The name in the language asked for.
    pub name: String,
    /// The name in Latin letters, for screens whose font has no others: the
    /// name itself when it is written in them, otherwise its English name.
    pub latin: String,
    pub region: String,
    pub country: String,
    /// ISO 3166 code, which picks the warning service.
    pub country_code: String,
    pub lat: f32,
    pub lon: f32,
}

/// Whether text is written in Latin letters (with any accents), so a screen
/// can show it by dropping the accents.
pub fn is_latin(s: &str) -> bool {
    s.chars().all(|c| c.is_ascii() || ('\u{00C0}'..='\u{024F}').contains(&c) || ('\u{1E00}'..='\u{1EFF}').contains(&c) || " '’-.,·".contains(c))
}

/// The places an Open-Meteo geocoding reply lists.
pub fn parse_places(reply: &serde_json::Value) -> Vec<Place> {
    let s = |v: &serde_json::Value| v.as_str().unwrap_or("").to_string();
    reply["results"].as_array().into_iter().flatten().filter_map(|r| Some(Place {
        id: r["id"].as_i64().unwrap_or(0),
        name: s(&r["name"]),
        latin: s(&r["name"]),
        region: s(&r["admin1"]),
        country: s(&r["country"]),
        country_code: s(&r["country_code"]),
        lat: r["latitude"].as_f64()? as f32,
        lon: r["longitude"].as_f64()? as f32,
    })).collect()
}

/// Up to eight places matching a name, from Open-Meteo's free geocoding (no
/// account), as the M3's place picker searched, named in `language` (such as
/// "de"; see [units::language]). Names not in Latin letters also get their
/// English name, for screens that can only show Latin letters.
pub fn search(get: Get, name: &str, language: &str) -> Result<Vec<Place>, String> {
    let q: String = name.trim().chars().map(|c| match c {
        'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '.' | '_' | '~' => c.to_string(),
        _ => c.to_string().bytes().map(|b| format!("%{b:02X}")).collect(),
    }).collect();
    if q.is_empty() {
        return Ok(vec![]);
    }
    let lang: String = language.chars().filter(|c| c.is_ascii_alphabetic()).take(3).collect();
    let lang = if lang.is_empty() { "en".to_string() } else { lang };
    let ask = |l: &str| get_json(get, &format!("https://geocoding-api.open-meteo.com/v1/search?count=8&language={l}&format=json&name={q}")).map(|r| parse_places(&r));
    let mut found = ask(&lang)?;
    // Open-Meteo cannot match some scripts at all (a search in Japanese finds
    // nothing), so Photon, over OpenStreetMap's names, is asked instead
    if found.is_empty() {
        found = photon(get, &q, &lang).unwrap_or_default();
    }
    // a name in another script gets its English name, looked up by its id
    for p in found.iter_mut().filter(|p| !is_latin(&p.name) && p.id > 0 && p.latin == p.name) {
        if let Ok(e) = get_json(get, &format!("https://geocoding-api.open-meteo.com/v1/get?id={}&language=en", p.id)) {
            if let Some(n) = e["name"].as_str().filter(|n| is_latin(n)) {
                p.latin = n.to_string();
            }
        }
    }
    Ok(found)
}

/// Where the weather is for: latitude, longitude, and the ISO country code
/// that picks the warning service.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct At {
    pub lat: f32,
    pub lon: f32,
    pub country: String,
}

/// Places from Photon (photon.komoot.io, OpenStreetMap data, ODbL), for
/// names Open-Meteo cannot match. Photon names places in English, German or
/// French, or else as the place itself writes its name, so it is asked twice
/// when the local name is not in Latin letters: once for the name to show and
/// once in English for the Latin one.
fn photon(get: Get, q: &str, lang: &str) -> Result<Vec<Place>, String> {
    let ask = |l: &str| get_json(get, &format!("https://photon.komoot.io/api/?q={q}&limit=8&osm_tag=place&lang={l}"));
    let local = if ["en", "de", "fr"].contains(&lang) { lang } else { "default" };
    let read = |reply: &serde_json::Value| -> Vec<Place> {
        let s = |v: &serde_json::Value| v.as_str().unwrap_or("").to_string();
        reply["features"].as_array().into_iter().flatten().filter_map(|f| {
            let p = &f["properties"];
            let c = &f["geometry"]["coordinates"];
            Some(Place {
                id: -p["osm_id"].as_i64().unwrap_or(0),
                name: s(&p["name"]),
                latin: s(&p["name"]),
                region: s(&p["state"]),
                country: s(&p["country"]),
                country_code: s(&p["countrycode"]),
                lat: c[1].as_f64()? as f32,
                lon: c[0].as_f64()? as f32,
            })
        }).filter(|p| !p.name.is_empty()).collect()
    };
    let mut found = read(&ask(local)?);
    if found.iter().any(|p| !is_latin(&p.name)) {
        let english = read(&ask("en").unwrap_or_default());
        for p in found.iter_mut().filter(|p| !is_latin(&p.name)) {
            if let Some(e) = english.iter().find(|e| e.id == p.id && is_latin(&e.name)) {
                p.latin = e.name.clone();
            }
        }
    }
    Ok(found)
}

/// The time now, in seconds since 1970.
pub fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// The sky in words, from the weather's WMO code.
pub fn sky(code: i32) -> String {
    WMO.iter().find(|(k, _)| *k == code).map_or(format!("Weather code {code}"), |(_, n)| n.to_string())
}

const WMO: [(i32, &str); 28] = [
    (0, "Clear"), (1, "Mostly clear"), (2, "Partly cloudy"), (3, "Overcast"), (45, "Fog"), (48, "Freezing fog"),
    (51, "Light drizzle"), (53, "Drizzle"), (55, "Heavy drizzle"), (56, "Freezing drizzle"), (57, "Freezing drizzle"),
    (61, "Light rain"), (63, "Rain"), (65, "Heavy rain"), (66, "Freezing rain"), (67, "Freezing rain"),
    (71, "Light snow"), (73, "Snow"), (75, "Heavy snow"), (77, "Snow grains"), (80, "Light showers"), (81, "Showers"), (82, "Heavy showers"),
    (85, "Snow showers"), (86, "Heavy snow showers"), (95, "Thunderstorm"), (96, "Thunderstorm with hail"), (99, "Thunderstorm with hail"),
];

/// A warning's name, by its rank (the constants above); none for no warning.
pub fn warning_name(level: i32) -> Option<&'static str> {
    match level {
        TROPICAL_STORM_WARNING => Some("Tropical storm"),
        HURRICANE_WARNING => Some("Hurricane"),
        TORNADO_WARNING => Some("Tornado"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_an_open_meteo_reply() {
        let reply = serde_json::json!({
            "utc_offset_seconds": -14400,
            "current": { "weather_code": 73, "wind_speed_10m": 31.5, "wind_gusts_10m": 58.0, "wind_direction_10m": 270, "cloud_cover": 85,
                         "rain": 0.5, "showers": 0.0, "snowfall": 0.2, "visibility": 1800.0, "temperature_2m": -3.5 },
            "daily": { "sunrise": ["2026-09-25T07:12"], "sunset": ["2026-09-25T19:18"] }
        });
        let r = parse(&reply).unwrap();
        assert_eq!((r.code, r.wind, r.offset), (73, 58.0, -14400));
        assert_eq!((r.sunrise, r.sunset), (7.0 * 3600.0 + 12.0 * 60.0, 19.0 * 3600.0 + 18.0 * 60.0));
        assert_eq!((r.cloud, r.rain, r.wind_speed, r.wind_from, r.visibility, r.temperature), (0.85, 2.0, 31.5, 270.0, 1.8, -3.5));
        assert!((r.snow - 0.8).abs() < 1e-5);
        assert_eq!(parse(&serde_json::json!({ "error": true })), None);
        assert_eq!(parse_air(&serde_json::json!({ "current": { "pm2_5": 35.5, "dust": 12.0, "aerosol_optical_depth": 0.9 } })), Some((35.5, 12.0, 0.9)));
    }

    #[test]
    fn infers_storms_only_from_storm_force_wind() {
        let r = |wind_speed: f32, code: i32| Report { wind_speed, code, ..Report::default() };
        assert_eq!(infer(&r(130.0, 0)), HURRICANE_WARNING);
        assert_eq!(infer(&r(70.0, 63)), TROPICAL_STORM_WARNING);
        assert_eq!(infer(&r(70.0, 1)), NO_ALERT); // a gale under a clear sky is no storm
        assert_eq!(infer(&r(40.0, 95)), NO_ALERT);
        assert!(is_latin("São Paulo") && is_latin("Zürich") && !is_latin("Москва") && !is_latin("東京"));
    }

    #[test]
    fn reads_a_place_search() {
        let reply = serde_json::json!({ "results": [
            { "id": 4174757, "name": "Tampa", "admin1": "Florida", "country": "United States", "country_code": "US", "latitude": 27.94752, "longitude": -82.45843 },
            { "name": "Tampa", "admin1": "Brașov", "country": "Romania", "latitude": 45.6, "longitude": 25.7 },
            { "name": "No position" }
        ]});
        let found = parse_places(&reply);
        assert_eq!(found.len(), 2);
        assert_eq!((found[0].name.as_str(), found[0].region.as_str(), found[1].country.as_str()), ("Tampa", "Florida", "Romania"));
        assert_eq!((found[0].id, found[0].country_code.as_str(), found[0].latin.as_str()), (4174757, "US", "Tampa"));
        assert!(parse_places(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn reads_the_worst_warning() {
        let alerts = |events: &[&str]| serde_json::json!({ "features": events.iter().map(|e| serde_json::json!({ "properties": { "event": e } })).collect::<Vec<_>>() });
        assert_eq!(parse_alerts(&alerts(&[])), NO_ALERT);
        assert_eq!(parse_alerts(&alerts(&["Flood Watch", "Tropical Storm Warning"])), TROPICAL_STORM_WARNING);
        assert_eq!(parse_alerts(&alerts(&["Hurricane Warning", "Tornado Warning", "Heat Advisory"])), TORNADO_WARNING);
        assert_eq!(parse_alerts(&alerts(&["Tornado Watch"])), NO_ALERT);
    }
}
