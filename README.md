# EU power plant emission factor estimation

This is a reimplementation of the [electricity
maps](https://app.electricitymaps.com/map) EU emission factors estimation method
from [this python
script](https://colab.research.google.com/drive/1buA-yHQCF711F53IX7_GAxYMJ9p4qJoT?usp=sharing),
which is similar to the method described by Unnewehr et al. [1]. In comparison
to the python script, it is supposed to enable easier manual matching of
generation units with their respective ETS emission data, and hopefully
calculate heat emissions a bit better (see below for a description).

Example output of this tool is in "data/[year]/output" of this repository. Of
course, since Entso-E regularly publishes corrections for old data, output will
most likely differ a bit when downloading currently published Entso-E data and
running the tool.

## Preprocessing

To start preprocessing data for one given year, some input data is needed:

 - The "data/[year]/entsoe_unit_generation" directory needs to contain raw
   Entso-E unit generation data for the year in question, namely 12 zip files
   "[year]_[month]_ActualGenerationOutputPerGenerationUnit_16.1.A.zip". These
   can be downloaded from Entso-E sftp server after registration, from the
   server's "zip" directory.
 - The "data/verified_ets_emissions" directory needs to contain the ETS
   "verified emissions" xlsx file. It needs to be renamed to
   "verified_emissions.xlsx", and can be downloaded from [EU
   ETS](https://climate.ec.europa.eu/eu-action/eu-emissions-trading-system-eu-ets/union-registry_en),
   in "Documentation -> Phase [I-IV] -> Reports -> Verified Emissions for
   [year]". Files from previous years can probably be used as long as they
   include data for the year in question, but newer files may include
   corrections for past years so it is recommended to always use the most
   recently released file (releases seem to happen in April).

After the 13 files are placed inside the corresponding directories, data
preprocessing can start: `cargo run --release -- preprocess <year>`. This will
process the raw data and create two csv files in "data/[year]/preprocessed", one
for electricity generation per generation unit, and one for ETS emissions and
free-of-charge ETS allocations in the given year.

Note that raw input data does not appear to be available under a free license,
so the generated csv files in the "preprocessed" dir may not be freely
redistributable, as they're basically just copies / aggregated values of parts
of the original "raw" files.

## Degree days database

In addition to data from the preprocessing step, the "Energy statistics -
cooling and heating degree days (nrg_chdd)" database from Eurostat is needed in
the "data/degree_days" directory. It's a file called nrg_chdd_a.tsv. The file is
freely redistributable, can be downloaded from [the Eurostat energy
database](https://ec.europa.eu/eurostat/web/energy/database), and includes data
from past years, so the most recent release is recommended.

## Matching generation units with their emission data

The generation and emissions data sets have no common identifier for generation
units / power plants, so generation units need to be matched with their
respective emissions data in order to estimate emission factors. An automatic
matching is attempted based on unit names. In addition, a file
"data/[year]/manual_matches.csv" needs to be created for each year, although
they can be largely the same between different years with only minor updates.
The file can be used to manually map electricity generation data to emission
data. The file should look like this, with each line defining one match:

```
generation,emission,settings,comment
Irsching 4,Kraftwerk Irsching Block 4,,
GKM AG DBEnergie|GKM AG TNG|GKM AG Amprion,Grosskraftwerk Mannheim,,
,,,This is a general comment
IKS Schwedt SE1 Block 1|IKS Schwedt SE2 Block 2,,,"only combined refinery + power plant ETS data is available"
eic:11WD2HANN5C---1X|eic:11WD2HANN5C---2V,GKH - Gemeinschaftskraftwerk Hannover GmbH,,
Plock B01,id:PL-0391-05:455,,
Rya KVV,Rya Kraftvärmeverk,plausible-emission-factor-range:200-600,"uses some biomass"
```

The `generation` field lists one or more generation unit names from the
"data/[year]/preprocessed/powerplant_generation.csv" file, separated by "|". The
`emission` field lists zero or more ETS data installation names from the
"data/[year]/preprocessed/powerplant_emissions.csv" file, also separated by "|".

The `settings` field may contain some settings to override default behavior,
separated by "|". Currently supported:

- `plausible-emssions-factor-range:[min]-[max]`. Default is `300-3000`. If the
  emission factor calculation result for this plant is outside of the given
  range, the plant is ignored during country-level data aggregation.

The optional `comment` field may contain some text to explain the line. If the
`generation` and `emission` fields are empty, the line is ignored – so that's a
way to add general comments to the file.

In the example above, the "IKS Schwedt" line has an empty `emission` field. This
causes the generation unit(s) of that line to be ignored in the emission factors
calculation. No automatic matching to emissions data will be attempted for these
generation units.

Sometimes names for generation units or emissions data sets are too generic. For
example, the Hannover power plant consists of two blocks, and their generation
unit names in the Entso-E dataset are "Block 1" and "Block 2", respectively. In
a case like this, the generation unit should be specified as "eic:[EIC of the
generation unit]" as demonstrated above. Similarly, generic ETS record names
like "ELEKTROCIEPŁOWNIA" for the Plock power plant (and other power plants in
Poland) should be specified as "id:[permit_id]:[installation_id]". The value to
append after "eic:" or "id:" is listed in the "powerplant_generation.csv" and
"powerplant_emissions.csv" files, respectively.

### How to find a manual match

Some matches are difficult to figure out, but this process usually works:
Geocoordinates of generation units can be found based on their name and/or EIC
using a search engine. Next, find the matching ETS installation on the map at
[euets.info](https://www.euets.info/installations). The installation name is
displayed in a tooltip when hovering the mouse cursor over over the dot on the
map. If the installation name is generic and there are multiple installations
with that name, clicking the dot will show details about verified emissions and
allocations per year, which can be used to find the correct match and reference
it via "id:[permit_id]:[installation_id]".

## Estimating emission factors

Once preprocessing is done and a "manual_matches.csv" was created, emission
factors can be estimated: `cargo run --release -- <year>`. This will create
three csv files in "data/[year]/output":

 1. The estimated emission factors and related data for all relevant power
    plants that were successfuly matched to emissions data and passed some
    plausibility testing.
 2. Ignored power plants along with the reason why they were ignored. Depending
    on what exactly went wrong, it's often possible to fix this by adding a line
    to manual_matches.csv.
 3. Emission factors and other data aggregated at the country level, grouped by
    fuel type.

### Combined heat and power

An attempt is made to estimate emissions caused by heat production, based on the
number of free-of-charge ETS allocations. The basic idea is described by
Unnewehr et al. [1, p. 5] and was previously used by Hermann et al. [2, p. 151].
These heat-related emissions are subtracted from the total emissions of each
plant before calculating the emission factor for electricity generation. Details
are described below.

**Currently, the code only handles years 2020-2025.**

#### Free-of-charge ETS allocations

Power plants receive [free-of-charge ETS emission
allowances](https://climate.ec.europa.eu/eu-action/eu-emissions-trading-system-eu-ets/free-allocation_en)
if they provide heat for district heating or industry. This process is called
"(free) allocations". The total number of allocations (corresponding to metric
tons of CO2) per year for each power plant is published by ETS, along with the
verified emissions for each year. The number of allocations depends on the
yearly average of heat provided by the power plant in the "baseline period"
2014-2018. This baseline is multiplied by the "heat benchmark" number specified
by the EU to calculate the so-called "preliminary allocation". The preliminary
allocation is reduced by up to two different factors to calculate the actual
allocations:

 - For "privileged" and "non-privileged" heat: Multiplying by the "linear
   reduction factor", which was 0.8782 in 2020 and is reduced by 0.022 every
   year. This is the "beta" factor in this code base.
 - For "non-privileged" heat only: Multiplying by the "carbon leakage exposure
   factor", which was fixed to 0.3 for the 2020-2025 time period. This is the
   "gamma" factor in this code base.

If some industry is determined to be at risk of shutting down because of high
ETS/CO2 costs, and moving production outside of the EU, that is called "risk of
carbon leakage". The factor of 0.3 is not applied for these "high carbon leakage
risk" use cases, so these industries receive more free-of-charge allocations and
hopefully stay competitive globally.

#### Estimating a power plant's share of heat at risk of "carbon leakage" (sigma)

To determine the amount of heat provided by combined heat and power (CHP) plants
from the number of free-of-charge ETS allocations, the share of "privileged" vs.
"non-privileged" heat provided by each CHP plant needs to be calculated first.
This happens in the preprocessing step. The method is like the one used in the
source code of [3], but only years 2018 and 2019 are considered. The resulting
"sigma" value is the share of "privileged" heat (at high risk of carbon leakage)
provided by the CHP plant. For example, a value of sigma=0 might indicate that
the CHP plant only provides heat for district heating, a very common situation,
which is not at risk of carbon leakage. A value of sigma=0.5 might indicate that
half of the heat provided by the plant is delivered to industry at risk of
carbon leakage, and the other half is used for district heating.

The sigma calculation is possible from free-of-charge allocations data pre-2020.
In that time period, heat provision had the "linear reduction factor" (beta)
applied to its preliminary allocation like today (using a slightly different
formula). However, the gamma factor was not fixed to 0.3, but was going from 1.0
to 0.3 over a few years. So if a power plant had its number of yearly
allocations decrease only very slightly over the years, that indicates
"privileged heat", so sigma=1.0. For power plants where the number of
allocations decreased rapidly every year, that indicates sigma=0. By observing
the reduction of the number of allocations from 2018 to 2019, a sigma value is
estimated for every relevant ETS record in the preprocessing step [3, source
code].

It is no longer possible to estimate sigma from 2020+ data, because the gamma
factor was fixed to 0.3 for "non-privileged" heat and is still fixed to 1.0 for
"privileged" heat. So these estimates might be out of date for some power
plants, resulting in incorrect heat calculations.

#### Estimating the amount of heat provided by a CHP plant

Knowing the number of free-of-charge allocations and estimated sigma for each
plant, the procedure described above for calculating allocations is applied in
reverse to get the estimated number of preliminary allocations. Using the ETS
heat benchmark, the yearly average for heat provided by each power plant in the
baseline period 2014-2018 is calculated from the number of its estimated
preliminary allocations. To account for colder/warmer winter temperatures in the
current year, the "heating degree days" of each country (as provided by
Eurostat) for the baseline period vs. the current year are used to scale the
heat provided in the baseline period up or down to estimate the amount of heat
provided in the current year.

This is obviously not a great estimation for the actual amount of heat provided
in a given year. The CHP plant might have been offline for extended maintenance
/ upgrades / fuel switch / ... in a given year, reducing the amount of heat
provided. And homes might have been upgraded with better insulation, reducing
the need for heat from the CHP plant. Local heat-consuming industry might have
shut down since the baseline period. And so on. However, it might be the best
estimation possible.

#### Splitting emissions between heat and electricity generation

Finally, knowing the amount of heat and electricity generated in the current
year as well as the total verified emissions, the emissions are split between
electricity generation and heat generation using the "efficiency method" [4, p.
6]. Currently, efficiency for heat generation is assumed to be 80% and power
generation 35%, as recommended for US CHP plants [4, p. 9].

## References

[1]: [J. F. Unnewehr, A. Weidlich, L. Gfüllner and M. Schäfer, "Open-data based
    carbon emission intensity signals for electricity generation in European
    countries – top down vs. bottom up approach," *Cleaner Energy Systems*, vol.
    3, 2022, doi:
    10.1016/j.cles.2022.100018](https://doi.org/10.1016/j.cles.2022.100018).\
[2]: [H. Hermann, F. Matthes and V. Cook, "Die deutsche Braunkohlenwirtschaft.
    Historische Entwicklungen, Ressourcen, Technik, wirtschaftliche Strukturen
    und Umweltauswirkungen", May
    2017](https://www.oeko.de/publikationen/p-details/die-deutsche-braunkohlenwirtschaft-historische-entwicklungen-ressourcen-technik-wirtschaftliche).\
[3]: [T. N. Schubert, G. Avenmarg, J. F. Unnewehr and M. Schäfer, "Generation
    and emission data for main power plants in Germany (2015 - 2021)," *Zenodo*,
    2022, doi: 10.5281/zenodo.7316186](https://doi.org/10.5281/zenodo.7316186).\
[4]: ["Allocation of GHG Emissions from a Combined Heat and Power (CHP) Plant."
    GHG Protocol.
    https://ghgprotocol.org/sites/default/files/2023-03/CHP_guidance_v1.0.pdf
    (accessed Jun. 5,
    2023)](https://ghgprotocol.org/sites/default/files/2023-03/CHP_guidance_v1.0.pdf).

## License

Licensed under either of

 * Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
   http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or
   http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.