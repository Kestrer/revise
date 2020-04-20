use crossterm::style::Colorize;
use rustyline::{EditMode, Editor};
use std::io::{self, Write, ErrorKind};
use crossterm::{execute, cursor};
use crossterm::terminal::{self, enable_raw_mode, disable_raw_mode, Clear, ClearType};
use crossterm::event::{self, Event, KeyEvent, KeyCode, KeyModifiers};

pub fn get_key() -> Result<KeyEvent, anyhow::Error> {
    enable_raw_mode()?;
    let key = loop {
        if let Event::Key(key) = event::read()? {
            break key;
        }
    };
    disable_raw_mode()?;
    if key == KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL) {
        return Err(anyhow::Error::from(io::Error::from(ErrorKind::Interrupted)));
    }
    Ok(key)
}

pub fn get_key_ln() -> Result<KeyEvent, anyhow::Error> {
    let key = get_key()?;
    println!();
    Ok(key)
}

pub fn get_key_map<R>(mut condition: impl FnMut(KeyEvent) -> Option<R>) -> Result<R, anyhow::Error> {
    loop {
        if let Some(r) = condition(get_key()?) {
            println!();
            return Ok(r);
        }
    }
}

pub fn wait_key() -> Result<(), anyhow::Error> {
    print!("\nPress any key to continue...");
    io::stdout().flush()?;
    get_key()?;
    Ok(())
}

pub fn get_line(prompt: &str) -> Result<String, anyhow::Error> {
    let mut reader: Editor<()> = Editor::with_config(
        rustyline::Config::builder()
            .edit_mode(EditMode::Vi)
            .max_history_size(0)
            .build(),
    );
    Ok(reader.readline(prompt)?)
}

pub fn clear() -> Result<(), anyhow::Error> {
    execute!(io::stdout(), Clear(ClearType::All), cursor::MoveTo(0, 0))?;
    Ok(())
}

pub fn show_separator() -> Result<(), anyhow::Error> {
    for _ in 0..terminal::size()?.0 {
        print!("{}", "-".dark_grey());
    }
    Ok(())
}
