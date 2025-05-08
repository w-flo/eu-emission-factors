use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{BufRead, stdin},
    path::{Path, PathBuf},
};

use csv::{Reader, ReaderBuilder, Trim, Writer};
use deunicode::deunicode;
use file_paths::FilePaths;
use generation_emission_match::{Match, YearlyEmission, YearlyGeneration};
use serde::{Deserialize, Serialize};

mod file_paths;
mod generation_emission_match;
mod preprocess;

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("preprocess") => {
            let year = args
                .next()
                .expect("Must specify `preprocess <year>`")
                .parse::<u32>()
                .expect("Not a valid year");

            let paths = FilePaths::new(PathBuf::from("data"), year);
            let ets_countries = preprocess::yearly_emissions(year, &paths);
            preprocess::yearly_generation(&ets_countries, &paths);
        }
        Some(year_str) => {
            let year = year_str
                .parse::<u32>()
                .unwrap_or_else(|_| panic!("Not a valid year: \"{year_str}\""));

            let paths = FilePaths::new(PathBuf::from("data"), year);
            let mut matches = Vec::new();
            load_manual_matches(&mut matches, &paths);
            generate_auto_matches(&mut matches, &paths);
            filter_matches(&mut matches);
            calculate_emission_factors(year, &mut matches, &paths);
            generate_output(&mut matches, &paths);
        }
        None => panic!("Must specify a year to process or `preprocess <year>`."),
    }
}

#[derive(Debug, Deserialize)]
struct ManualMatch {
    generation: String,
    emission: String,
    settings: String,
    comment: String,
}

fn load_manual_matches(out: &mut Vec<Match>, paths: &FilePaths) {
    let mut manual_matches = Vec::new();
    let mut load_generation = BTreeMap::<String, Option<YearlyGeneration>>::new();
    let mut load_emission = BTreeMap::<String, Option<YearlyEmission>>::new();

    let mut csv_reader = load_csv_file(&paths.manual_matches_file(), ',');
    for result in csv_reader.deserialize() {
        let m: ManualMatch = result.expect("badly formated manual_matches.csv file!");

        load_generation.extend(m.generation.split('|').map(|name| (name.to_string(), None)));
        load_emission.extend(m.emission.split('|').map(|name| (name.to_string(), None)));

        manual_matches.push(m);
    }

    let mut csv_reader = load_csv_file(&paths.generation_file(), ',');
    for result in csv_reader.deserialize() {
        let csv_gen: YearlyGeneration = result.unwrap();

        let mut generation = load_generation.get_mut(&csv_gen.name);
        if generation.is_none() {
            generation = load_generation.get_mut(&format!("eic:{}", csv_gen.eic));
        }

        if let Some(generation) = generation {
            assert!(generation.is_none(), "Found two generation units: \"{}\"", csv_gen.name);
            *generation = Some(csv_gen);
        }
    }

    let mut csv_reader = load_csv_file(&paths.emissions_file(), ',');
    for result in csv_reader.deserialize() {
        let em: YearlyEmission = result.unwrap();

        let mut emission = load_emission.get_mut(&em.name);
        if emission.is_none() {
            emission = load_emission.get_mut(&format!("id:{}", em.id));
        }

        if let Some(emission) = emission {
            assert!(emission.is_none(), "Found two ETS emission records: \"{}\"", em.name);
            *emission = Some(em);
        }
    }

    for manual_match in manual_matches {
        if manual_match.emission.is_empty() && manual_match.generation.is_empty() {
            // so a CSV comment line can be inserted like ",,,DE"
            continue;
        }

        let mut m = Match::new(
            String::from("Manual Match"),
            manual_match
                .generation
                .split('|')
                .map(|name| {
                    load_generation
                        .remove(name)
                        .unwrap_or_else(|| panic!("generation \"{name}\" used more than once."))
                        .unwrap_or_else(|| panic!("generation \"{name}\" not found."))
                })
                .collect(),
            manual_match
                .emission
                .split('|')
                .filter(|name| !name.is_empty())
                .map(|name| {
                    load_emission
                        .remove(name)
                        .unwrap_or_else(|| panic!("emission \"{name}\" used more than once."))
                        .unwrap_or_else(|| panic!("emission \"{name}\" not found."))
                })
                .collect(),
        );

        if manual_match.emission.is_empty() {
            m.ignore(format!("filtered in manual_matches.csv: {}", manual_match.comment));
        }

        for (key, val) in manual_match
            .settings
            .split_terminator('|')
            .map(|s| s.split_once(':').unwrap_or_else(|| panic!("bad setting {s}")))
        {
            match key {
                "plausible-emission-factor-range" => {
                    let (min_s, max_s) = val.split_once('-').expect("bad emission factor range");
                    let min = min_s.parse().expect("bad minimum plausible emission factor");
                    let max = max_s.parse().expect("bad maximum plausible emission factor");
                    m.plausible_emission_factor = min..max;
                }
                _ => panic!("invalid setting {key}:{val}"),
            }
        }

        out.push(m);
    }
}

