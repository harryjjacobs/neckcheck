use std::time::Duration;

use rodio::source::SineWave;
use rodio::{OutputStream, Sink, Source};

pub fn play_tone(duration: f64) {
    // _stream must live as long as the sink
    let (_stream, stream_handle) = OutputStream::try_default().unwrap();
    let sink = Sink::try_new(&stream_handle).unwrap();

    // Add a dummy source of the sake of the example.
    let source = SineWave::new(440.0)
        .take_duration(Duration::from_secs_f64(duration))
        .amplify(1.0);

    sink.append(source);

    sink.sleep_until_end();
}
