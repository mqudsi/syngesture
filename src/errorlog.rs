struct ErrorLog {}
static LOG: ErrorLog = ErrorLog {};

pub fn init() {
    log::set_logger(&LOG as &dyn log::Log).unwrap();
    log::set_max_level(log::LevelFilter::Error);
}

impl log::Log for ErrorLog {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("{}", record.args());
        }
    }

    fn flush(&self) {}
}
