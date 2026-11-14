//! `cargo xtask capacity`: each unit's net summer capacity for 1999 to 2006 from EIA-860, the years
//! before `NUC_STATUS` (DESIGN.md Decision 22). Those files are frozen history in three formats, so
//! they're read once here into `catalog/capacity.csv` rather than at every rebuild.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{Cursor, Read as _};
use std::path::Path;

use anyhow::{Context as _, bail, ensure};
use calamine::{Data, Reader as _, Xls};

use downpower_ingest::units::{EiaGenerator, Units};

const FIRST_YEAR: i32 = 1999;
const LAST_YEAR: i32 = 2006;

/// One nuclear generator's rating in one year's file.
#[derive(Debug)]
struct Rating {
    generator: EiaGenerator,
    summer_mw: f64,
    status: String,
}

/// The zip, the file inside it, and the columns that hold each field, per year. The 1999 and 2000
/// utility files rate in kW and mark nuclear `UR` or `NUC`; later files rate in MW.
struct Layout {
    zip: String,
    member: String,
    plant: &'static str,
    generator: &'static str,
    summer: &'static str,
    source: &'static str,
    status: &'static str,
    per_mw: f64,
}

fn layout(year: i32) -> Layout {
    let yy = year % 100;
    match year {
        1999 | 2000 => Layout {
            zip: format!("eia860a{year}.zip"),
            member: format!("ExistingGenerators{year}.xls"),
            plant: "PLANT_CODE",
            generator: "GENERATOR_ID",
            summer: "EXISTING_SUMMER_CAPABILITY",
            source: "EXISTING_ENERGY_SOURCE_1",
            status: "EXISTING_STATUS",
            per_mw: 1000.0,
        },
        2001..=2003 => Layout {
            zip: format!("eia860{year}.zip"),
            member: format!("GENY{yy:02}.dbf"),
            plant: "PLNTCODE",
            generator: "GENCODE",
            summer: "SUMMCAP",
            source: "ENSOURCE1",
            status: "STATUS",
            per_mw: 1.0,
        },
        _ => Layout {
            zip: format!("eia860{year}.zip"),
            member: format!("GenY{yy:02}.xls"),
            plant: "PLNTCODE",
            generator: "GENCODE",
            summer: "SUMMCAP",
            source: "ENERGY_SOURCE_1",
            status: "STATUS",
            per_mw: 1.0,
        },
    }
}

