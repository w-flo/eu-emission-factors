use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
};

use calamine::{Reader, Xlsx};
use csv::{ReaderBuilder, Writer};
use zip::ZipArchive;

pub(crate) use crate::FilePaths;
use crate::{YearlyEmission, YearlyGeneration};

pub(crate) fn yearly_emissions(year: u32, paths: &FilePaths) -> BTreeSet<String> {
    let mut excel: Xlsx<_> = calamine::open_workbook(paths.verified_emissions_file()).unwrap();
    let worksheets = excel.worksheets();
    let (_, sheet) = worksheets.first().unwrap();

    let header_row = (0..100)
        .find(|&row| sheet.get((row, 0)).unwrap().get_string() == Some("REGISTRY_CODE"))
        .unwrap();

    let name_col = find_col(sheet, header_row, "INSTALLATION_NAME");
    let permit_id_col = find_col(sheet, header_row, "PERMIT_IDENTIFIER");
    let installation_id_col = find_col(sheet, header_row, "INSTALLATION_IDENTIFIER");
    let activity_type = find_col(sheet, header_row, "MAIN_ACTIVITY_TYPE_CODE");
    let emissions_col = find_col(sheet, header_row, &format!("VERIFIED_EMISSIONS_{year}"));
    let allocations_col = find_col(sheet, header_row, &format!("ALLOCATION_{year}"));
    let allocations_2018_col = find_col(sheet, header_row, "ALLOCATION_2018");
    let allocations_2019_col = find_col(sheet, header_row, "ALLOCATION_2019");

    let mut countries = BTreeSet::new();
    let mut pp_emissions = Vec::new();

    for row in (header_row + 1)..sheet.height() {
        let activity_type = get_float(sheet, row, activity_type) as i32;
        let emissions = get_float(sheet, row, emissions_col).max(0.0);
        let allocations = get_float(sheet, row, allocations_col).max(0.0);

        if activity_type == 20 || activity_type == 1 {
            let country = sheet.get((row, 0)).unwrap().to_string();
            if country == "GB" {
                // They no longer report data to ETS, only old data available
                continue;
            }

            let permit_id = sheet.get((row, permit_id_col)).unwrap().to_string();
            let installation_id = sheet.get((row, installation_id_col)).unwrap().to_string();
            let name = sheet.get((row, name_col)).unwrap().to_string();

            // Based on https://github.com/INATECH-CIG/CO2_emissions_factors_DE
            let sigma = if allocations > 0.0 {
                let allocations_2018 = get_float(sheet, row, allocations_2018_col).max(0.0);
                let allocations_2019 = get_float(sheet, row, allocations_2019_col).max(0.0);

                if allocations_2018 == 0.0 || allocations_2019 == 0.0 {
                    0.0
                } else {
                    let beta_2018 = 1.0 - 0.0174 * f64::from(2018 - 2013);
                    let beta_2019 = 1.0 - 0.0174 * f64::from(2019 - 2013);
                    let gamma_2018 = 0.8 - (0.5 / 7.0) * f64::from(2018 - 2013);
                    let gamma_2019 = 0.8 - (0.5 / 7.0) * f64::from(2019 - 2013);
                    let raw_sigma = (allocations_2018 * beta_2019 * gamma_2019
                        - allocations_2019 * beta_2018 * gamma_2018)
                        / (allocations_2019 * beta_2018 * (1.0 - gamma_2018)
                            - allocations_2018 * beta_2019 * (1.0 - gamma_2019));
                    raw_sigma.clamp(0.0, 1.0)
                }
            } else {
                0.0
            };

            countries.insert(country.clone());
            pp_emissions.push(YearlyEmission {
                country,
                name: name.to_string(),
                id: format!("{permit_id}:{installation_id}"),
                emissions,
                allocations,
                sigma,
            });
        }
    }

    let mut csv_writer = Writer::from_path(paths.emissions_file()).unwrap();
    for emission in pp_emissions {
        csv_writer.serialize(emission).unwrap();
    }
    csv_writer.flush().unwrap();

    countries
}

fn find_col(sheet: &calamine::Range<calamine::DataType>, row: usize, val: &str) -> usize {
    (0..1000).find(|&col| sheet.get((row, col)).unwrap().get_string() == Some(val)).unwrap()
}

fn get_float(sheet: &calamine::Range<calamine::DataType>, row: usize, col: usize) -> f64 {
    sheet.get((row, col)).unwrap().get_float().unwrap_or(0.0)
}

#[derive(Debug, serde::Deserialize)]
struct UnitGenerationHour {
    #[serde(rename = "ResolutionCode")]
    resolution_code: String,
    #[serde(rename = "PowerSystemResourceName")]
    name: String,
    #[serde(rename = "MapCode")]
    map_code: String,
    #[serde(rename = "GenerationUnitEIC")]
    eic: String,
    #[serde(rename = "ProductionType")]
    unit_type: String,
    #[serde(rename = "ActualGenerationOutput")]
    output: Option<f64>,
    #[serde(rename = "ActualConsumption")]
    consumption: Option<f64>,
}

pub(crate) fn yearly_generation(countries: &BTreeSet<String>, paths: &FilePaths) {
    let mut units = BTreeMap::<String, YearlyGeneration>::new();

    for month in 1..=12 {
        let zip_name = paths.entso_e_zip_file(month);
        println!("Loading {zip_name:?}...");

        let mut zip = ZipArchive::new(File::open(zip_name).unwrap()).unwrap();
        let file = zip.by_index(0).unwrap();

        let mut csv_reader = ReaderBuilder::new().delimiter(b'\t').from_reader(file);
        for result in csv_reader.deserialize() {
            let generation_hour: UnitGenerationHour = result.unwrap();

            let country = generation_hour.map_code.split('_').next().unwrap().to_string();
            if !countries.contains(&country) {
                continue;
            }

            let fuel = match generation_hour.unit_type.as_str() {
                "Fossil Gas" => "gas",
                "Fossil Hard coal" => "coal",
                "Fossil Brown coal/Lignite" => "lignite",
                "Fossil Oil" => "oil",
                "Nuclear" => continue,
                "Hydro Pumped Storage" | "Hydro Water Reservoir" => continue,
                "Hydro Run-of-river and poundage" => continue,
                "Solar" | "Wind Onshore" | "Wind Offshore" => continue,
                _ => "other",
            }
            .to_string();

            let divide_by = match generation_hour.resolution_code.as_str() {
                "PT60M" => 1.0,
                "PT30M" => 2.0,
                "PT15M" => 4.0,
                code => panic!("unknown resolution code {code}"),
            };

            let unit = units.entry(generation_hour.eic).or_insert_with(|| YearlyGeneration {
                country,
                name: generation_hour.name,
                eic: String::new(),
                fuel,
                output: 0.0,
            });
            unit.output += generation_hour.output.unwrap_or_default() / divide_by;
            unit.output -= generation_hour.consumption.unwrap_or_default() / divide_by;
        }
    }

    let mut csv_writer = Writer::from_path(paths.generation_file()).unwrap();
    for (unit_eic, mut unit) in units {
        unit.eic = unit_eic;
        csv_writer.serialize(unit).unwrap();
    }
    csv_writer.flush().unwrap();
}
