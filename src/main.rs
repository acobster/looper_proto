extern crate anyhow;

use cpal::{StreamConfig};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

fn main() -> anyhow::Result<()> {
    // Set up an audio Device.
    let host = cpal::default_host();

    let input = host.default_input_device()
        .expect("no input device available.");
    let output = host.default_output_device()
        .expect("no output device available.");
    println!("Input device: {}", input.name()?);
    println!("Output device: {}", output.name()?);

    let config: StreamConfig = output.default_input_config()?.into();
    println!("Output config:  {:?}", config);

    // Design notes:
    //
    // Below is our vector of samples. Each line between the [ square brackets ]
    // is a loop; each dot is a sample. (This is many thousands of times
    // below the number of samples we actually deal with in a buffer, but this
    // is fine for an illustration.
    //
    // [
    //  .....
    //  .....
    //  .....
    //  ...
    //    ^
    //    |
    //    +----- this is where the current recording ends, and is where the
    //           looper playback index is in this example.
    //
    //
    //
    // ]
    //
    // loop_len = 5
    // loop_count = 3
    //
    // playback:
    // sample_idx = 0..loop_len-1

    let samples: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(vec![0.0; 44100 * 100]));
    let loop_len = Arc::new(AtomicUsize::new(0));
    let recording = Arc::new(AtomicBool::new(true));
    let recording_mut = recording.clone();

    // Clone our Arc so we can read from/write to it
    // within separate input/output audio callbacks.
    let input_samples = samples.clone();
    let input_len = loop_len.clone();
    let input_data_fn = move |data: &[f32], _: &cpal::InputCallbackInfo| {
        if !recording.load(Ordering::Acquire) {
            // We're not recording, save nothing.
            return;
        }
        let mut input_samples_ = input_samples.lock().unwrap();
        for &sample in data {
            let len = input_len.load(Ordering::Acquire);
            input_samples_[len] = sample;
            input_len.store(len+1, Ordering::Release);
            // TODO avoid io in audio thread
            println!("loop_len = {}", len+1);
        }
    };

    println!("RECORDING.");
    let input_stream = input.build_input_stream(&config, input_data_fn, err_fn)?;
    println!("Successfully built streams.");

    input_stream.play()?;
    std::thread::sleep(std::time::Duration::from_secs(3));
    // end of thread::sleep() simulates the user pressing the recording button
    // a second time, signaling the end of the loop recording.
    recording_mut.store(false, Ordering::Release);

    let mut output_idx = 0;
    let output_data_fn = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
        let playback_samples = samples.lock().unwrap();
        for sample in data {
            *sample = playback_samples[output_idx];
            output_idx += 1;
            if output_idx >= loop_len.load(Ordering::Acquire) {
                // RESET THE LOOP PLAYBACK
                output_idx = 0;
            }
        }
        // TODO avoid io in audio thread
        println!("output_idx = {}", output_idx);
    };
    let output_stream = output.build_output_stream(&config, output_data_fn, err_fn)?;
    output_stream.play()?;
    std::thread::sleep(std::time::Duration::from_secs(9));

    drop(input_stream);
    drop(output_stream);
    println!("Done!");

    Ok(())
}

fn err_fn(err: cpal::StreamError) {
    eprintln!("an error occurred on stream: {}", err);
}
