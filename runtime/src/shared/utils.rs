use rand::Rng;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

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

pub fn random_port() -> String {
    let mut rng = rand::thread_rng();
    let port: u16 = rng.gen_range(30000..60000);
    port.to_string()
}

pub fn random_container_name() -> String {
    let mut rng = rand::thread_rng();
    let suffix: u32 = rng.gen();
    format!("invok-container-{}", suffix)
}