/// The member of `zip` named `name`, ignoring case.
fn member(zip: &Path, name: &str) -> anyhow::Result<Vec<u8>> {
    let mut archive = zip::ZipArchive::new(File::open(zip).with_context(|| format!("opening {}", zip.display()))?)?;
    let index = (0..archive.len())
        .find(|&index| archive.name_for_index(index).is_some_and(|member| member.is_ok_and(|member| member.eq_ignore_ascii_case(name))))
        .with_context(|| format!("{} has no {name}", zip.display()))?;
    let mut bytes = Vec::new();
    archive.by_index(index)?.read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// A spreadsheet or dBase cell as text, trimmed.
fn text(cell: &Data) -> String {
    match cell {
        Data::Float(value) if value.fract() == 0.0 => format!("{value:.0}"),
        other => other.to_string().trim().to_owned(),
    }
}

/// Every row of the year's generator table, as column → text.
fn rows(dir: &Path, year: i32, layout: &Layout) -> anyhow::Result<Vec<BTreeMap<String, String>>> {
    let bytes = member(&dir.join(&layout.zip), &layout.member)?;
    if layout.member.to_ascii_lowercase().ends_with(".dbf") {
        let mut reader = dbase::Reader::new(Cursor::new(bytes)).with_context(|| format!("reading {}", layout.member))?;
        let mut rows = Vec::new();
        for record in reader.iter_records() {
            let record = record.with_context(|| format!("a record of {}", layout.member))?;
            let row = record
                .into_iter()
                .map(|(field, value)| {
                    let value = match value {
                        dbase::FieldValue::Character(text) => text.unwrap_or_default().trim().to_owned(),
                        dbase::FieldValue::Numeric(number) => number.map(|number| format!("{number}")).unwrap_or_default(),
                        other => format!("{other:?}"),
                    };
                    (field, value)
                })
                .collect();
            rows.push(row);
        }
        return Ok(rows);
    }
    let mut workbook: Xls<_> = Xls::new(Cursor::new(bytes)).with_context(|| format!("reading {}", layout.member))?;
    // The utility files hold proposed changes on another sheet; the existing generators are on
    // their own.
    let sheet = workbook
        .sheet_names()
        .into_iter()
        .find(|sheet| year > 2000 || sheet == "Existing Generators")
        .with_context(|| format!("{} has no generator sheet", layout.member))?;
    let range = workbook.worksheet_range(&sheet).with_context(|| format!("{} sheet {sheet}", layout.member))?;
    let mut lines = range.rows();
    let header: Vec<String> = lines.next().context("an empty sheet")?.iter().map(text).collect();
    Ok(lines.map(|line| header.iter().cloned().zip(line.iter().map(text)).collect()).collect())
}

/// The year's nuclear generators and their summer ratings.
fn ratings(dir: &Path, year: i32) -> anyhow::Result<Vec<Rating>> {
    let layout = layout(year);
    let mut ratings = Vec::new();
    for row in rows(dir, year, &layout)? {
        let column = |name: &str| row.get(name).map(String::as_str).with_context(|| format!("{year}: no {name} column"));
        if !matches!(column(layout.source)?, "NUC" | "UR") {
            continue;
        }
        let plant: f64 = column(layout.plant)?.parse().with_context(|| format!("{year}: plant {:?}", column(layout.plant)))?;
        let summer: f64 = column(layout.summer)?.parse().with_context(|| format!("{year}: summer capacity {:?}", column(layout.summer)))?;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "EIA plant codes are small whole numbers")]
        let plant = plant as u32;
        ratings.push(Rating {
            generator: EiaGenerator {
                plant,
                generator: column(layout.generator)?.to_owned(),
            },
            summer_mw: summer / layout.per_mw,
            status: column(layout.status)?.to_owned(),
        });
    }
    ensure!(!ratings.is_empty(), "{year}: no nuclear generators in {}", layout.member);
    Ok(ratings)
}

/// Writes `out`: one row per unit and year, from the year's own file where it lists the unit and
/// otherwise from the nearest later year that does.
pub fn run(dir: &Path, out: &Path) -> anyhow::Result<()> {
    let units = Units::catalog()?;
    // (docket, year) → (MW, status, the year whose file it came from).
    let mut found: BTreeMap<(String, i32), (f64, String)> = BTreeMap::new();
    for year in FIRST_YEAR..=LAST_YEAR {
        let ratings = ratings(dir, year)?;
        let mut unknown = Vec::new();
        for rating in &ratings {
            match units.by_eia(&rating.generator) {
                Some(unit) => {
                    if let Some((mw, _)) = found.insert((unit.docket.as_str().to_owned(), year), (rating.summer_mw, rating.status.clone())) {
                        bail!("{year}: {} is listed twice ({mw} MW and {} MW)", unit.name, rating.summer_mw);
                    }
                }
                None => unknown.push(format!("{}-{} ({} MW, {})", rating.generator.plant, rating.generator.generator, rating.summer_mw, rating.status)),
            }
        }
        let listed = found.keys().filter(|(_, listed)| *listed == year).count();
        println!("{year}: {} nuclear generators, {listed} matched to units; unmatched: {unknown:?}", ratings.len());
    }

    let mut text = String::from("docket,year,net_summer_mw,eia860_year,status\n");
    let mut borrowed = Vec::new();
    for unit in units.all() {
        let docket = unit.docket.as_str();
        for year in FIRST_YEAR..=LAST_YEAR {
            let Some((from, (mw, status))) = (year..=LAST_YEAR).find_map(|from| found.get(&(docket.to_owned(), from)).map(|rating| (from, rating))) else {
                continue;
            };
            if from != year {
                borrowed.push(format!("{} {year} from {from}", unit.name));
            }
            writeln!(text, "{docket},{year},{mw},{from},{status}")?;
        }
    }
    fs::write(out, text).with_context(|| format!("writing {}", out.display()))?;
    println!("{} unit-years borrowed from a later file: {borrowed:?}", borrowed.len());
    Ok(())
}
