use std::collections::BTreeMap;

use serde::Serialize;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct YearlyEmission {
    pub(crate) country: String,
    pub(crate) name: String,
    pub(crate) id: String,
    pub(crate) emissions: f64,
    pub(crate) allocations: f64,
    pub(crate) sigma: f64,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct YearlyGeneration {
    pub(crate) country: String,
    pub(crate) name: String,
    pub(crate) eic: String,
    pub(crate) fuel: String,
    pub(crate) output: f64,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct Match {
    pub(crate) country: String,
    pub(crate) name: String,
    #[serde(serialize_with = "join_vec")]
    pub(crate) generation: Vec<YearlyGeneration>,
    #[serde(serialize_with = "join_vec")]
    pub(crate) emission: Vec<YearlyEmission>,

    #[serde(skip_serializing_if = "Option::is_none")]
    ignore_reason: Option<String>,

    pub(crate) fuel: Option<String>,
    pub(crate) sigma: f64,
    pub(crate) generation_el: f64,
    pub(crate) generation_heat: f64,
    pub(crate) emissions_heat: f64,
    pub(crate) emissions_el: f64,
    pub(crate) emission_factor: f64,
}

impl Match {
    pub(crate) fn new(
        name: String,
        generation: Vec<YearlyGeneration>,
        emission: Vec<YearlyEmission>,
    ) -> Self {
        let mut output_sum = 0.0;
        let mut fuel_mix = BTreeMap::<_, f64>::new();
        for generation in &generation {
            *fuel_mix.entry(generation.fuel.as_str()).or_default() += generation.output;
            output_sum += generation.output;
        }

        let fuel = fuel_mix
            .into_iter()
            .filter(|(_, out)| *out > 0.95 * output_sum)
            .map(|(fuel, _)| fuel.to_string())
            .next();

        let privileged_allocs = emission.iter().map(|e| e.sigma * e.allocations).sum::<f64>();
        let sigma = if privileged_allocs == 0.0 {
            0.0
        } else {
            privileged_allocs / emission.iter().map(|e| e.allocations).sum::<f64>()
        };

        Self {
            country: generation.first().unwrap().country.clone(),
            name,
            generation,
            emission,
            fuel,
            sigma,
            generation_el: output_sum,
            ..Default::default()
        }
    }

    pub(crate) fn ignore(&mut self, reason: String) {
        assert!(self.ignore_reason.is_none());
        self.ignore_reason = Some(reason);
    }

    pub(crate) fn is_ignored(&self) -> bool {
        self.ignore_reason.is_some()
    }
}

fn join_vec<S>(vec: &[impl AsRef<str>], serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&vec.iter().map(AsRef::as_ref).collect::<Vec<_>>().join("|"))
}

impl AsRef<str> for YearlyGeneration {
    fn as_ref(&self) -> &str {
        &self.name
    }
}

impl AsRef<str> for YearlyEmission {
    fn as_ref(&self) -> &str {
        &self.name
    }
}
