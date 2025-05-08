use std::path::PathBuf;

pub(crate) struct FilePaths {
    data_dir: PathBuf,
    year_dir: PathBuf,
    year: u32,
}

impl FilePaths {
    pub(crate) fn new(data_dir: PathBuf, year: u32) -> Self {
        let data = data_dir.display();
        let year_dir = PathBuf::from(format!("{data}/{year}"));

        std::fs::create_dir_all(year_dir.join("preprocessed")).unwrap();
        std::fs::create_dir_all(year_dir.join("output")).unwrap();

        Self { data_dir, year_dir, year }
    }

    pub(crate) fn degree_days_file(&self) -> PathBuf {
        self.data_dir.join("degree_days/nrg_chdd_a.tsv")
    }

    pub(crate) fn emissions_file(&self) -> PathBuf {
        self.year_dir.join("preprocessed/powerplant_emissions.csv")
    }

    pub(crate) fn entso_e_zip_file(&self, month: u8) -> PathBuf {
        let year = self.year;
        let mut path = self.year_dir.join("entsoe_unit_generation");

        let new =
            format!("{year}_{month:02}_ActualGenerationOutputPerGenerationUnit_16.1.A_r2.1.zip");
        path.push(new);
        if path.exists() {
            return path;
        }

        let old = format!("{year}_{month:02}_ActualGenerationOutputPerGenerationUnit_16.1.A.zip");
        path.set_file_name(old);
        if path.exists() {
            return path;
        }

        panic!("File not found: {path:?}");
    }

    pub(crate) fn generation_file(&self) -> PathBuf {
        self.year_dir.join("preprocessed/powerplant_generation.csv")
    }

    pub(crate) fn manual_matches_file(&self) -> PathBuf {
        self.year_dir.join("manual_matches.csv")
    }

    pub(crate) fn out_powerplants_file(&self) -> PathBuf {
        self.year_dir.join("output/powerplants.csv")
    }

    pub(crate) fn ignored_powerplants_file(&self) -> PathBuf {
        self.year_dir.join("output/ignored_powerplants.csv")
    }

    pub(crate) fn out_countries_file(&self) -> PathBuf {
        self.year_dir.join("output/countries.csv")
    }

    pub(crate) fn verified_emissions_file(&self) -> PathBuf {
        self.data_dir.join("verified_ets_emissions/verified_emissions.xlsx")
    }
}
