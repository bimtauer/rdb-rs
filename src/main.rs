use clap::Parser;
use regex::Regex;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "rdb")]
#[command(override_usage = "rdb [options] dump.rdb")]
struct Cli {
    /// Path to the RDB dump file
    dump_file: PathBuf,

    /// Format to output. Valid: json, plain, nil, protocol
    #[arg(short, long, value_name = "FORMAT")]
    format: Option<String>,

    /// Keys to show. Can be a regular expression
    #[arg(short, long, value_name = "KEYS")]
    keys: Option<String>,

    /// Database to show. Can be specified multiple times
    #[arg(short = 'd', long = "databases", value_name = "DB")]
    databases: Vec<u32>,

    /// Type to show. Can be specified multiple times
    #[arg(short = 't', long = "type", value_name = "TYPE")]
    type_: Vec<String>,

    /// Output file path. If not specified, writes to stdout
    #[arg(short = 'o', long = "output", value_name = "FILE")]
    output: Option<PathBuf>,
}

fn parse_type(type_str: &str) -> Option<rdb::Type> {
    match type_str {
        "string" => Some(rdb::Type::String),
        "list" => Some(rdb::Type::List),
        "set" => Some(rdb::Type::Set),
        "sortedset" | "sorted-set" | "sorted_set" => Some(rdb::Type::SortedSet),
        "hash" => Some(rdb::Type::Hash),
        _ => None,
    }
}

pub fn main() {
    let cli = Cli::parse();
    let mut filter = rdb::filter::Simple::new();

    // Add databases to filter
    for db in cli.databases {
        filter.add_database(db);
    }

    // Add types to filter
    for t in &cli.type_ {
        match parse_type(t) {
            Some(typ) => filter.add_type(typ),
            None => {
                eprintln!("Unknown type: {}", t);
                std::process::exit(1);
            }
        }
    }

    // Add key pattern to filter if specified
    if let Some(k) = cli.keys {
        match Regex::new(&k) {
            Ok(re) => filter.add_keys(re),
            Err(err) => {
                eprintln!("Incorrect regexp: {}", err);
                std::process::exit(1);
            }
        }
    }

    // Open and read the dump file
    let file = match File::open(&cli.dump_file) {
        Ok(f) => f,
        Err(err) => {
            eprintln!("Failed to open {}: {}", cli.dump_file.display(), err);
            std::process::exit(1);
        }
    };
    let reader = BufReader::new(file);

    // Parse with the specified formatter
    let formatter: rdb::FormatterType = match cli.format.as_deref().unwrap_or("json") {
        "json" => rdb::FormatterType::Json(rdb::formatter::JSON::new(cli.output)),
        "plain" => rdb::FormatterType::Plain(rdb::formatter::Plain::new(cli.output)),
        "nil" => rdb::FormatterType::Nil(rdb::formatter::Nil::new(cli.output)),
        "protocol" => rdb::FormatterType::Protocol(rdb::formatter::Protocol::new(cli.output)),
        f => {
            eprintln!("Unknown format: {}", f);
            std::process::exit(1);
        }
    };

    if let Err(err) = rdb::parse(reader, formatter, filter) {
        eprintln!("Failed to parse {}: {}", cli.dump_file.display(), err);
        std::process::exit(1);
    }
}
