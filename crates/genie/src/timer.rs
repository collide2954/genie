use std::thread;
use std::time::Duration;

use iced::futures::channel::oneshot;

pub async fn sleep(duration: Duration) {
    let (sender, receiver) = oneshot::channel();
    thread::spawn(move || {
        thread::sleep(duration);
        let _ = sender.send(());
    });
    let _ = receiver.await;
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use iced::futures::executor::block_on;

    use super::sleep;

    #[test]
    fn sleeps_for_the_duration() {
        let started = Instant::now();
        block_on(sleep(Duration::from_millis(30)));
        assert!(started.elapsed() >= Duration::from_millis(30));
    }
}
