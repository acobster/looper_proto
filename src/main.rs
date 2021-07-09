extern crate anyhow;

use cpal::{StreamConfig};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};

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

    let samples: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(vec![0.0; 44100 * 10]));

    // Split our Arc into consumer/producer so we can read from/write to it
    // within separate input/output audio callbacks.
    let input_samples = samples.clone();

    let input_data_fn = move |data: &[f32], _: &cpal::InputCallbackInfo| {
        for &sample in data {
            (*input_samples.lock().unwrap())[0] = sample;
        }
    };

    let output_data_fn = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
        for sample in data {
            *sample = (*samples.lock().unwrap())[0];
        }
    };

    let input_stream = input.build_input_stream(&config, input_data_fn, err_fn)?;
    let output_stream = output.build_output_stream(&config, output_data_fn, err_fn)?;
    println!("Successfully built streams.");

    input_stream.play()?;
    output_stream.play()?;

    std::thread::sleep(std::time::Duration::from_secs(10));
    drop(input_stream);
    drop(output_stream);
    println!("Done!");

    Ok(())
}

fn err_fn(err: cpal::StreamError) {
    eprintln!("an error occurred on stream: {}", err);
}
