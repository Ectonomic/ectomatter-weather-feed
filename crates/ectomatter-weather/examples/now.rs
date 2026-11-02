//! The weather now for the first place a search finds, as a given locale
//! would ask: `cargo run --example now -- de-DE Berlin`. It asks the web
//! itself, as Ectomatter does for the Weather Feed plugin.
fn main() {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(20))).user_agent("Ectomatter (github.com/Ectonomic/Ectomatter)").build().into();
    let get = |url: &str| -> Result<String, String> { agent.get(url).call().map_err(|e| e.to_string())?.body_mut().read_to_string().map_err(|e| e.to_string()) };
    let mut args = std::env::args().skip(1);
    let locale = args.next().unwrap_or_else(|| "en-US".into());
    let name = args.collect::<Vec<_>>().join(" ");
    let lang = ectomatter_weather::units::language(&locale);
    let places = ectomatter_weather::search(&get, if name.is_empty() { "Tampa" } else { &name }, &lang).expect("search");
    let Some(p) = places.first() else { return println!("No place found.") };
    println!("{} (Latin: {}), {}, {} [{}] ({}, {})", p.name, p.latin, p.region, p.country, p.country_code, p.lat, p.lon);
    println!("units for {locale}: {:?}", ectomatter_weather::units::units_for(&locale));
    let r = ectomatter_weather::ask(&get, p.lat, p.lon).expect("weather");
    println!("{r:?}");
    println!("warning service: {:?}, answer: {:?}, inferred: {}", ectomatter_weather::alerts::source(&p.country_code),
        ectomatter_weather::alerts::ask(&get, p.lat, p.lon, &p.country_code, ectomatter_weather::now_secs()), ectomatter_weather::infer(&r));
}
