use rand::{thread_rng, Rng};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use rand::distributions::Alphanumeric;

pub fn timeout(timeout: Duration) -> (mpsc::Receiver<()>, Box<dyn FnOnce()>) {
    let (tx, rx) = mpsc::channel();

    // The closure to trigger the timeout
    let tiger = Box::new(move || {
        thread::spawn(move || {
            thread::sleep(timeout);
            let _ = tx.send(());
        });
    });

    (rx, tiger)
}

/// Generates a random container name suitable for Docker
///
/// Returns a lowercase alphanumeric string prefixed with 'c-' to ensure it starts with a letter
pub fn random_container_name() -> String {
    // Generate a random 10-character string
    let random_string: String = thread_rng()
        .sample_iter(&Alphanumeric)
        .take(10)
        .map(char::from)
        .collect::<String>()
        .to_lowercase();

    // Prefix with 'c-' to ensure it starts with a letter (Docker requirement)
    format!("c-{}", random_string)
}

/// Generates a random port number (as a string) in the range 8000-8999.
///
/// Note: This function does not guarantee that the returned port is available.
pub fn random_port() -> String {
    let port = rand::random::<u16>() % 1000 + 8000;
    port.to_string()
}