fn generate_auto_matches(matches: &mut Vec<Match>, paths: &FilePaths) {
    let manual_map_generation: BTreeSet<_> =
        matches.iter().flat_map(|m| &m.generation).map(|g| &g.name).collect();
    let manual_map_emission: BTreeSet<_> =
        matches.iter().flat_map(|m| &m.emission).map(|g| &g.name).collect();
    let manual_match_keys: BTreeSet<_> = manual_map_generation
        .iter()
        .chain(manual_map_emission.iter())
        .map(|name| get_key(name))
        .collect();

    let mut auto_matches = BTreeMap::<_, (Vec<YearlyGeneration>, Vec<YearlyEmission>)>::new();

    let mut csv_reader = load_csv_file(&paths.generation_file(), ',');
    for result in csv_reader.deserialize() {
        let csv_gen: YearlyGeneration = result.unwrap();

        if manual_map_generation.contains(&csv_gen.name) {
            continue;
        }

        let key = get_key(&csv_gen.name);
        auto_matches.entry((csv_gen.country.to_string(), key)).or_default().0.push(csv_gen);
    }

    let mut csv_reader = load_csv_file(&paths.emissions_file(), ',');
    for result in csv_reader.deserialize() {
        let em: YearlyEmission = result.unwrap();

        if manual_map_emission.contains(&em.name) {
            continue;
        }

        let key = get_key(&em.name);
        if !key.is_empty() {
            // some power stations use the "XI" country code in emissions data and "IE" in generation data
            let country = match em.country.as_str() {
                "XI" => "IE",
                other => other,
            };
            auto_matches.entry((country.to_string(), key)).and_modify(|m| m.1.push(em));
        }
    }

    matches.extend(auto_matches.into_iter().map(|((_, key), (generation, emission))| {
        let mut m = Match::new(key, generation, emission);

        if m.name.is_empty() {
            m.ignore("seems to be a meaningless generation unit name".to_string());
        } else {
            if m.emission.len() != 1 {
                m.ignore(format!("found {} possibly matching ETS records", m.emission.len()));
            }

            if manual_match_keys.contains(&m.name) {
                if m.emission.is_empty() {
                    println!("Possibly missing generation units for an existing manual match:");
                } else {
                    println!("Info: Detected similar automatic match in addition to manual match:");
                }
                println!("- {:?}\n- {:?}", m.generation, m.emission);
            }
        }

        m
    }));
}

fn get_key(name: &str) -> String {
    #[rustfmt::skip]
    const IGNORE_WORDS: &[&str] = &[
        "electrabel", // BE
        "elektrarn", // CZ
        "block", "dampf", "energie", "gud", "kraft", "turbine", // DE
        "generat", "power", "station", // EN
        "combinado", "electrica", "espana", "endesa", "generacion", "grupo", "iberdrola", // ES
        "voimalaitos", "lämpökeskus", // FI
        "electrique", // FR
        "limited", // GB
        "gazturbinas", "eromu", // HU
        "centrale", "energi", "termoelettrica", "turbogas", "combinato", "cogenera", // IT
        "vattenfall", // NL
        "cieplownia", "oddzial", "elektrowni", "energetyczny", "wytwarzanie", // PL
        "central", "termoelectrica", "termoeletrica", "termica", // RO
    ];

    deunicode(&name.to_lowercase())
        .split(|c: char| !c.is_alphabetic())
        .filter(|part| IGNORE_WORDS.iter().all(|i| !part.contains(i)))
        .filter(|part| part.len() >= 3)
        .max_by_key(|part| part.len())
        .unwrap_or_default()
        .to_string()
}

fn filter_matches(matches: &mut Vec<Match>) {
    matches.retain_mut(|m| {
        if m.fuel.as_deref() == Some("other") {
            // Remove match silently: not interesting for coal/oil/gas emission factors
            return false;
        } else if m.is_ignored() {
            // Ignore already filtered matches
        } else if m.generation_el == 0.0 {
            m.ignore("0 generation".to_string());
        } else if m.fuel.is_none() {
            m.ignore("uses mixed fuels".to_string());
        } else if m.emission.iter().map(|e| e.emissions).sum::<f64>() == 0.0 {
            m.ignore("0 emissions".to_string());
        }

        true
    });
}

