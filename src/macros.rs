#[macro_export]
macro_rules! trace {
    ( $msg:literal $(, $args:expr)* ) => {
        #[cfg(feature = "logging")]
        log::trace!($msg $(, $args)*);
    }
}

#[macro_export]
macro_rules! debug {
    ( $msg:literal $(, $args:expr)* ) => {
        #[cfg(feature = "logging")]
        log::debug!($msg $(, $args)*);
    }
}

#[macro_export]
macro_rules! info {
    ( $msg:literal $(, $args:expr)* ) => {
        #[cfg(feature = "logging")]
        log::info!($msg $(, $args)*);
    }
}

#[macro_export]
macro_rules! warn {
    ( $msg:literal $(, $args:expr)* ) => {
        #[cfg(feature = "logging")]
        log::warn!($msg $(, $args)*);
    }
}

#[macro_export]
macro_rules! error {
    ( $msg:literal $(, $args:expr)* ) => {
        #[cfg(not(feature = "logging"))]
        eprintln!($msg $(, $args)*);
        #[cfg(feature = "logging")]
        log::error!($msg $(, $args)*);
    }
}
