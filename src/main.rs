use clap::{Parser, ValueEnum};
use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Attribute, Cell, CellAlignment, Table};
use console::style;
use dialoguer::{Confirm, Select};
use itertools::Itertools;
use jsonpath_rust::JsonPath;
use jsonpath_rust::query::queryable::Queryable;
use serde::Deserialize;
use serde_json::Value;
use std::fmt::Display;
use std::fs;
use std::path::PathBuf;

#[derive(Parser)]
#[command(author, version, about)]
#[command(arg_required_else_help(true))]
struct Cli {
    filename: PathBuf,

    #[arg(
        long,
        default_value = "https://validator.fhir.diz.uni-marburg.de/validateResource"
    )]
    url: String,

    #[arg(long)]
    show_details: bool,

    #[arg(long)]
    count: bool,

    #[arg(long, value_enum)]
    min_severity: Option<Severity>,
}

#[derive(Deserialize)]
struct Response {
    #[serde(rename = "issue")]
    issues: Vec<Issue>,
}

#[derive(Deserialize)]
struct Issue {
    severity: Severity,
    code: String,
    details: Details,
    expression: Vec<String>,
}

#[derive(Deserialize, PartialOrd, PartialEq, Eq, Ord, Clone, ValueEnum)]
enum Severity {
    #[serde(rename = "error")]
    Error,
    #[serde(rename = "warning")]
    Warning,
    #[serde(rename = "information")]
    Information,
}

impl Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Error => write!(f, "{:<5}", style("ERROR").bold().red()),
            Severity::Warning => write!(f, "{:<5}", style("WARN").bold().yellow()),
            Severity::Information => write!(f, "{:<5}", style("INFO").bold().blue()),
        }
    }
}

#[derive(Deserialize)]
struct Details {
    text: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let fhir = fs::read_to_string(cli.filename).map_err(|err| anyhow::anyhow!(err.to_string()))?;

    if cli.count {
        let bundle =
            serde_json::from_str::<Value>(&fhir).map_err(|err| anyhow::anyhow!(err.to_string()))?;

        println!("{}", style("Used profiles").bold().underlined());

        let mut table = Table::new();
        table
            .load_preset(UTF8_FULL)
            .apply_modifier(UTF8_ROUND_CORNERS);
        table.set_header(vec![
            Cell::new("Profile").add_attribute(Attribute::Bold),
            Cell::new("Count").add_attribute(Attribute::Bold),
        ]);

        let column = table.column_mut(1).expect("table has two columns");
        column.set_cell_alignment(CellAlignment::Right);

        bundle
            .query_only_path("$..meta.profile")
            .iter()
            .flatten()
            .filter_map(|query_path| bundle.reference(query_path))
            .flat_map(|value| value.as_array())
            .flatten()
            .filter_map(|value| value.as_str())
            .map(|profile| {
                let re =
                    regex::Regex::new(r"(.*)/(?<profile>[^/|]+)|(.*)$").expect("invalid regex");
                match re.captures(&profile) {
                    Some(captures) => match captures.name("profile") {
                        Some(capture) => capture.as_str(),
                        None => profile,
                    },
                    None => profile,
                }
            })
            .sorted()
            .chunk_by(|profile| profile.to_string())
            .into_iter()
            .map(|(name, profiles)| (name, profiles.count()))
            .for_each(|(key, count)| {
                table.add_row(vec![key, count.to_string()]);
            });

        println!("{}", table);
    }

    let response = reqwest::blocking::Client::new()
        .post(&cli.url)
        .body(fhir)
        .send()
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;

    let response = response
        .json::<Response>()
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;

    let issues = response.issues.iter().collect::<Vec<_>>();

    let errors = issues
        .iter()
        .filter(|item| item.severity == Severity::Error)
        .count();
    let warnings = issues
        .iter()
        .filter(|item| item.severity == Severity::Warning)
        .count();
    let infos = issues
        .iter()
        .filter(|item| item.severity == Severity::Information)
        .count();

    println!("{}", style("Validation results").bold().underlined());
    println!(
        "{}\n{}\n{}",
        format!("{:<5} {:>5}", style("ERROR").bold().red(), errors),
        format!("{:<5} {:>5}", style("WARN").bold().yellow(), warnings),
        format!("{:<5} {:>5}", style("INFO").bold().blue(), infos),
    );
    println!();

    let show_details = if cli.show_details {
        cli.show_details
    } else {
        Confirm::new()
            .with_prompt("Show details?")
            .default(true)
            .interact()?
    };

    if !show_details {
        return Ok(());
    }

    let min_severity = match cli.min_severity {
        Some(severity) => severity,
        None => {
            let items = vec![Severity::Error, Severity::Warning, Severity::Information];

            match Select::new()
                .with_prompt("Select minimum severity")
                .default(0)
                .items(&items)
                .interact()?
            {
                0 => Severity::Error,
                1 => Severity::Warning,
                _ => Severity::Information,
            }
        }
    };

    let mut issues = issues
        .iter()
        .filter(|issue| issue.severity <= min_severity)
        .collect::<Vec<_>>();

    issues.sort_by_key(|issue| issue.severity.clone());

    println!();
    issues.iter().for_each(|issue| {
        println!(
            "{} {}\n{}\n{}\n",
            style(&issue.severity).bold(),
            style(&issue.code).bold().underlined(),
            issue.expression.join("\n"),
            style(&issue.details.text).dim(),
        );
    });
    println!();

    Ok(())
}