fn calculate_emission_factors(year: u32, matches: &mut [Match], paths: &FilePaths) {
    // Heat emissions estimation. See README.md
    assert!(year >= 2020, "year < 2020 unsupported");

    let k: f64 = (year - 2020).into();
    let beta = 0.8782 - (k * 0.022); // This is valid for >= 2020
    let gamma = 0.3; // This is valid for >= 2020

    // https://climate.ec.europa.eu/system/files/2021-10/policy_ets_allowances_bm_curve_factsheets_en.pdf
    let heat_benchmark = match year {
        2013..=2020 => 62.3, // t CO2/TJ
        2021..=2025 => 47.3, // t CO2/TJ
        _ => panic!("year {year} is unsupported"),
    };

    let efficiency_heat = 0.8;
    let efficiency_el = 0.35;

    let mut degdays = BTreeMap::new();
    let mut csv_reader = load_csv_file(&paths.degree_days_file(), '\t');

    let latest_year: u32 = {
        let headers = csv_reader.headers().unwrap();
        headers.get(headers.len() - 1).unwrap().parse().unwrap()
    };

    if latest_year < year {
        println!("WARNING! Degree days database does not include data for year {year}.");
        println!("Ignoring this will lead to even worse data for combined heat and power plants.");
        println!("To continue anyway, press enter.");
        stdin().lock().lines().next().unwrap().unwrap();
    }

    for result in csv_reader.records() {
        let record = result.unwrap();
        let record_description = record.get(0).unwrap();

        let mut record_data = record_description.split(',').skip(2);
        if record_data.next().unwrap() != "HDD" {
            continue;
        }

        let country = match record_data.next().unwrap() {
            "EL" => "GR".to_string(), // Greece is Ελλάς / Ellás in this dataset
            other => other.to_string(),
        };

        degdays.insert(
            country,
            (2014..=latest_year)
                .map(|year| record.get((year - 1979) as usize + 1).unwrap().parse().unwrap())
                .collect::<Vec<f64>>(),
        );
    }

    for m in matches.iter_mut().filter(|m| !m.is_ignored()) {
        // average 2014-2018
        let baseline_degdays = degdays.get(&m.country).unwrap().iter().take(5).sum::<f64>() / 5.0;
        let current_degdays =
            degdays.get(&m.country).unwrap().get(year as usize - 2014).unwrap_or(&baseline_degdays);

        let allocation_sum: f64 = m.emission.iter().map(|g| g.allocations).sum();
        let emission_sum: f64 = m.emission.iter().map(|g| g.emissions).sum();

        // Heat provision can be privileged (some industry types) or non-privileged (e.g. district
        // heating). For privileged heat provided by a power plant, free allocation of ETS
        // allowances according to the heat benchmark is granted, reduced only by the linear
        // reduction factor (beta). Allocations for unprivileged heat are reduced further using
        // the carbon leakage exposure factor (gamma). Sigma is the share of privileged heat
        // provided by a power plant.
        let alloc_privileged = m.sigma * allocation_sum;
        let alloc_nonpriv = (1.0 - m.sigma) * allocation_sum;

        // "preliminary allocation" = allocation before any reduction factors (beta/gamma) are
        // applied, so just the result of the ETS heat benchmark.
        let prelim_privileged = alloc_privileged / beta;
        let prelim_nonpriv = alloc_nonpriv / (beta * gamma);

        // Non-privileged heat is mostly district heating. Since the allocation is based on the
        // historical average (2014-2018) of heat provided, scale the preliminary allocation
        // according to current year's winter temperatures (= heating degree days).
        let scaled_nonpriv = prelim_nonpriv * (current_degdays / baseline_degdays);

        // 277 MWh in one TJ
        m.generation_heat = (scaled_nonpriv + prelim_privileged) / (heat_benchmark / 277.777777);

        // "Efficiency method" as described in
        // https://ghgprotocol.org/sites/default/files/2023-03/CHP_guidance_v1.0.pdf
        let share_heat = m.generation_heat / efficiency_heat;
        let share_el = m.generation_el / efficiency_el;
        m.emissions_heat = emission_sum * share_heat / (share_heat + share_el);

        m.emissions_el = emission_sum - m.emissions_heat;
        m.emission_factor = (m.emissions_el * 1000.0) / m.generation_el;

        if !m.plausible_emission_factor.contains(&m.emission_factor) {
            m.ignore("emission factor seems implausible".to_string());
        }
    }
}

