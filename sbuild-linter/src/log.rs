#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {
        if crate::CONFIG.lock().unwrap().parallel.is_none() {
            println!("{}", format!($($arg)*))
        }
    }
}

#[macro_export]
macro_rules! einfo {
    ($($arg:tt)*) => {
        if crate::CONFIG.lock().unwrap().parallel.is_none() {
            eprintln!("{}", format!($($arg)*))
        }
    }
}
