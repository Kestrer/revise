#![deny(warnings)]

use revise::term::Term;
use revise::Set;
use std::fs::File;
use std::io::BufReader;
use clap::{App, Arg};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let matches = App::new("revise")
        .version("0.1")
        .about("Utility to help students revise.")
        .author("Koxiaet")
        .arg(Arg::with_name("mode")
            .help("The mode to revise in. Test goes through the terms once, rounds goes through in rounds, each one containing all the incorrect terms from the previous round, and learn helps you learn the terms.")
            .short("m")
            .long("mode")
            .possible_value("test")
            .possible_value("rounds")
            .possible_value("learn")
            .default_value("test")
        )
        .arg(Arg::with_name("type")
            .help("What type of test to give. Has no effect in learn mode.")
            .short("t")
            .long("type")
            .possible_value("choose")
            .possible_value("write")
            .default_value("write")
        )
        .arg(Arg::with_name("inverted")
            .help("Whether to enter terms and be prompted with definitions instead of the other way around. Has no effect in learn mode.")
            .short("i")
            .long("inverted")
        )
        .arg(Arg::with_name("sets")
            .help("The sets to revise")
            .multiple(true)
            .required(true)
        )
        .get_matches();

    let mut files = matches.values_of("sets").unwrap();
    let mode = matches.value_of("mode").unwrap();
    let tester = match (matches.value_of("type").unwrap(), matches.is_present("inverted")) {
        ("choose", false) => Term::choose_definition,
        ("choose", true) => Term::choose_term,
        ("write", false) => Term::write_definition,
        ("write", true) => Term::write_term,
        _ => unreachable!(),
    };

    let mut set: Set = serde_json::from_reader(BufReader::new(File::open(&files.next().unwrap())?))?;
    for file in files {
        set.push_set(serde_json::from_reader(BufReader::new(File::open(&file)?))?);
    }

    match mode {
        "test" => {
            set.shuffle();
            set.test(tester)?;
        },
        "rounds" => set.rounds(tester)?,
        "learn" => set.learn(&[
            Box::new(Term::choose_definition),
            Box::new(Term::write_term),
            Box::new(Term::write_definition),
        ])?,
        _ => unreachable!(),
    }

    Ok(())
}
