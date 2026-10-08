use cpal::traits::{DeviceTrait, HostTrait};
use radias_infrastructure::{audio::Player, wav::WaveInput};
use std::{
    path::Path,
    time::{Duration, Instant},
};

fn main() {
    let host = cpal::default_host();
    let Some(device) = host.default_output_device() else {
        eprintln!("No default audio output");
        std::process::exit(1);
    };
    println!("Default output: {device}");
    println!("Default config: {:?}", device.default_output_config());
    match device.supported_output_configs() {
        Ok(configs) => {
            for config in configs {
                println!("Supported: {config:?}");
            }
        }
        Err(error) => eprintln!("Supported configurations: {error}"),
    }
    if let Some(path) = std::env::args().nth(1) {
        let wave = WaveInput::load(Path::new(&path)).expect("Read PCM32 WAV");
        assert!(
            wave.frames
                .iter()
                .any(|frame| frame.iter().any(|&x| x != 0)),
            "Silent input WAV"
        );
        let player = Player::new().expect("Open audio device");
        player.play(
            wave.frames
                .into_iter()
                .map(|frame| frame.map(|x| x as i32))
                .collect(),
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let status = player.status();
            if let Some(error) = status.error {
                panic!("{error}");
            }
            if !status.preview_playing {
                assert!(
                    status.callbacks > 0 && status.audible_frames > 0,
                    "Output callback received no audio"
                );
                println!("Playback verified: {status:?}");
                break;
            }
            assert!(Instant::now() < deadline, "Audio callback timeout");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
