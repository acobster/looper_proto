extern crate anyhow;

use cpal::{Sample, StreamConfig};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
//use fltk::{app, prelude::*, window::Window};
use ringbuf::RingBuffer;

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

    let default_latency = 200.0;

    // configure latency between input/output
    let latency_frames = (default_latency / 1000.0) * config.sample_rate.0 as f32;
    let latency_samples = latency_frames as usize * config.channels as usize;

    let ring = RingBuffer::new(latency_samples * 2);
    let (mut producer, mut consumer) = ring.split();

    for _ in 0..latency_samples {
        // "this should never fail"
        producer.push(0.0).unwrap();
    }

    let input_data_fn = move |data: &[f32], _: &cpal::InputCallbackInfo| {
        let mut output_fell_behind = false;
        for &sample in data {
            if producer.push(sample).is_err() {
                output_fell_behind = true;
            }
        }
        if output_fell_behind {
            eprintln!("output stream fell behind: try increasing latency");
        }
    };

    let output_data_fn = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
        let mut input_fell_behind = false;
        for sample in data {
            *sample = match consumer.pop() {
                // Write each sample from the input stream buffer
                // into the output.
                Some(s) => s,
                None => {
                    input_fell_behind = true;
                    0.0
                }
            }
        }
        // This seems to happen for anything below ~200ms for some reason.
        if input_fell_behind {
            eprintln!("input stream fell behind: try increasing latency");
        }
    };

    let input_stream = input.build_input_stream(&config, input_data_fn, err_fn)?;
    let output_stream = output.build_output_stream(&config, output_data_fn, err_fn)?;
    println!("Successfully built streams.");

    println!("Playing with latency={}ms", default_latency);
    input_stream.play()?;
    output_stream.play()?;

    std::thread::sleep(std::time::Duration::from_secs(10));
    drop(input_stream);
    drop(output_stream);
    println!("Done!");

    // TODO GUI code
    //let app = app::App::default();
    //let mut window = Window::new(100, 100, 400, 300, "Hello from Rust! 🦀");
    //window.end();
    //window.show();
    //app.run().unwrap();

    Ok(())
}

fn err_fn(err: cpal::StreamError) {
    eprintln!("an error occurred on stream: {}", err);
}