#[derive(Default, Serialize)]
struct FuelStats {
    country: String,
    fuel: String,
    total_generation: f64,
    matched_generation: f64,
    coverage_percentage: f64,
    emissions_el: f64,
    emissions_heat: f64,
    emission_factor: Option<f64>,
}

impl FuelStats {
    fn add_match(&mut self, m: &Match) {
        self.matched_generation += m.generation_el;
        self.emissions_el += m.emissions_el;
        self.emissions_heat += m.emissions_heat;
    }

    fn add_stat(&mut self, other: &Self) {
        self.total_generation += other.total_generation;
        self.matched_generation += other.matched_generation;
        self.emissions_el += other.emissions_el;
        self.emissions_heat += other.emissions_heat;
    }
}

fn generate_output(matches: &mut [Match], paths: &FilePaths) {
    matches.sort_unstable_by(|x, y| {
        let cmp_criteria_x = (&x.country, &x.name, &x.generation.first().map(|g| &g.name));
        let cmp_criteria_y = (&y.country, &y.name, &y.generation.first().map(|g| &g.name));
        cmp_criteria_x.cmp(&cmp_criteria_y)
    });

    // Write powerplant-level data
    let mut plants_writer = Writer::from_path(paths.out_powerplants_file()).unwrap();
    let mut ignored_writer = Writer::from_path(paths.ignored_powerplants_file()).unwrap();
    for m in matches.iter() {
        if m.is_ignored() {
            ignored_writer.serialize(m).unwrap();
        } else {
            plants_writer.serialize(m).unwrap();
        }
    }
    plants_writer.flush().unwrap();
    ignored_writer.flush().unwrap();

    // Generate country-level stats
    let mut fuel_stats = BTreeMap::<_, FuelStats>::new();

    // sum up all relevant generation per country and fuel (even unmatched), to calculate coverage
    let mut csv_reader = load_csv_file(&paths.generation_file(), ',');
    for result in csv_reader.deserialize() {
        let csv_gen: YearlyGeneration = result.unwrap();

        if csv_gen.fuel != "other" {
            let key = (csv_gen.country.to_string(), csv_gen.fuel.to_string());
            fuel_stats.entry(key).or_default().total_generation += csv_gen.output;
        }
    }

    // include all valid matches in country-level stats
    for m in matches.iter().filter(|m| !m.is_ignored()) {
        let key = (m.country.to_string(), m.fuel.as_ref().unwrap().to_string());
        fuel_stats.get_mut(&key).unwrap().add_match(m);
    }

    // create additional stats for country "" (sums up all countries)
    let mut all_country_stats = BTreeMap::<_, FuelStats>::new();
    for ((_, fuel), stat) in &fuel_stats {
        all_country_stats.entry(("".to_string(), fuel.clone())).or_default().add_stat(stat);
    }
    fuel_stats.append(&mut all_country_stats);

    // create additional stats for fuel "coal+lignite" (sums both, like in electricitymaps)
    let mut coal_lignite_stats = BTreeMap::<_, FuelStats>::new();
    for ((country, fuel), stat) in &fuel_stats {
        if fuel == "coal" || fuel == "lignite" {
            let key = (country.clone(), "coal+lignite".to_string());
            coal_lignite_stats.entry(key).or_default().add_stat(stat);
        }
    }
    fuel_stats.append(&mut coal_lignite_stats);

    // write country-level stats
    let mut csv_writer = Writer::from_path(paths.out_countries_file()).unwrap();
    for ((country, fuel), mut stat) in fuel_stats {
        stat.country = country;
        stat.fuel = fuel;
        stat.coverage_percentage = if stat.total_generation > 0.0 {
            (100.0 * stat.matched_generation) / stat.total_generation
        } else {
            100.0
        };

        if stat.matched_generation > 0.0 {
            stat.emission_factor = Some((1000.0 * stat.emissions_el) / stat.matched_generation);
        }

        csv_writer.serialize(stat).unwrap();
    }
    csv_writer.flush().unwrap();
}

fn load_csv_file(path: &Path, separator: char) -> Reader<File> {
    ReaderBuilder::new()
        .delimiter(separator as u8)
        .trim(Trim::All)
        .from_path(path)
        .unwrap_or_else(|e| panic!("Failed to load {path:?}: {e:?}"))
}
