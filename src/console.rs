use log::{Level, LevelFilter, Log, Metadata, Record};

/// Status lines on stdout, everything else prefixed by level on stderr.
struct Console;

static CONSOLE: Console = Console;

impl Log for Console {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        match record.level() {
            Level::Info => println!("{}", record.args()),
            Level::Warn => eprintln!("warning: {}", record.args()),
            Level::Error => eprintln!("error: {}", record.args()),
            Level::Debug => eprintln!("debug: {}", record.args()),
            Level::Trace => eprintln!("trace: {}", record.args()),
        }
    }

    fn flush(&self) {}
}

/// `quiet` keeps only warnings and errors; each `-v` raises the level from info towards trace.
pub fn init(verbosity: u8, quiet: bool) {
    let level = match (quiet, verbosity) {
        (true, _) => LevelFilter::Warn,
        (false, 0) => LevelFilter::Info,
        (false, 1) => LevelFilter::Debug,
        (false, _) => LevelFilter::Trace,
    };
    let _ = log::set_logger(&CONSOLE);
    log::set_max_level(level);
}
