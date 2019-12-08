use colored::Colorize;
use rustyline::{error::ReadlineError, EditMode, Editor};
use std::io::{self, Write};
use termion::event::Key;
use termion::input::TermRead;
use termion::raw::IntoRawMode;
use termion::{clear, cursor};

pub fn get_key() -> Result<Key, io::Error> {
    let key = {
        let mut stdout = io::stdout().into_raw_mode()?;
        stdout.flush()?;
        io::stdin()
            .keys()
            .next()
            .ok_or(io::ErrorKind::UnexpectedEof)??
    };
    if key == Key::Ctrl('c') {
        return Err(io::ErrorKind::Interrupted.into());
    }
    Ok(key)
}

pub fn get_one_key() -> Result<Key, io::Error> {
    let key = get_key()?;
    println!();
    Ok(key)
}

//pub fn get_key_where<C>(mut condition: C) -> Result<Key, io::Error>
//where
//    C: FnMut(Key) -> bool,
//{
//    loop {
//        let key = get_key()?;
//        if condition(key) {
//            return Ok(key);
//        }
//    }
//}

pub fn get_key_map<C, R>(mut condition: C) -> Result<R, io::Error>
where
    C: FnMut(Key) -> Option<R>,
{
    loop {
        if let Some(r) = condition(get_key()?) {
            println!();
            return Ok(r);
        }
    }
}

pub fn wait_key() -> Result<(), io::Error> {
    print!("\n...");
    get_key()?;
    Ok(())
}

pub fn get_line(prompt: &str) -> Result<String, ReadlineError> {
    let mut reader: Editor<()> = Editor::with_config(
        rustyline::Config::builder()
            .edit_mode(EditMode::Vi)
            .max_history_size(0)
            .build(),
    );
    Ok(reader.readline(prompt)?)
}

pub fn clear() -> Result<(), io::Error> {
    print!("{}{}", clear::All, cursor::Goto(1, 1));
    Ok(())
}

pub fn show_separator() -> Result<(), io::Error> {
    for _ in 0..termion::terminal_size()?.0 {
        print!("{}", "-".bright_black());
    }
    Ok(())
}
